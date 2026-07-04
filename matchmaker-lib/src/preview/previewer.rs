use ansi_to_tui::IntoText;
use cba::broc::{CommandExt, EnvVars};
use futures::FutureExt;
use log::{debug, error, warn};
use ratatui::text::{Line, Text};
use std::io::BufReader;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::watch::{Receiver, Sender, channel};
use tokio::task::JoinHandle;

use super::AppendOnly;
use crate::config::PreviewerConfig;
use crate::event::EventSender;
use crate::message::Event;
use crate::preview::Preview;

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
    /// The reciever for for [`PreviewMessage`]'s.
    rx: Receiver<PreviewMessage>,
    /// storage for preview command output
    lines: AppendOnly<Line<'static>>,
    /// storage for preview string override
    string: Arc<Mutex<Option<Text<'static>>>>,
    /// Flag which is set to true whenever the state changes
    /// and which the viewer can toggle after receiving the current state
    changed: Arc<AtomicBool>,

    paused: bool,
    /// Maintain a queue of child processes to improve cleanup reliability
    procs: Vec<Child>,
    /// The currently executing child process
    current: Option<(Child, JoinHandle<bool>)>,

    pub config: PreviewerConfig,
    last: String,
    /// Event loop controller
    // We only use it to send [`ControlEvent::Event`]
    event_controller_tx: Option<EventSender>,
}

impl Previewer {
    pub fn new(config: PreviewerConfig) -> (Self, Sender<PreviewMessage>) {
        let (tx, rx) = channel(PreviewMessage::Stop);

        let new = Self {
            rx,
            lines: AppendOnly::new(),
            string: Default::default(),
            changed: Default::default(),
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
            self.string.clone(),
            self.changed.clone(),
        )
    }

    fn signal_dirty(&self) {
        self.changed.store(true, Ordering::Release);
        if let Some(event_tx) = &self.event_controller_tx {
            let _ = event_tx.send(Event::PreviewFinished);
        }
    }

    pub fn set_string(&self, s: Text<'static>) {
        if let Ok(mut guard) = self.string.lock() {
            *guard = Some(s);
            self.signal_dirty();
        }
    }

    pub fn clear_string(&self) {
        if let Ok(mut guard) = self.string.lock() {
            *guard = None;
            self.signal_dirty();
        }
    }

    pub fn has_string(&self) -> bool {
        let guard = self.string.lock();
        guard.is_ok_and(|s| s.is_some())
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
                    continue;
                }

