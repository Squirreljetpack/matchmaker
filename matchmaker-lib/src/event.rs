use crate::action::{Action, ActionExt, NullActionExt};
use crate::binds::BindMap;
use crate::message::{BindDirective, Event, RenderCommand};
use anyhow::Result;
use cli_boilerplate_automation::bait::ResultExt;
use cli_boilerplate_automation::bath::PathExt;
use cli_boilerplate_automation::unwrap;
use crokey::{Combiner, KeyCombination, KeyCombinationFormat, key};
use crossterm::event::{
    Event as CrosstermEvent, EventStream, KeyModifiers, MouseEvent, MouseEventKind,
};
use futures::stream::StreamExt;
use log::{debug, error, info, warn};
use ratatui::layout::Rect;
use std::collections::btree_map::Entry;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::time::{self};

pub type RenderSender<A = NullActionExt> = mpsc::UnboundedSender<RenderCommand<A>>;
pub type EventSender = mpsc::UnboundedSender<Event>;
pub type BindSender<A> = mpsc::UnboundedSender<BindDirective<A>>;

#[derive(Debug)]
pub struct EventLoop<A: ActionExt> {
    txs: Vec<mpsc::UnboundedSender<RenderCommand<A>>>,
    tick_interval: time::Duration,

    pub binds: BindMap<A>,
    combiner: Combiner,
    fmt: KeyCombinationFormat,

    mouse_events: bool,
    paused: bool,
    event_stream: Option<EventStream>,

    rx: mpsc::UnboundedReceiver<Event>,
    controller_tx: mpsc::UnboundedSender<Event>,

    bind_rx: mpsc::UnboundedReceiver<BindDirective<A>>,
    bind_tx: BindSender<A>,

    key_file: Option<PathBuf>,
    current_task: Option<tokio::task::JoinHandle<Result<()>>>,
}

impl<A: ActionExt> Default for EventLoop<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: ActionExt> EventLoop<A> {
    pub fn new() -> Self {
        let combiner = Combiner::default();
        let fmt = KeyCombinationFormat::default();
        let (controller_tx, controller_rx) = tokio::sync::mpsc::unbounded_channel();

        let (bind_tx, bind_rx) = tokio::sync::mpsc::unbounded_channel();

        Self {
            txs: vec![],
            tick_interval: time::Duration::from_millis(200),

            binds: BindMap::new(),
            combiner,
            fmt,
            event_stream: None, // important not to initialize it too early?
            rx: controller_rx,
            controller_tx,

            mouse_events: false,
            paused: false,
            key_file: None,
            current_task: None,

            bind_rx,
            bind_tx,
        }
    }

    pub fn with_binds(binds: BindMap<A>) -> Self {
        let mut ret = Self::new();
        ret.binds = binds;
        ret
    }

    pub fn record_last_key(&mut self, path: PathBuf) -> &mut Self {
        self.key_file = Some(path);
        self
    }

    pub fn with_tick_rate(mut self, tick_rate: u8) -> Self {
        self.tick_interval = time::Duration::from_secs_f64(1.0 / tick_rate as f64);
        self
    }

    pub fn add_tx(&mut self, handler: mpsc::UnboundedSender<RenderCommand<A>>) -> &mut Self {
        self.txs.push(handler);
        self
    }

    pub fn with_mouse_events(mut self) -> Self {
        self.mouse_events = true;
        self
    }

    pub fn clear_txs(&mut self) {
        self.txs.clear();
    }

    pub fn controller(&self) -> EventSender {
        self.controller_tx.clone()
    }
    pub fn bind_controller(&self) -> BindSender<A> {
        self.bind_tx.clone()
    }

    fn handle_event(&mut self, e: Event) {
        debug!("Received: {e}");

        match e {
            Event::Pause => {
                self.paused = true;
                self.send(RenderCommand::Ack);
                self.event_stream = None; // drop because EventStream "buffers" event
            }
            Event::Refresh => {
                self.send(RenderCommand::Refresh);
            }
            _ => {}
        }
        if let Some(actions) = self.binds.get(&e.into()).cloned() {
            self.send_actions(actions);
        }
    }

