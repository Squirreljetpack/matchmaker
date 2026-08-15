use crate::action::{Action, ActionExt, Actions, NullActionExt};
use crate::binds::{BindMap, BindMapExt, ResolvedBindMap, SimpleMouseEvent, TriggerKind};
use crate::message::{BindDirective, Event, RenderCommand};
use anyhow::Result;
use arc_swap::ArcSwap;
use cba::bait::ResultExt;
use cba::bath::PathExt;
use cba::{_info, unwrap};
use crokey::{Combiner, KeyCombination, KeyCombinationFormat, key};
use crossterm::event::{
    Event as CrosstermEvent, EventStream, KeyModifiers, MouseEvent, MouseEventKind,
};
use futures::stream::StreamExt;
use log::{debug, error, info, warn};
use ratatui::layout::Rect;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{self};

pub type RenderSender<A = NullActionExt> = mpsc::UnboundedSender<RenderCommand<A>>;
pub type EventSender = mpsc::UnboundedSender<Event>;
pub type BindSender<A> = mpsc::UnboundedSender<BindDirective<A>>;

#[derive(Debug)]
pub struct EventLoop<A: ActionExt> {
    txs: Vec<RenderSender<A>>,
    tick_interval: time::Duration,
    tick_rate_override: Option<u8>,
    interval_dirty: bool,
    paused: bool,
    skip_ticks: [bool; 2],
    dirty: bool,

    binds: Arc<ArcSwap<ResolvedBindMap<A>>>,
    original_binds: BindMap<A>,
    combiner: Combiner,
    fmt: KeyCombinationFormat,

    mouse_events: bool,
    scroll_debounce: time::Duration,
    scroll_buffer: Vec<MouseEvent>,
    scroll_deadline: Option<std::pin::Pin<Box<time::Sleep>>>,
    event_stream: Option<EventStream>,
    optional_stream: bool,

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
            tick_rate_override: None,
            interval_dirty: false,
            skip_ticks: [false, true],
            paused: false,
            dirty: true,

            binds: Arc::new(ArcSwap::from_pointee(ResolvedBindMap::new())),
            original_binds: BindMap::new(),
            combiner,
            fmt,
            event_stream: None, // important not to initialize it too early?
            optional_stream: false,
            rx: controller_rx,
            controller_tx,

            mouse_events: false,
            scroll_debounce: time::Duration::from_millis(0),
            scroll_buffer: Vec::new(),
            scroll_deadline: None,
            key_file: None,
            current_task: None,