                if self.config.debounce_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(self.config.debounce_ms)).await;
                }

                // wait indefinitely until zombies get cleaned up
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
                        self.set_string(s.clone());
                        self.last.clear();
                        // don't kill the underlying
                        continue;
                    }
                    PreviewMessage::Unset => {
                        self.clear_string();
                        self.last.clear();
                        continue;
                    }
                    _ => {}
                }
            }

            self.dispatch_kill();
            self.clear_string();

            match m {
                PreviewMessage::Run(cmd, variables) => {
                    self.last = cmd.clone();
                    let mut cmd_builder = if let Some(s) = &self.config.shell
                        && !s.is_empty()
                    {
                        let mut iter = s.iter();
                        let mut program = Command::new(iter.next().unwrap());
                        program.args(iter).arg(&cmd);
                        program
                    } else {
                        Command::from_script(&cmd)
                    };

                    cmd_builder
                        .envs(variables.iter().cloned())
                        .stdout(Stdio::piped())
                        .stdin(Stdio::null())
                        .stderr(Stdio::null());

                    #[cfg(not(target_os = "windows"))]
                    cmd_builder.detach();

                    if !self.config.delay_clear {
                        self.lines.clear();
                        self.changed.store(true, Ordering::Release);
                    }
                    // we could send it later to save some cpu but maybe this is more responsive
                    if let Some(event_tx) = &self.event_controller_tx {
                        let _ = event_tx.send(Event::PreviewStarted);
                    }

                    // we need the child handle
                    if let Some(mut child) = cmd_builder._spawn() {
                        if let Some(stdout) = child.stdout.take() {
                            let lines = self.lines.clone();
                            let mut guard = self.lines.read();
                            let changed = self.changed.clone();
                            let cmd_str = cmd.clone();
                            let event_tx = self.event_controller_tx.clone();

                            // false => needs refresh (i.e. invalid utf-8)
                            let handle = tokio::spawn(async move {
                                let mut reader = BufReader::new(stdout);
                                let mut leftover = Vec::new();
                                let mut buf = [0u8; 8192];
                                let mut first = true;

                                while let Ok(n) = std::io::Read::read(&mut reader, &mut buf) {
                                    if n == 0 {
                                        break;
                                    }

                                    if first {
                                        if self.config.delay_clear {
                                            lines.clear();
                                            guard = lines.read(); // get new consistent snapshot
                                            changed.store(true, Ordering::Relaxed);
                                        }
                                        first = false;
                                    }

                                    leftover.extend_from_slice(&buf[..n]);

                                    let valid_up_to = match std::str::from_utf8(&leftover) {
                                        Ok(_) => leftover.len(),
                                        Err(e) => e.valid_up_to(),
                                    };

                                    let split_at = leftover[..valid_up_to]
                                        .iter()
                                        .rposition(|&b| b == b'\n' || b == b'\r')
                                        .map(|pos| pos + 1)
                                        .unwrap_or(valid_up_to);

                                    let (valid_bytes, rest) = leftover.split_at(split_at);

                                    match valid_bytes.into_text() {
                                        Ok(text) => {
                                            for line in text {
                                                // re-check before pushing
                                                if lines.is_expired(&guard) {
                                                    return true;
                                                }
                                                guard.push(line);
                                            }
                                        }
                                        Err(e) => {
                                            if self.config.try_lossy {
                                                for bytes in valid_bytes.split(|b| *b == b'\n') {
                                                    if lines.is_expired(&guard) {
                                                        return true;
                                                    }
                                                    let line =
                                                        String::from_utf8_lossy(bytes).into_owned();
                                                    guard.push(Line::from(line));
                                                }
                                            } else {
                                                error!("Error displaying {cmd_str}: {:?}", e);
                                                return false;
                                            }
                                        }
                                    }

                                    leftover = rest.to_vec();
                                }

                                // no lines read, clear
                                if first && self.config.delay_clear {
                                    lines.clear();
                                    changed.store(true, Ordering::Relaxed);
                                } else if !leftover.is_empty() && !lines.is_expired(&guard) {
                                    match leftover.into_text() {
                                        Ok(text) => {
                                            for line in text {
                                                if lines.is_expired(&guard) {
                                                    return true;
                                                }
                                                guard.push(line);
                                            }
                                        }
                                        Err(e) => {
                                            if self.config.try_lossy {
                                                for bytes in leftover.split(|b| *b == b'\n') {
                                                    if lines.is_expired(&guard) {
                                                        return true;
                                                    }
                                                    let line =
                                                        String::from_utf8_lossy(bytes).into_owned();
                                                    guard.push(Line::from(line));
                                                }
                                            } else {
                                                error!("Error displaying {cmd_str}: {:?}", e);
                                                return false;
                                            }
                                        }
                                    }
                                }
                                if let Some(event_tx) = &event_tx {
                                    let _ = event_tx.send(Event::PreviewFinished);
                                }

                                true
                            });
                            self.current = Some((child, handle))
                        } else {
                            error!("Failed to get stdout of preview command: {cmd}")
                        }
                    }
                }
                PreviewMessage::Stop => {
                    self.lines.clear();
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

            // drop future
            let mut old = Box::pin(old);
            match old.as_mut().now_or_never() {
                Some(Ok(result))
                    // unicode error
                    if !result => {
                        self.send(Event::Redraw)
                    }
                None => {
                    old.abort(); // still works because `AbortHandle` is separate
                }
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

    // todo: This would be cleaner with tokio::Child, but does that merit a conversion? I'm not sure if its worth it for the previewer to yield control while waiting for output cuz we are multithreaded anyways
    // also, maybe don't want this delaying exit?
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

// ---------- NON ANSI VARIANT
// let reader = BufReader::new(stdout);
// if self.config.try_lossy {
// for line_result in reader.split(b'\n') {
//     match line_result {
//         Ok(bytes) => {
//             let line =
//             String::from_utf8_lossy(&bytes).into_owned();
//             lines.push(Line::from(line));
//         }
//         Err(e) => error!("Failed to read line: {:?}", e),
//     }
// }
// } else {
//     for line_result in reader.lines() {
//         match line_result {
//             Ok(line) => lines.push(Line::from(line)),
//             Err(e) => {
//                 // todo: don't know why that even with an explicit ratatui clear, garbage sometimes stays on the screen
//                 error!("Error displaying {cmd}: {:?}", e);
//                 break;
//             }
//         }
//     }
// }

// trait Resettable: Default {
//     fn reset(&mut self) {}
// }
// impl<T> Resettable for AppendOnly<T> {
//     fn reset(&mut self) {
//         self.clear();
//     }
// }

// use std::ops::{Deref, DerefMut};

// #[derive(Debug)]
// struct Queue<V: Resettable> {
//     entries: Vec<(String, V)>,
//     order: Vec<usize>, // indices ordered by recency (0 = most recent)
// }

// impl<V: Resettable> Queue<V> {
//     pub fn new(len: usize) -> Self {
//         Self {
//             entries: (0..len)
//             .map(|_| (String::default(), V::default()))
//             .collect(),
//             order: vec![len; len],
//         }
//     }

//     fn find_key_pos(&self, key: &str) -> Option<(usize, usize)> {
//         for (order_idx, &entries_idx) in self.order.iter().enumerate() {
//             if order_idx == self.entries.len() {
//                 return None
//             }
//             if self.entries[entries_idx].0 == key {
//                 return Some((order_idx, entries_idx));
//             }
//         }
//         None
//     }

//     /// Try to get a key; if found, move it to the top.
//     /// If not found, replace the oldest, clear its vec, set new key.
//     pub fn try_get(&mut self, key: &str) -> bool {
//         let n = self.entries.len();

//         if !key.is_empty() && let Some((order_idx, idx)) = self.find_key_pos(key) {
//             self.order.copy_within(0..order_idx, 1);
//             self.order[0] = idx;
//             true
//         } else {
//             let order_idx = (0..n)
//             .rfind(|&i| self.order[i] < n)
//             .map(|i| i + 1)
//             .unwrap_or(0);

//             let idx = if self.order[order_idx] < self.entries.len() {
//                 order_idx
//             } else {
//                 *self.order.last().unwrap()
//             };

//             // shift and insert at front
//             self.order.copy_within(0..order_idx, 1);
//             self.order[0] = idx;

//             // reset and assign new key
//             let (ref mut k, ref mut v) = self.entries[idx];
//             *k = key.to_owned();
//             v.reset();

//             false
//         }
//     }
// }

// impl<V: Resettable> Deref for Queue<V> {
//     type Target = V;
//     fn deref(&self) -> &Self::Target {
//         &self.entries[self.order[0]].1
//     }
// }

// impl<V: Resettable> DerefMut for Queue<V> {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         &mut self.entries[self.order[0]].1
//     }
// }

// impl<V: Resettable> Default for Queue<V> {
//     fn default() -> Self {
//         Self::new(1)
//     }
// }
