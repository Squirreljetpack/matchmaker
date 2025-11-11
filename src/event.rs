use std::collections::HashMap;

use crate::Result;
use crate::action::{Action, Actions, Count};
use crate::binds::{BindMap};
use crate::message::{Event, RenderCommand};
use anyhow::bail;
use crokey::{Combiner, KeyCombination, KeyCombinationFormat, key};
use crossterm::event::{Event as CrosstermEvent, EventStream};
use futures::stream::StreamExt;
use log::{debug, error, info, warn};
use ratatui::layout::Rect;
use tokio::sync::mpsc;
use tokio::time::{self};

#[derive(Debug)]
pub struct EventLoop {
    txs: Vec<mpsc::UnboundedSender<RenderCommand>>,
    tick_interval: time::Duration,

    binds: BindMap,
    combiner: Combiner,
    fmt: KeyCombinationFormat,

    paused: bool,
    event_stream: Option<EventStream>,
    controller_rx: mpsc::UnboundedReceiver<Event>,
    controller_tx: mpsc::UnboundedSender<Event>,
}

impl Default for EventLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLoop {
    pub fn new() -> Self {
        let combiner = Combiner::default();
        let fmt = KeyCombinationFormat::default();
        let event_stream = Some(EventStream::new());
        let (controller_tx, controller_rx) = tokio::sync::mpsc::unbounded_channel();


        Self {
            txs: vec![],
            tick_interval: time::Duration::from_secs(1),

            binds: HashMap::new(),
            combiner,
            fmt,
            event_stream,
            controller_rx,
            controller_tx,

            paused: false
        }
    }


    pub fn add_tx(&mut self, handler: mpsc::UnboundedSender<RenderCommand>) -> &mut Self {
        self.txs.push(handler);
        self
    }

    pub fn clear_txs(&mut self) {
        self.txs.clear();
    }

    pub fn set_tick_rate(&mut self, tick_rate: u8) -> &mut Self {
        self.tick_interval = time::Duration::from_secs_f64(1.0 / tick_rate as f64);
        self
    }

    pub fn get_controller(&self) -> mpsc::UnboundedSender<Event> {
        self.controller_tx.clone()
    }

    fn handle_event(&mut self, e: Event) -> bool {
        debug!("Received: {e}");

        match e {
            Event::Pause => {
                self.paused = true;
                self.send(RenderCommand::Ack);
                self.event_stream = None;
            }
            Event::Refresh => {
                self.send(RenderCommand::Refresh);
            },
            _ => {}
        }
        if let Some(actions) = self.binds.get(&e.into()) {
            self.send_actions(actions);
        }

        self.paused
    }

    pub fn binds(&mut self, binds: BindMap) -> &mut Self {
        self.binds = binds;
        self
    }

    // todo: should its return type carry info
    pub async fn run(&mut self) -> Result<()> {
        let mut interval = time::interval(self.tick_interval);

        loop {
            while self.paused {
                if let Some(event) = self.controller_rx.recv().await {
                    if matches!(event, Event::Resume) {
                        self.paused = false;
                        self.send(RenderCommand::Ack);
                        self.event_stream = Some(EventStream::new());
                        continue;
                    }
                } else {
                    error!("Event controller closed while paused.");
                    break;
                }
            }

            // prioritize pause. Any event that is still processing or already in render queue
            while let Ok(event) = self.controller_rx.try_recv() {
                if self.handle_event(event) {
                }
            };

            self.txs.retain(|tx| !tx.is_closed());
            if self.txs.is_empty() {
                break;
            }

            let event = if let Some(stream) = &mut self.event_stream {
                stream.next()
            } else {
                bail!("No event stream (this should be unreachable)");
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
                        break;
                    }
                }

                Some(event) = self.controller_rx.recv() => {
                    self.handle_event(event);
                    // todo: note that our dynamic event handlers don't detect events originating outside of render currently, maybe we could reseed through render somehow
                }

                // Input ready
                maybe_event = event => {
                    match maybe_event {
                        Some(Ok(event)) => {
                            info!("Event {event:?}");
                            match event {
                                CrosstermEvent::Key(k) => {
                                    info!("{k:?}");
                                    if let Some(key) = self.combiner.transform(k) {
                                        let key = KeyCombination::normalized(key);
                                        if let Some(actions) = self.binds.get(&key.into()) {
                                            self.send_actions(actions);
                                        } else if let Some(c) = key.as_letter() {
                                            self.send(RenderCommand::Input(c));
                                        } else {
                                            // a basic set of keys to prevent confusion
                                            match key {
                                                key!(ctrl-c) | key!(esc) => {
                                                    info!("quitting");
                                                    self.send(RenderCommand::quit());
                                                    break;
                                                }
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
                                                // todo: help action?
                                                _ => {}
                                            }
                                        }
                                        info!("You typed {}", self.print_key(key));
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
                                // CrosstermEvent::FocusLost => {
                                // }
                                // CrosstermEvent::FocusGained => {
                                // }
                                // CrosstermEvent::Paste(_) => {}
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

    fn send(&self, action: RenderCommand) {
        for tx in &self.txs {
            tx.send(action.clone())
            .unwrap_or_else(|_| debug!("Failed to send {action}"));
        }
    }

    fn send_actions(&self, actions: &Actions) {
        for action in actions.0.iter() {
            self.send(action.into());
        }
    }

    fn print_key(&self, key_combination: KeyCombination) -> String {
        self.fmt.to_string(key_combination)
    }

    fn send_action(&self, action: Action) {
        self.send(RenderCommand::Action(action));
    }
}