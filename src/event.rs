use std::collections::HashMap;

use crate::Result;
use crate::action::{Actions};
use crate::binds::{BindMap};
use crate::message::{Event, RenderCommand};
use anyhow::bail;
use crokey::{Combiner, KeyCombination, KeyCombinationFormat, key};
use crossterm::event::{Event as CrosstermEvent, EventStream};
use futures::stream::StreamExt;
use log::{debug, error, info, warn};
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
}

impl EventLoop {
    pub fn new(
        handler_channels: impl IntoIterator<Item = mpsc::UnboundedSender<RenderCommand>>,
        tick_rate: u16,
    ) -> (Self, mpsc::UnboundedSender<Event>) {
        let combiner = Combiner::default();
        let fmt = KeyCombinationFormat::default();
        let event_stream = Some(EventStream::new());
        let (controller_tx, controller_rx) = tokio::sync::mpsc::unbounded_channel();
        
        let new = Self {
            txs: handler_channels.into_iter().collect(),
            tick_interval: time::Duration::from_secs_f64(1.0 / tick_rate as f64),
            
            binds: HashMap::new(),
            combiner,
            fmt,
            event_stream,
            controller_rx,
            
            paused: false
        };
        (new, controller_tx)
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
                if let Some(picker_event) = self.controller_rx.recv().await {
                    if matches!(picker_event, Event::Resume) {
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
            while let Ok(picker_event) = self.controller_rx.try_recv() {
                if self.handle_event(picker_event) {
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
                
                Some(picker_event) = self.controller_rx.recv() => {
                    self.handle_event(picker_event);
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
                                            match key {
                                                key!(ctrl-c) | key!(esc) => {
                                                    info!("quitting");
                                                    self.send(RenderCommand::quit());
                                                    break;
                                                }
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
                                #[allow(unused)]
                                CrosstermEvent::Resize(width, height) => {
                                    // self.send_actions(&[Action::Resize(ratatui::layout::Rect::new(0, 0, width, height))].into());
                                }
                                CrosstermEvent::FocusLost => {
                                }
                                CrosstermEvent::FocusGained => {
                                }
                                CrosstermEvent::Paste(_) => {}
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
}