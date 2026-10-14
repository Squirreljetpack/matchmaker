use ansi_to_tui::IntoText;
use arc_swap::ArcSwap;
use cba::broc::{CommandExt, EnvVars};
use futures::FutureExt;
use log::{debug, error, warn};
use ratatui::text::{Line, Text};
use std::io::BufReader;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::watch::{Receiver, Sender, channel};
use tokio::task::JoinHandle;

use super::{AppendOnly, Override};
use crate::config::PreviewerConfig;
use crate::event::EventSender;
use crate::message::Event;
use crate::preview::Preview;
#[cfg(feature = "image")]
use crate::preview::image::HeaderParser;

#[derive(Debug, Default, strum_macros::Display, Clone)]
pub enum PreviewMessage {
    Run(String, EnvVars),
    Set(Text<'static>),
    Unset,
    #[default]
    Stop,
    Pause,
    Unpause,
}

#[derive(Debug)]
pub struct Previewer {
    /// The receiver for [`PreviewMessage`]'s.
    rx: Receiver<PreviewMessage>,
    /// Storage for newline-terminated preview command output
    lines: AppendOnly<Line<'static>>,
    /// Storage for trailing un-terminated line of preview command output
    last_line: Arc<ArcSwap<Option<Line<'static>>>>,
    /// Storage for preview override (text or image)
    override_: Arc<Mutex<Override>>,
    /// Bumped whenever the previewer starts showing something else.
    generation: Arc<AtomicUsize>,

    paused: bool,
    /// Maintain a queue of child processes to improve cleanup reliability
    procs: Vec<Child>,
    /// The currently executing child process
    current: Option<(Child, JoinHandle<bool>)>,

    pub config: PreviewerConfig,
    last: String,
    /// Event loop controller
    event_controller_tx: Option<EventSender>,
}

impl Previewer {
    pub fn new(config: PreviewerConfig) -> (Self, Sender<PreviewMessage>) {
        let (tx, rx) = channel(PreviewMessage::Stop);

        let new = Self {
            rx,
            lines: AppendOnly::new(),
            last_line: Default::default(),
            override_: Default::default(),
            generation: Default::default(),
            paused: false,

            procs: Vec::new(),
            current: None,

            config,
            last: Default::default(),
            event_controller_tx: None,
        };

        (new, tx)
    }

    pub fn view(&self) -> Preview {
        Preview::new(
            self.lines.clone(),
            self.last_line.clone(),
            self.override_.clone(),
            self.generation.clone(),
        )
    }

    /// Wake the renderer: what the preview shows changed.
    fn signal_dirty(&self) {
        if let Some(event_tx) = &self.event_controller_tx {
            let _ = event_tx.send(Event::PreviewFinished);
        }
    }