            bind_rx,
            bind_tx,
        }
    }

    /// Runs the loop without creating a crossterm event stream; input events
    /// never arrive (the input future never resolves). Intended for headless
    /// runs (e.g. tests) where there is no terminal to read from.
    pub fn as_optional(mut self) -> Self {
        self.optional_stream = true;
        self
    }

    pub fn with_binds(binds: BindMap<A>) -> Self {
        let mut ret = Self::new();
        ret.original_binds = binds.clone();
        let mode = MODE.lock().unwrap();
        let resolved = binds.resolve_semantics(&mode);
        ret.binds = Arc::new(ArcSwap::from_pointee(resolved));
        #[cfg(not(debug_assertions))]
        log::trace!("Resolved with mode {mode:?}: {:?}", ret.binds);
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

    /// Tick interval in effect: the tick rate override when set, the base
    /// tick rate otherwise.
    fn effective_tick_interval(&self) -> time::Duration {
        self.tick_rate_override
            .map(|rate| time::Duration::from_secs_f64(1.0 / rate.max(1) as f64))
            .unwrap_or(self.tick_interval)
    }

    pub fn get_binds_ptr(&self) -> Arc<ArcSwap<ResolvedBindMap<A>>> {
        self.binds.clone()
    }

    pub fn binds(&self) -> Arc<ResolvedBindMap<A>> {
        self.binds.load_full()
    }

    /// Returns a reference to the original (unresolved) bind map.
    /// Useful for operations that need access to the full bind map with mode information,
    /// such as help display or trace checking.
    pub fn original_binds(&self) -> &BindMap<A> {
        &self.original_binds
    }

    pub fn add_tx(&mut self, handler: RenderSender<A>) -> &mut Self {
        self.txs.push(handler);
        self
    }

    pub fn with_mouse_events(mut self, enabled: bool) -> Self {
        self.mouse_events = enabled;
        self
    }

    pub fn with_scroll_debounce(mut self, ms: u64) -> Self {
        self.scroll_debounce = time::Duration::from_millis(ms);
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

    fn get_bind(&self, kind: TriggerKind) -> Option<Actions<A>> {
        let binds = self.binds.load();
        binds.get(&kind).cloned()
    }

    fn handle_event(&mut self, e: Event) {
        debug!("Received event: {e}");
        self.dirty = true;

        for flag in e.iter() {
            match flag {
                Event::Pause => {
                    self.paused = true;
                    self.send(RenderCommand::Ack);
                    self.event_stream = None; // drop because EventStream "buffers" event
                }
                Event::Redraw => {
                    self.send(RenderCommand::Redraw);
                }
                Event::Synced | Event::Resynced => {
                    self.skip_ticks[0] = true;
                }
                Event::PreviewFinished => {
                    self.skip_ticks[1] = true;
                }
                Event::Restarted | Event::Reloaded => {
                    self.skip_ticks[0] = false;
                }
                Event::PreviewStarted => {
                    self.skip_ticks[1] = false;
                }
                _ => {}
            }

            if let Some(actions) = self.get_bind(TriggerKind::Event(flag)) {
                self.send_actions(actions, None);
            }
        }
    }

    fn handle_rebind(&mut self, e: BindDirective<A>) {
        debug!("Received: {e:?}");

        match e {
            BindDirective::Bind(k, v) => {
                self.original_binds.insert(k, v);
            }

            BindDirective::PushBind(k, v) => {
                self.original_binds.entry(k).or_default().0.push(v);
            }

            BindDirective::Unbind(k) => {
                self.original_binds.remove(&k);
            }

            BindDirective::PopBind(k) => {
                if let Some(actions) = self.original_binds.get_mut(&k) {
                    actions.0.pop();

                    if actions.0.is_empty() {
                        self.original_binds.remove(&k);
                    }
                }
            }

            BindDirective::SetMode(s) => {
                set_mode(&s);
            }

            BindDirective::PushMode(s) => {
                let trimmed = s.trim();
                if !trimmed.is_empty()
                    && let Ok(mut mode) = MODE.lock()
                {
                    mode.push(trimmed.into());
                }
            }

            BindDirective::PopMode => {
                if let Ok(mut mode) = MODE.lock() {
                    mode.pop();
                }
            }

            BindDirective::OverrideTickrate(rate) => {
                self.tick_rate_override = rate;
                self.interval_dirty = true;
                return;
            }

            BindDirective::Action(action) => {
                self.send_actions(std::iter::once(action), None);
                return;
            }
        }
        let binds = self.original_binds.clone();
        let mode = MODE.lock().unwrap();
        let resolved = binds.resolve_semantics(&mode);
        self.binds.store(Arc::new(resolved));
    }

    // todo: should its return type carry info
    pub async fn run(&mut self) {
        // log::trace!("{:?}", self.binds.load());
        if !self.optional_stream {
            self.event_stream = Some(EventStream::new());
        }
        let mut interval = time::interval(self.tick_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        if let Some(path) = self.key_file.clone() {
            // log::debug!("Cleaning up temp files @ {path:?}");
            tokio::spawn(async move {
                cleanup_tmp_files(&path).await._elog();
            });
        }

        // this loops infinitely until all readers are closed
        loop {
            self.txs.retain(|tx| !tx.is_closed());
            if self.txs.is_empty() {
                log::trace!("Event loop completed");
                break;
            }

            // Recreate the tick interval when the override changed. A fresh
            // tokio interval resolves its first tick immediately, so this
            // yields one prompt tick at the new rate instead of looping.
            if self.interval_dirty {
                interval = time::interval(self.effective_tick_interval());
                interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
                self.interval_dirty = false;
            }

            // wait for resume signal
            while self.paused {
                if let Some(event) = self.rx.recv().await {
                    if matches!(event, Event::Resume) {
                        debug!("Resumed from pause");
                        self.paused = false;
                        self.dirty = true;
                        self.send(RenderCommand::Ack);
                        self.send(RenderCommand::Redraw);
                        if !self.optional_stream {
                            self.event_stream = Some(EventStream::new());
                        }
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
                futures::future::Either::Left(stream.next())
            } else if self.optional_stream {
                futures::future::Either::Right(futures::future::pending())
            } else {
                continue; // event stream is removed when paused by handle_event
            };

            tokio::select! {
                biased;

                _ = interval.tick() => {
                    if self.tick_rate_override.is_some()
                        || !self.skip_ticks.iter().all(|x| *x)
                        || self.dirty
                    {
                        _info!("event tick": self.dirty);
                        self.send(RenderCommand::Tick)
                    }
                    self.dirty = false;
                }

                // In case ctrl-c manifests as a signal instead of a key
                _ = tokio::signal::ctrl_c() => {
                    self.record_key("ctrl-c".into());
                    if let Some(actions) = self.get_bind(TriggerKind::Key(key!(ctrl-c))) {
                        self.send_actions(actions, Some("ctrl-c".into()));
                    } else {
                        self.send(RenderCommand::quit());
                        info!("Received ctrl-c");
                    }
                }

                // Scroll debounce deadline — flush all buffered scroll events
                // together so the render loop processes them as a single batch.
                _ = scroll_deadline_future(&mut self.scroll_deadline) => {
                    self.flush_scroll_buffer();
                    self.scroll_deadline = None;
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
                                })
                            ) {
                                if matches!(event,  CrosstermEvent::Key {..}) {
                                    info!("Event {event:?}");
                                }
                            }
                            match event {
                                CrosstermEvent::Key(k) => {
                                    if let Some(key) = self.combiner.transform(k) {
                                        info!("{key:?}");
                                        let key = KeyCombination::normalized(key);
                                        if let Some(actions) = self.get_bind(TriggerKind::Key(key)) {
                                            self.record_key(key.to_string());
                                            self.send_actions(actions, Some(key.to_string()));
                                        } else if let Some(c) = key_code_as_letter(key) {                                            self.send(RenderCommand::Action(Action::Char(c)));
                                        } else {
                                            let mut matched = true;
                                            // a basic set of keys to ensure basic usability
                                            match key {
                                                key!(ctrl-c) | key!(esc) => {
                                                    self.send(RenderCommand::quit())
                                                },
                                                key!(up) => self.send_action(Action::Up(1)),
                                                key!(down) => self.send_action(Action::Down(1)),
                                                key!(enter) => self.send_action(Action::Accept),
                                                key!(right) => self.send_action(Action::ForwardChar),
                                                key!(left) => self.send_action(Action::BackwardChar),
                                                key!(ctrl-right) => self.send_action(Action::ForwardWord),
                                                key!(ctrl-left) => self.send_action(Action::BackwardWord),
                                                key!(backspace) => self.send_action(Action::DeleteChar),
                                                key!(ctrl-h) => self.send_action(Action::DeleteWord),
                                                key!(ctrl-u) => self.send_action(Action::ClearQuery),
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
                                    if !self.mouse_events {
                                        continue;
                                    };
                                    if let Some(actions) = self.get_bind(TriggerKind::Mouse(SimpleMouseEvent {
                                        kind: mouse.kind,
                                        modifiers: mouse.modifiers,
                                    })) {
                                        self.send_actions(actions, None);
                                    } else if !matches!(mouse.kind, MouseEventKind::Moved) {
                                        if is_scroll_kind(&mouse.kind) && !self.scroll_debounce.is_zero()
                                        {
                                            // Buffer scroll events and reset/start the debounce timer.
                                            // When the timer fires, all buffered scrolls are flushed
                                            // together so the render loop sees them as a single batch.
                                            self.scroll_buffer.push(mouse);
                                            self.scroll_deadline =
                                                Some(Box::pin(time::sleep(self.scroll_debounce)));
                                        } else {
                                            // mouse binds can be disabled by overriding with empty action
                                            // preview scroll can be disabled by overriding scroll event with scroll action
                                            self.send(RenderCommand::Mouse(mouse));
                                        }
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

    fn send_actions<'a>(
        &mut self,
        actions: impl IntoIterator<Item = Action<A>>,
        key: Option<String>,
    ) {
        for action in actions {
            match action {
                Action::PrintKey => {
                    if let Some(k) = &key {
                        self.send(Action::Print(k.clone()).into());
                    }
                }
                Action::Semantic(s) => {
                    if let Some(actions) = self.get_bind(TriggerKind::Semantic(s)) {
                        self.send_actions(actions, None);
                    }
                }
                #[cfg(not(debug_assertions))]
                Action::Trace(_) => {}
                _ => self.send(action.into()),
            }
        }
    }

    pub fn print_key(&self, key_combination: KeyCombination) -> String {
        self.fmt.to_string(key_combination)
    }

    fn send_action(&self, action: Action<A>) {
        self.send(RenderCommand::Action(action));
    }

    /// Drain the scroll buffer, sending every buffered scroll event through the
    /// unbounded channel back-to-back. Because `UnboundedSender::send` does not
    /// await, the events are all enqueued before the render loop's next
    /// `recv_many` call, so the render loop processes them as a single batch and
    /// performs one re-render after all of them have been applied.
    fn flush_scroll_buffer(&mut self) {
        if self.scroll_buffer.is_empty() {
            return;
        }
        let buffered = std::mem::take(&mut self.scroll_buffer);
        for mouse in buffered {
            self.send(RenderCommand::Mouse(mouse));
        }
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

/// Returns true if the given mouse event kind is a scroll event
/// (ScrollUp, ScrollDown, ScrollLeft, or ScrollRight).
fn is_scroll_kind(kind: &crossterm::event::MouseEventKind) -> bool {
    matches!(
        kind,
        crossterm::event::MouseEventKind::ScrollUp
            | crossterm::event::MouseEventKind::ScrollDown
            | crossterm::event::MouseEventKind::ScrollLeft
            | crossterm::event::MouseEventKind::ScrollRight
    )
}

/// Adapter for use in `tokio::select!` that awaits the optional scroll
/// debounce deadline. When no deadline is set, the future is pending forever
/// so this branch never fires.
async fn scroll_deadline_future(deadline: &mut Option<std::pin::Pin<Box<time::Sleep>>>) {
    match deadline.as_mut() {
        Some(sleep) => sleep.as_mut().await,
        None => std::future::pending().await,
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

        if let Ok(filename) = entry_path.filename()
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

    let tmp_path = path.with_file_name(format!("{}.{}.tmp", path.filename()?, suffix));

    // Write temp file
    fs::write(&tmp_path, &content).await?;

    // Atomically replace target
    fs::rename(&tmp_path, &path).await?;

    Ok(())
}
// -----------------------------------------
pub static MODE: std::sync::Mutex<Vec<Box<str>>> = std::sync::Mutex::new(Vec::new());

/// Set the current mode stack from a comma-separated string.
/// Empty segments are filtered out. If the lock is poisoned, the call is a no-op.
pub fn set_mode(mode: &str) {
    if let Ok(mut m) = MODE.lock() {
        *m = mode
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.into())
            .collect();
        log::trace!("Set mode: {mode}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::NullActionExt;

    #[test]
    fn test_handle_combined_events() {
        let mut loop_handle = EventLoop::<NullActionExt>::new();
        assert_eq!(loop_handle.skip_ticks, [false, true]);

        loop_handle.handle_event(Event::CursorChange | Event::Synced);
        assert_eq!(loop_handle.skip_ticks[0], true);

        loop_handle.handle_event(Event::PreviewFinished);
        assert_eq!(loop_handle.skip_ticks[1], true);
        assert!(loop_handle.skip_ticks.iter().all(|x| *x));
    }

    #[test]
    fn override_tickrate_sets_fields_and_interval() {
        let mut loop_handle = EventLoop::<NullActionExt>::new();
        assert!(loop_handle.tick_rate_override.is_none());
        assert!(!loop_handle.interval_dirty);

        loop_handle.handle_rebind(BindDirective::OverrideTickrate(Some(20)));
        assert_eq!(loop_handle.tick_rate_override, Some(20));
        assert!(loop_handle.interval_dirty);

        loop_handle.handle_rebind(BindDirective::OverrideTickrate(None));
        assert_eq!(loop_handle.tick_rate_override, None);
    }
}
