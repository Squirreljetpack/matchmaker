use ansi_to_tui::IntoText;
use futures::FutureExt;
use log::{debug, error, warn};
use ratatui::text::Line;
use std::io::BufReader;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use strum_macros::Display;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch::{Receiver, Sender, channel};
use tokio::task::JoinHandle;

use super::{AppendOnly, EnvVars, spawn};
use crate::proc::Preview;
use crate::config::PreviewerConfig;
use crate::message::Event;

#[derive(Debug, Display, Clone)]
pub enum PreviewMessage {
    Run(String, EnvVars),
    Stop,
}

#[derive(Debug)]
pub struct Previewer {
    rx: Receiver<PreviewMessage>,
    lines: AppendOnly<Line<'static>>, // append-only buffer
    procs: Vec<Child>,
    current: Option<(Child, JoinHandle<bool>)>,
    changed: Arc<AtomicBool>,
    pub config: PreviewerConfig,
    controller_tx: Option<UnboundedSender<Event>>,
}

impl Previewer {
    pub fn new(config: PreviewerConfig) -> (Self, Sender<PreviewMessage>) {
        let (tx, rx) = channel(PreviewMessage::Stop);

        let lines = AppendOnly::new();

        let new = Self {
            rx,
            lines: lines.clone(),
            procs: Vec::new(),
            current: None,
            changed: Default::default(),
            config,
            controller_tx: None,
        };

        (new, tx)
    }

    pub fn view(&self) -> Preview {
        Preview::new(self.lines.clone(), self.changed.clone())
    }

    pub async fn run(mut self) -> Result<(), Vec<Child>> {
        while self.rx.changed().await.is_ok() {
            if !self.procs.is_empty() {
                debug!("procs: {:?}", self.procs);
            }

            self.dispatch_kill();

            match &*self.rx.borrow() {
                PreviewMessage::Run(cmd, variables) => {
                    self.lines.clear();
                    if let Some(mut child) = spawn(
                        cmd,
                        variables.iter().cloned(),
                        Stdio::null(),
                        Stdio::piped(),
                        Stdio::null(),
                    ) {
                        if let Some(stdout) = child.stdout.take() {
                            self.changed.store(true, Ordering::Relaxed);
                            let lines = self.lines.clone();
                            let cmd = cmd.clone();
                            let handle = tokio::spawn(async move {
                                let mut reader = BufReader::new(stdout);
                                let mut leftover = Vec::new();
                                let mut buf = [0u8; 8192];

                                // TODO: want to use buffer over lines (for efficiency?), but partial lines are not handled, and damaged utf-8 still leaks thu somehow
                                while let Ok(n) = std::io::Read::read(&mut reader, &mut buf) {
                                    if n == 0 {
                                        break;
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
                                        .unwrap_or(valid_up_to); // todo: problem if line exceeds

                                    let (valid_bytes, rest) = leftover.split_at(split_at);

                                    match valid_bytes.into_text() {
                                        Ok(text) => {
                                            for line in text {
                                                lines.push(line);
                                            }
                                        }
                                        Err(e) => {
                                            if self.config.try_lossy {
                                                for bytes in valid_bytes.split(|b| *b == b'\n') {
                                                    let line =
                                                        String::from_utf8_lossy(bytes).into_owned();
                                                    lines.push(Line::from(line));
                                                }
                                            } else {
                                                error!("Error displaying {cmd}: {:?}", e);
                                                return false;
                                            }
                                        }
                                    }

                                    leftover = rest.to_vec();
                                }

                                // handle any remaining bytes
                                if !leftover.is_empty() {
                                    match leftover.into_text() {
                                        Ok(text) => {
                                            for line in text {
                                                lines.push(line);
                                            }
                                        }
                                        Err(e) => {
                                            if self.config.try_lossy {
                                                for bytes in leftover.split(|b| *b == b'\n') {
                                                    let line =
                                                        String::from_utf8_lossy(bytes).into_owned();
                                                    lines.push(Line::from(line));
                                                }
                                            } else {
                                                error!("Error displaying {cmd}: {:?}", e);
                                                return false;
                                            }
                                        }
                                    }
                                }
                                true
                            });
                            self.current = Some((child, handle))
                        } else {
                            error!("Failed to get stdout of preview command: {cmd}")
                        }
                    }
                }

                PreviewMessage::Stop => {}
            }

            self.prune_procs();
        }

        let ret = self.cleanup_procs();
        if ret.is_empty() { Ok(()) } else { Err(ret) }
    }

    fn dispatch_kill(&mut self) {
        if let Some((mut child, old)) = self.current.take() {
            let _ = child.kill();
            self.procs.push(child);
            let mut old = Box::pin(old); // pin it to heap

            match old.as_mut().now_or_never() {
                Some(Ok(result)) => {
                    if !result && let Some(ref tx) = self.controller_tx {
                        let _ = tx.send(Event::Refresh);
                    }
                }
                None => {
                    old.abort(); // still works because `AbortHandle` is separate
                }
                _ => {}
            }
        }
    }

    pub fn connect_controller(&mut self, controller_tx: UnboundedSender<Event>) {
        self.controller_tx = Some(controller_tx)
    }

    // todo: This would be cleaner with tokio::Child, but does that merit a conversion? I'm not sure if its worth it for the previewer to yield control while waiting for output cuz we are multithreaded anyways
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
            Ok(None) => true,
            Ok(Some(_)) => false,
            Err(e) => {
                warn!("Error waiting on child: {e}");
                true
            }
        });
    }
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