    pub fn set_override(&self, value: Override) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut guard) = self.override_.lock() {
            *guard = value;
            self.signal_dirty();
        }
    }

    pub fn set_text(&self, text: Text<'static>) {
        self.set_override(Override::Text(text));
    }

    pub fn clear_override(&self) {
        self.set_override(Override::None);
    }

    pub fn has_override(&self) -> bool {
        let guard = self.override_.lock();
        guard.is_ok_and(|s| s.is_set())
    }

    fn parse_slice(valid_bytes: &[u8], try_lossy: bool, cmd_str: &str) -> Option<Text<'static>> {
        match valid_bytes.into_text() {
            Ok(text) => Some(text),
            Err(e) => {
                if try_lossy {
                    warn!("Error displaying {cmd_str} (falling back to lossy): {e:?}");
                    let lines = valid_bytes
                        .strip_suffix(b"\n")
                        .unwrap_or(valid_bytes)
                        .split(|b| *b == b'\n')
                        .map(|bytes| Line::from(String::from_utf8_lossy(bytes).into_owned()));
                    Some(Text::from_iter(lines))
                } else {
                    error!("Error displaying {cmd_str}: {e:?}");
                    None
                }
            }
        }
    }

    pub async fn run(mut self) -> Result<(), Vec<Child>> {
        while self.rx.changed().await.is_ok() {
            let mut m = self.rx.borrow_and_update().clone();

            if self.config.trim_commands
                && let PreviewMessage::Run(cmd, _) = &mut m
            {
                *cmd = cmd.trim().to_string();
            }

            log::trace!("Previewer received: {m:?}");

            if let PreviewMessage::Run(cmd, _) = &m {
                if !self.config.always_trigger && &self.last == cmd {
                    self.signal_dirty();
                    continue;
                }

                if self.config.debounce_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(self.config.debounce_ms)).await;
                }

                while self.procs.len() >= self.config.max_procs {
                    self.prune_procs();
                    if self.procs.len() < self.config.max_procs {
                        break;
                    }

                    log::error!(
                        "too many zombie processes ({}), delaying preview update ({m:?}).",
                        self.procs.len()
                    );
                    let sleep = self.config.debounce_ms.max(10);
                    tokio::time::sleep(Duration::from_millis(sleep)).await;

                    if self.rx.has_changed().unwrap_or(false) {
                        break;
                    }
                }

                if self.rx.has_changed().unwrap_or(false) {
                    continue;
                }
            }

            if !self.procs.is_empty() {
                debug!("procs: {:?}", self.procs);
            }

            {
                match m {
                    PreviewMessage::Pause => {
                        self.paused = true;
                        continue;
                    }
                    PreviewMessage::Unpause => {
                        self.paused = false;
                        continue;
                    }
                    _ if self.paused => {
                        continue;
                    }
                    PreviewMessage::Set(s) => {
                        self.set_text(s.clone());
                        self.last.clear();
                        continue;
                    }
                    PreviewMessage::Unset => {
                        self.clear_override();
                        self.last.clear();
                        continue;
                    }
                    _ => {}
                }
            }

            self.generation.fetch_add(1, Ordering::Relaxed);
            self.dispatch_kill();

            match m {
                PreviewMessage::Run(cmd, variables) => {
                    self.last = cmd.clone();
                    let mut cmd_builder = Command::from_script(&cmd, &self.config.shell);

                    cmd_builder
                        .envs(variables)
                        .stdout(Stdio::piped())
                        .stdin(Stdio::null())
                        .stderr(Stdio::null());

                    #[cfg(not(target_os = "windows"))]
                    cmd_builder.detach();

                    if !self.config.delay_clear {
                        self.lines.clear();
                        self.last_line.store(Default::default());
                        self.clear_override();
                    }

                    if let Some(event_tx) = &self.event_controller_tx {
                        let _ = event_tx.send(Event::PreviewStarted);
                    }

                    if let Some(mut child) = cmd_builder._spawn() {
                        if let Some(stdout) = child.stdout.take() {
                            let lines = self.lines.clone();
                            let mut guard = self.lines.read();
                            let last_line_shared = self.last_line.clone();
                            let override_ = self.override_.clone();

                            let mut last_line_guard = self.last_line.load_full();
                            let cmd_str = cmd.clone();
                            let generation = Generation {
                                counter: self.generation.clone(),
                                value: self.generation.load(Ordering::Relaxed),
                                event_tx: self.event_controller_tx.clone(),
                            };

                            let handle = tokio::task::spawn_blocking(move || {
                                let mut reader = BufReader::new(stdout);
                                let mut raw_buf = Vec::new();
                                let mut buf = [0u8; 8192];
                                let mut first = true;
                                #[cfg(feature = "image")]
                                let mut image = HeaderParser::new(self.config.images);

                                while let Ok(n) = std::io::Read::read(&mut reader, &mut buf) {
                                    if n == 0 {
                                        break;
                                    }

                                    if generation.superseded(false) {
                                        break;
                                    }

                                    #[cfg(feature = "image")]
                                    match image.feed(&buf[..n], &mut raw_buf) {
                                        HeaderParser::Text
                                        | HeaderParser::Path { remaining: 0, .. }
                                        | HeaderParser::Inline { remaining: 0, .. } => {
                                            if first {
                                                if self.config.delay_clear {
                                                    lines.clear();
                                                    last_line_shared.store(Default::default());
                                                    if let Ok(mut guard) = override_.lock() {
                                                        *guard = Override::None;
                                                    }
                                                    guard = lines.read();
                                                    last_line_guard = last_line_shared.load_full();
                                                }
                                                first = false;
                                            }
                                        }
                                        _ => {
                                            continue;
                                        }
                                    }

                                    #[cfg(not(feature = "image"))]
                                    {
                                        raw_buf.extend_from_slice(&buf[..n]);
                                        if first {
                                            if self.config.delay_clear {
                                                lines.clear();
                                                last_line_shared.store(Default::default());
                                                if let Ok(mut guard) = override_.lock() {
                                                    *guard = Override::None;
                                                }
                                                guard = lines.read();
                                                last_line_guard = last_line_shared.load_full();
                                            }
                                            first = false;
                                        }
                                    }

                                    if let Some(pos) =
                                        raw_buf.iter().rposition(|&b| b == b'\n' || b == b'\r')
                                    {
                                        let (valid_bytes, rest) = raw_buf.split_at(pos + 1);
                                        let Some(text) = Self::parse_slice(
                                            valid_bytes,
                                            self.config.try_lossy,
                                            &cmd_str,
                                        ) else {
                                            generation.superseded(true);
                                            return false;
                                        };

                                        for line in text {
                                            if generation.superseded(true) {
                                                return true;
                                            }
                                            guard.push(line);
                                        }

                                        raw_buf = rest.to_vec();
                                    }

                                    let parsed_last_line = if raw_buf.is_empty() {
                                        None
                                    } else {
                                        let valid_up_to = match std::str::from_utf8(&raw_buf) {
                                            Ok(_) => raw_buf.len(),
                                            Err(e) => e.valid_up_to(),
                                        };
                                        if valid_up_to > 0 {
                                            match (&raw_buf[..valid_up_to]).into_text() {
                                                Ok(t) => t.into_iter().next(),
                                                Err(_) => None, // not try_lossy because we don't want to flash incomplete ansi seq
                                            }
                                        } else {
                                            None
                                        }
                                    };

                                    // todo: implement custom parsing so we can parse to spans incrementally
                                    let next = Arc::new(parsed_last_line);
                                    last_line_shared
                                        .compare_and_swap(&last_line_guard, Arc::clone(&next));
                                    last_line_guard = next;
                                }

                                if generation.superseded(false) {
                                    return true;
                                }

                                if first && self.config.delay_clear {
                                    lines.clear();
                                    last_line_shared.store(Default::default());
                                    if let Ok(mut guard) = override_.lock() {
                                        *guard = Override::None;
                                    }
                                }

                                #[cfg(feature = "image")]
                                if let Some(image) = image.decode(generation.value) {
                                    match image {
                                        Ok(image) => {
                                            match override_.lock() {
                                                Ok(mut guard) => {
                                                    *guard = Override::Image(image);
                                                }
                                                Err(_) => {
                                                    generation.superseded(true);
                                                    return false;
                                                }
                                            };
                                        }
                                        Err(e) => {
                                            error!("Failed to load image for {cmd_str}: {e}");
                                            generation.superseded(true);
                                            return false;
                                        }
                                    };
                                }

                                generation.superseded(true);
                                true
                            });
                            self.current = Some((child, handle))
                        } else {
                            error!("Failed to get stdout of preview command: {cmd}");
                            self.signal_dirty();
                            self.clear_override();
                        }
                    } else {
                        error!("Failed to spawn preview command: {cmd}");
                        self.signal_dirty();
                        self.clear_override();
                    }
                }
                PreviewMessage::Stop => {
                    self.lines.clear();
                    self.last_line.store(Default::default());
                    self.clear_override();
                    if !self.last.is_empty() {
                        self.last.clear();
                        self.signal_dirty();
                    }
                }
                _ => {}
            }

            self.prune_procs();
        }

        let ret = self.cleanup_procs();
        if ret.is_empty() { Ok(()) } else { Err(ret) }
    }

    fn dispatch_kill(&mut self) {
        if let Some((mut child, old)) = self.current.take() {
            kill_child(&mut child);
            self.procs.push(child);

            let mut old = Box::pin(old);
            match old.as_mut().now_or_never() {
                Some(Ok(result)) if !result => self.send(Event::Redraw),
                _ => {}
            }
        }
    }

    fn send(&self, event: Event) {
        if let Some(ref tx) = self.event_controller_tx {
            let _ = tx.send(event);
        }
    }

    pub fn connect_controller(&mut self, event_controller_tx: EventSender) {
        self.event_controller_tx = Some(event_controller_tx)
    }

    fn cleanup_procs(mut self) -> Vec<Child> {
        let total_timeout = Duration::from_secs(1);
        let start = Instant::now();

        self.procs.retain_mut(|child| {
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return false,
                    Ok(None) => {
                        if start.elapsed() >= total_timeout {
                            error!("Child failed to exit in time: {:?}", child);
                            return true;
                        } else {
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                    Err(e) => {
                        error!("Error waiting on child: {e}");
                        return true;
                    }
                }
            }
        });

        self.procs
    }

    fn prune_procs(&mut self) {
        self.procs.retain_mut(|child| match child.try_wait() {
            Ok(None) => {
                kill_child(child);
                true
            }
            Ok(Some(_)) => false,
            Err(e) => {
                warn!("Error waiting on child: {e}");
                true
            }
        });
    }
}

struct Generation {
    counter: Arc<AtomicUsize>,
    value: usize,
    event_tx: Option<EventSender>,
}

impl Generation {
    fn superseded(&self, prepare_return: bool) -> bool {
        let superseded = self.value != self.counter.load(Ordering::Relaxed);
        if prepare_return
            && !superseded
            && let Some(event_tx) = &self.event_tx
        {
            let _ = event_tx.send(Event::PreviewFinished);
        }
        superseded
    }
}

fn kill_child(child: &mut Child) {
    let pid = child.id();

    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        let pgid = Pid::from_raw(-(pid as i32));
        let _ = kill(pgid, Signal::SIGKILL);
    }
    let _ = child.kill();
}