    fn handle_rebind(&mut self, e: BindDirective<A>) {
        debug!("Received: {e:?}");

        match e {
            BindDirective::Bind(k, v) => {
                self.binds.insert(k, v);
            }

            BindDirective::PushBind(k, v) => match self.binds.entry(k) {
                Entry::Occupied(mut entry) => {
                    entry.get_mut().0.extend(v);
                }
                Entry::Vacant(entry) => {
                    entry.insert(v);
                }
            },

            BindDirective::Unbind(k) => {
                self.binds.remove(&k);
            }

            BindDirective::PopBind(k) => {
                if let Some(actions) = self.binds.get_mut(&k) {
                    actions.0.pop();

                    if actions.0.is_empty() {
                        self.binds.remove(&k);
                    }
                }
            }
        }
    }

    pub fn binds(&mut self, binds: BindMap<A>) -> &mut Self {
        self.binds = binds;
        self
    }

    // todo: should its return type carry info
    pub async fn run(&mut self) {
        self.event_stream = Some(EventStream::new());
        let mut interval = time::interval(self.tick_interval);

        if let Some(path) = self.key_file.clone() {
            log::error!("Cleaning up temp files @ {path:?}");
            tokio::spawn(async move {
                cleanup_tmp_files(&path).await._elog();
            });
        }

        // this loops infinitely until all readers are closed
        loop {
            self.txs.retain(|tx| !tx.is_closed());
            if self.txs.is_empty() {
                break;
            }

            // wait for resume signal
            while self.paused {
                if let Some(event) = self.rx.recv().await {
                    if matches!(event, Event::Resume) {
                        debug!("Resumed from pause");
                        self.paused = false;
                        self.send(RenderCommand::Ack);
                        self.event_stream = Some(EventStream::new());
                        break;
                    }
                } else {
                    error!("Event controller closed while paused.");
                    break;
                }
            }

            // // flush controller events
            // while let Ok(event) = self.rx.try_recv() {
            //    self.handle_event(event)
            // }

            let event = if let Some(stream) = &mut self.event_stream {
                stream.next()
            } else {
                continue; // event stream is removed when paused by handle_event
            };

            tokio::select! {
                _ = interval.tick() => {
                    self.send(RenderCommand::Tick)
                }

                // In case ctrl-c manifests as a signal instead of a key
                _ = tokio::signal::ctrl_c() => {
                    self.record_key("ctrl-c".into());
                    if let Some(actions) = self.binds.get(&key!(ctrl-c).into()).cloned() {
                        self.send_actions(actions);
                    } else {
                        self.send(RenderCommand::quit());
                        info!("Received ctrl-c");
                    }
                }

                Some(event) = self.rx.recv() => {
                    self.handle_event(event)
                }

                Some(directive) = self.bind_rx.recv() => {
                    self.handle_rebind(directive)
                }

                // Input ready
                maybe_event = event => {
                    match maybe_event {
                        Some(Ok(event)) => {
                            if !matches!(
                                event,
                                CrosstermEvent::Mouse(MouseEvent {
                                    kind: crossterm::event::MouseEventKind::Moved,
                                    ..
                                }) |  CrosstermEvent::Key {..}
                            ) {
                                info!("Event {event:?}");
                            }
                            match event {
                                CrosstermEvent::Key(k) => {
                                    if let Some(key) = self.combiner.transform(k) {
                                        info!("{key:?}");
                                        let key = KeyCombination::normalized(key);
                                        if let Some(actions) = self.binds.get(&key.into()).cloned() {
                                            self.record_key(key.to_string());
                                            self.send_actions(actions);
                                        } else if let Some(c) = key_code_as_letter(key) {
                                            self.send(RenderCommand::Action(Action::Char(c)));
                                        } else {
                                            let mut matched = true;
                                            // a basic set of keys to prevent confusion
                                            match key {
                                                key!(ctrl-c) | key!(esc) => {
                                                    self.send(RenderCommand::quit())
                                                },
                                                key!(up) => self.send_action(Action::Up(1)),
                                                key!(down) => self.send_action(Action::Down(1)),
                                                key!(enter) => {
                                                    self.record_key(key.to_string());
                                                    self.send_action(Action::Accept)
                                                }
                                                key!(right) => self.send_action(Action::ForwardChar),
                                                key!(left) => self.send_action(Action::BackwardChar),
                                                key!(ctrl-right) => self.send_action(Action::ForwardWord),
                                                key!(ctrl-left) => self.send_action(Action::BackwardWord),
                                                key!(backspace) => self.send_action(Action::DeleteChar),
                                                key!(ctrl-h) => self.send_action(Action::DeleteWord),
                                                key!(ctrl-u) => self.send_action(Action::Cancel),
                                                key!(alt-h) => self.send_action(Action::Help("".to_string())),
                                                key!(ctrl-'[') => self.send_action(Action::ToggleWrap),
                                                key!(ctrl-']') => self.send_action(Action::TogglePreviewWrap),
                                                _ => {
                                                    matched = false
                                                }
                                            }
                                            if matched {
                                                self.record_key(key.to_string());
                                            }
                                        }
                                    }
                                }
                                CrosstermEvent::Mouse(mouse) => {
                                    if let Some(actions) = self.binds.get(&mouse.into()).cloned() {
                                        self.send_actions(actions);
                                    } else if !matches!(mouse.kind, MouseEventKind::Moved) {
                                        // mouse binds can be disabled by overriding with empty action
                                        // preview scroll can be disabled by overriding scroll event with scroll action
                                        self.send(RenderCommand::Mouse(mouse));
                                    }
                                }
                                CrosstermEvent::Resize(width, height) => {
                                    self.send(RenderCommand::Resize(Rect::new(0, 0, width, height)));
                                }
                                #[allow(unused_variables)]
                                CrosstermEvent::Paste(content) => {
                                    #[cfg(feature = "bracketed-paste")]
                                    {
                                        self.send(RenderCommand::Paste(content));
                                    }
                                    #[cfg(not(feature = "bracketed-paste"))]
                                    {
                                        unreachable!()
                                    }
                                }
                                // CrosstermEvent::FocusLost => {
                                // }
                                // CrosstermEvent::FocusGained => {
                                // }
                                _ => {},
                            }
                        }
                        Some(Err(e)) => warn!("Failed to read crossterm event: {e}"),
                        None => {
                            warn!("Reader closed");
                            break
                        }
                    }
                }
            }
        }
    }

