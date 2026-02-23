use ansi_to_tui::IntoText;
use cli_boilerplate_automation::broc::{CommandExt, EnvVars};
use futures::FutureExt;
use log::{debug, error, warn};
use ratatui::text::{Line, Text};
use std::io::BufReader;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

#[derive(Debug, strum_macros::Display, Clone)]
pub enum PreviewMessage {
    Run(String, EnvVars),
    Set(Text<'static>),
    Unset,
    Stop,
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

    /// Incremented on every new run / kill to invalidate old feeders
    version: Arc<AtomicUsize>,
    /// Maintain a queue of child processes to improve cleanup reliability
    procs: Vec<Child>,
    /// The currently executing child process
    current: Option<(Child, JoinHandle<bool>)>,
    pub config: PreviewerConfig,
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
            version: Arc::new(AtomicUsize::new(0)),

            procs: Vec::new(),
            current: None,
            config,
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

    pub fn set_string(&self, s: Text<'static>) {
        let mut guard = self.string.lock().unwrap();
        *guard = Some(s);
        self.changed.store(true, Ordering::Release);
    }

    pub fn clear_string(&self) {
        let mut guard = self.string.lock().unwrap();
        *guard = None;
        self.changed.store(true, Ordering::Release);
    }

    pub fn has_string(&self) -> bool {
        let guard = self.string.lock().unwrap();
        guard.is_some()
    }

    pub async fn run(mut self) -> Result<(), Vec<Child>> {
        while self.rx.changed().await.is_ok() {
            if !self.procs.is_empty() {
                debug!("procs: {:?}", self.procs);
            }

            {
                let m = &*self.rx.borrow();
                if let PreviewMessage::Set(s) = m {
                    self.set_string(s.clone());
                    // don't kill the underlying
                    continue;
                } else if matches!(m, PreviewMessage::Unset) {
                    self.clear_string();
                    continue;
                }
            }

            self.dispatch_kill();
            self.clear_string();
            self.lines.clear();

            match &*self.rx.borrow() {
                PreviewMessage::Run(cmd, variables) => {
                    if let Some(mut child) = Command::from_script(cmd)
                        .envs(variables.iter().cloned())
                        .stdout(Stdio::piped())
                        .stdin(Stdio::null())
                        .stderr(Stdio::null())
                        .detach()
                        ._spawn()
                    {
                        if let Some(stdout) = child.stdout.take() {
                            self.changed.store(true, Ordering::Relaxed);

                            let lines = self.lines.clone();
                            let cmd = cmd.clone();
                            let version = self.version.clone();
                            let _version = version.load(Ordering::Acquire);

                            // false => needs refresh (i.e. invalid utf-8)
                            let handle = tokio::spawn(async move {
                                let mut reader = BufReader::new(stdout);
                                let mut leftover = Vec::new();
                                let mut buf = [0u8; 8192];

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
                                        .unwrap_or(valid_up_to);

                                    let (valid_bytes, rest) = leftover.split_at(split_at);

                                    match valid_bytes.into_text() {
                                        Ok(text) => {
                                            for line in text {
                                                // re-check before pushing
                                                if version.load(Ordering::Acquire) != _version {
                                                    return true;
                                                }
                                                lines.push(line);
                                            }
                                        }
                                        Err(e) => {
                                            if self.config.try_lossy {
                                                for bytes in valid_bytes.split(|b| *b == b'\n') {
                                                    if version.load(Ordering::Acquire) != _version {
                                                        return true;
                                                    }
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

                                if !leftover.is_empty()
                                    && version.load(Ordering::Acquire) == _version
                                {
                                    match leftover.into_text() {
                                        Ok(text) => {
                                            for line in text {
                                                if version.load(Ordering::Acquire) != _version {
                                                    return true;
                                                }
                                                lines.push(line);
                                            }
                                        }
                                        Err(e) => {
                                            if self.config.try_lossy {
                                                for bytes in leftover.split(|b| *b == b'\n') {
                                                    if version.load(Ordering::Acquire) != _version {
                                                        return true;
                                                    }
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
                _ => unreachable!(),
            }

            self.prune_procs();
        }

        let ret = self.cleanup_procs();
        if ret.is_empty() { Ok(()) } else { Err(ret) }
    }

    fn dispatch_kill(&mut self) {
        if let Some((mut child, old)) = self.current.take() {
            // invalidate all existing feeders
            self.version.fetch_add(1, Ordering::AcqRel);

            let _ = child.kill();
            self.procs.push(child);
            let mut old = Box::pin(old); // pin it to heap

            match old.as_mut().now_or_never() {
                Some(Ok(result)) => {
                    if !result {
                        self.send(Event::Refresh)
                    }
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
