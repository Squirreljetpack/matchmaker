use crate::Result;
use crate::action::{Action, ActionExt, Count};
use crate::binds::BindMap;
use crate::message::{Event, RenderCommand};
use crokey::{Combiner, KeyCombination, KeyCombinationFormat, key};
use crossterm::event::{Event as CrosstermEvent, EventStream, KeyModifiers, MouseEvent};
use futures::stream::StreamExt;
use log::{debug, error, info, warn};
use ratatui::layout::Rect;
use tokio::sync::mpsc;
use tokio::time::{self};

pub type RenderSender<A> = mpsc::UnboundedSender<RenderCommand<A>>;
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
    controller_rx: mpsc::UnboundedReceiver<Event>,
    controller_tx: mpsc::UnboundedSender<Event>,
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

        Self {
            txs: vec![],
            tick_interval: time::Duration::from_secs(1),

            binds: BindMap::new(),
            combiner,
            fmt,
            event_stream: None, // important not to initialize it too early?
            controller_rx,
            controller_tx,

            mouse_events: false,
            paused: false,
        }
    }

    pub fn with_binds(binds: BindMap<A>) -> Self {
        let mut ret = Self::new();
        ret.binds = binds;
        ret
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

    pub fn get_controller(&self) -> mpsc::UnboundedSender<Event> {
        self.controller_tx.clone()
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
        if let Some(actions) = self.binds.get(&e.into()) {
            self.send_actions(actions);
        }
    }

    pub fn binds(&mut self, binds: BindMap<A>) -> &mut Self {
        self.binds = binds;
        self
    }

    // todo: should its return type carry info
    pub async fn run(&mut self) -> Result<()> {
        self.event_stream = Some(EventStream::new());
        let mut interval = time::interval(self.tick_interval);

        // this loops infinitely until all readers are closed
        loop {
            // wait for resume signal
            while self.paused {
                if let Some(event) = self.controller_rx.recv().await {
                    if matches!(event, Event::Resume) {
                        log::debug!("Resumed from pause");
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

            // flush controller events
            while let Ok(event) = self.controller_rx.try_recv() {
                // todo: note that our dynamic event handlers don't detect events originating outside of render currently, tho maybe we could reseed here
                self.handle_event(event)
            }

            self.txs.retain(|tx| !tx.is_closed());
            if self.txs.is_empty() {
                break;
            }

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
                    if let Some(actions) = self.binds.get(&key!(ctrl-c).into()) {
                        self.send_actions(actions);
                    } else {
                        self.send(RenderCommand::quit());
                        info!("Received ctrl-c");
                    }
                }

                Some(event) = self.controller_rx.recv() => {
                    self.handle_event(event);
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
                                info!("Event {event:?}");
                            }
                            match event {
                                CrosstermEvent::Key(k) => {
                                    info!("{k:?}");
                                    if let Some(key) = self.combiner.transform(k) {
                                        info!("{key:?}");
                                        let key = KeyCombination::normalized(key);
                                        if let Some(actions) = self.binds.get(&key.into()) {
                                            self.send_actions(actions);
                                        } else if let Some(c) = key_code_as_letter(key) {
                                            self.send(RenderCommand::Action(Action::Input(c)));
                                        } else {
                                            // a basic set of keys to prevent confusion
                                            match key {
                                                key!(ctrl-c) | key!(esc) => self.send(RenderCommand::quit()),
                                                key!(up) => self.send_action(Action::Up(Count(1))),
                                                key!(down) => self.send_action(Action::Down(Count(1))),
                                                key!(enter) => self.send_action(Action::Accept),
                                                key!(right) => self.send_action(Action::ForwardChar),
                                                key!(left) => self.send_action(Action::BackwardChar),
                                                key!(ctrl-right) => self.send_action(Action::ForwardWord),
                                                key!(ctrl-left) => self.send_action(Action::BackwardWord),
                                                key!(backspace) => self.send_action(Action::DeleteChar),
                                                key!(ctrl-h) => self.send_action(Action::DeleteWord),
                                                key!(ctrl-u) => self.send_action(Action::Cancel),
                                                key!(alt-h) => self.send_action(Action::Help("".to_string())),
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                                CrosstermEvent::Mouse(mouse) => {
                                    if let Some(actions) = self.binds.get(&mouse.into()) {
                                        self.send_actions(actions);
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
        Ok(())
    }

    fn send(&self, action: RenderCommand<A>) {
        for tx in &self.txs {
            tx.send(action.clone())
                .unwrap_or_else(|_| debug!("Failed to send {action}"));
        }
    }

    fn send_actions<'a>(&self, actions: impl IntoIterator<Item = &'a Action<A>>) {
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