    fn send(&self, action: RenderCommand<A>) {
        for tx in &self.txs {
            tx.send(action.clone())
                .unwrap_or_else(|_| debug!("Failed to send {action}"));
        }
    }

    fn record_key(&mut self, content: String) {
        let Some(path) = self.key_file.clone() else {
            return;
        };

        // Cancel previous task if still running
        if let Some(handle) = self.current_task.take() {
            handle.abort();
        }

        let handle = tokio::spawn(write_to_file(path, content));

        self.current_task = Some(handle);
    }

    fn send_actions<'a>(&self, actions: impl IntoIterator<Item = Action<A>>) {
        for action in actions {
            self.send(action.into());
        }
    }

    pub fn print_key(&self, key_combination: KeyCombination) -> String {
        self.fmt.to_string(key_combination)
    }

    fn send_action(&self, action: Action<A>) {
        self.send(RenderCommand::Action(action));
    }
}

fn key_code_as_letter(key: KeyCombination) -> Option<char> {
    match key {
        KeyCombination {
            codes: crokey::OneToThree::One(crossterm::event::KeyCode::Char(l)),
            modifiers: KeyModifiers::NONE,
        } => Some(l),
        KeyCombination {
            codes: crokey::OneToThree::One(crossterm::event::KeyCode::Char(l)),
            modifiers: KeyModifiers::SHIFT,
        } => Some(l.to_ascii_uppercase()),
        _ => None,
    }
}

use std::path::Path;
use tokio::fs;

/// Cleanup files in the same directory with the same basename, and a .tmp extension
async fn cleanup_tmp_files(path: &Path) -> Result<()> {
    let parent = unwrap!(path.parent(); Ok(()));
    let name = unwrap!(path.file_name().and_then(|s| s.to_str()); Ok(()));

    let mut entries = fs::read_dir(parent).await?;

    while let Some(entry) = entries.next_entry().await? {
        let entry_path = entry.path();

        if let Ok(filename) = entry_path._filename()
            && let Some(e) = filename.strip_prefix(name)
            && e.starts_with('.')
            && e.ends_with(".tmp")
        {
            fs::remove_file(entry_path).await._elog();
        }
    }

    Ok(())
}

/// Spawns a thread that writes `content` to `path` atomically using a temp file.
/// Returns the `JoinHandle` so you can wait for it if desired.
pub async fn write_to_file(path: PathBuf, content: String) -> Result<()> {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let tmp_path = path.with_file_name(format!("{}.{}.tmp", path._filename()?, suffix));

    // Write temp file
    fs::write(&tmp_path, &content).await?;

    // Atomically replace target
    fs::rename(&tmp_path, &path).await?;

    Ok(())
}
