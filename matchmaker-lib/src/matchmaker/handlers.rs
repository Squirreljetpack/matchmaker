use std::{
    env,
    fs::OpenOptions,
    io::{self, Write},
    process::{Command, Stdio},
    sync::Arc,
};

use cba::{_info, bait::ResultExt, broc::CommandExt, define_either, env_vars};
use log::{debug, info, warn};
use ratatui::text::Text;
use tokio::io::AsyncReadExt;

use crate::{
    Matchmaker, RenderFn, SSS,
    action::{Action, ActionExt},
    config::PreviewerConfig,
    event::RenderSender,
    message::{Event, Interrupt, RenderCommand},
    preview::{
        AppendOnly,
        previewer::{PreviewMessage, Previewer},
    },
    render::MMState,
    utils::{
        text::is_empty,
        tokio::{tokio_command_from_script, wait_with_timeout},
    },
};

define_either! {
    #[derive(serde::Serialize, serde::Deserialize)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Either<L, R = L> {
        Left,
        Right
    }
}

pub type AttachmentFormatter<T, D> = Either<
    Arc<RenderFn<T>>,
    for<'a, 'b, 'c> fn(&'a MMState<'b, 'c, T, D>, &'a str, Option<&dyn Fn(String)>) -> String,
>;

// we could check if template is empty here to avoid allocating but feels like it might be a footgun
pub fn use_formatter<T: SSS, D: 'static>(
    formatter: &AttachmentFormatter<T, D>,
    state: &MMState<'_, '_, T, D>,
    template: &str,
    repeat: Option<&dyn Fn(String)>,
) -> String {
    match formatter {
        Either::Left(f) => {
            if let Some(t) = state.current_raw() {
                f(t, template)
            } else {
                String::new()
            }
        }
        Either::Right(f) => f(state, template, repeat),
    }
}

// todo: this static bound shouldn't be necessary on S i don't know why its needed

/// A set of methods for registering the "standard" functionality for various interrupts/events.
/// These methods are prefixed with _ to indicate that library users will often prefer to override them.
impl<T: SSS, S, D: 'static> Matchmaker<T, S, D> {
    // technically we don't need concurrency but the cost should be negligable
    /// Causes [`Action::Print`] to print to stdout.
    pub fn _register_print_handler(
        &mut self,
        print_handle: AppendOnly<String>,
        output_separator: String,
        formatter: AttachmentFormatter<T, D>,
    ) {
        self.register_interrupt_handler(Interrupt::Print, move |state| {
            let template = state.payload().clone();
            let repeat = |s: String| {
                if atty::is(atty::Stream::Stdout) {
                    print_handle.push(s);
                } else {
                    print!("{}{}", s, output_separator);
                }
            };
            let s = use_formatter(&formatter, state, &template, Some(&repeat));
            if !s.is_empty() {
                repeat(s)
            }
        });
    }

    /// Causes [`Action::Execute`] to cause the program to execute the program specified by its payload.
    /// Note:
    /// - not intended for direct use.
    /// - Assumes preview and cmd formatter are the same.
    pub fn _register_execute_handler(&mut self, formatter: AttachmentFormatter<T, D>) {
        let formatter_1 = formatter.clone();
        self.register_interrupt_handler(Interrupt::Execute, move |state| {
            let template = state.payload();

            if !template.is_empty() {
                let cmd = use_formatter(&formatter_1, state, template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter_1, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);

                if let Some(mut child) = Command::from_script(&cmd)
                    .envs(vars)
                    .stdin(maybe_tty())
                    ._spawn()
                {
                    match child.wait() {
                        Ok(i) => {
                            info!("Command [{cmd}] exited with {i}");
                        }
                        Err(e) => {
                            info!("Failed to wait on command [{cmd}]: {e}")
                        }
                    }
                }
            };
        });

        let formatter_2 = formatter.clone();
        self.register_interrupt_handler(Interrupt::ExecuteSilent, move |state| {
            let template = state.payload();
            if !template.is_empty() {
                let cmd = use_formatter(&formatter_2, state, template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter_2, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);

                if let Some(mut _child) = Command::from_script(&cmd)
                    .envs(vars)
                    .stdin(maybe_tty())
                    ._spawn()
                {
                    // match child.wait() {
                    //     Ok(i) => {
                    //         info!("Command [{cmd}] exited with {i}")
                    //     }
                    //     Err(e) => {
                    //         info!("Failed to wait on command [{cmd}]: {e}")
                    //     }
                    // }
                }
            };
        });
    }

    /// Causes [`Action::ExecuteAsync`] and [`Action::ExecuteThen`] to execute their payload without blocking, and for the remaining actions in the batch to depend on the execution result.
    pub fn _register_execute_async_handler(&mut self, formatter: AttachmentFormatter<T, D>) {
        self.register_interrupt_handler(Interrupt::ExecuteAsync, move |state| {
            if state.discriminant_payload.as_ref().is_some_and(|p| *p >= 2)
                && let payload = state.discriminant_payload.take().unwrap()
                && let template = state.payload()
                && !template.is_empty()
            {
                let cmd = use_formatter(&formatter, state, template, None);
                if cmd.is_empty() {
                    return;
                }

                let id = payload / 2;
                let require_success = (payload % 2) == 1;

                let closure_opt = state.take_actions(id);

                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);

                tokio::spawn(async move {
                    let mut child = match tokio_command_from_script(&cmd)
                        .envs(vars)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(c) => c,
                        Err(e) => {
                            log::warn!("Failed to spawn async command [{}]: {}", cmd, e);
                            return;
                        }
                    };

                    match child.wait().await {
                        Ok(s) => {
                            info!("Async command [{}] exited with {}", cmd, s);
                            if (require_success || s.success())
                                && let Some(closure) = closure_opt
                            {
                                closure();
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to wait on async command [{}]: {}", cmd, e);
                        }
                    }
                });
            }
        });
    }

    /// Causes [`Action::Copy`] and [`Action::CopySync`] to execute their payload, and copy the result to the clipboard.
    /// Note:
    /// - intended for direct use
    pub fn register_copy<A: ActionExt + Send + 'static>(
        &mut self,
        formatter: AttachmentFormatter<T, D>,
        copy_trailing_newline: bool,
        render_tx: Option<RenderSender<A>>,
    ) {
        let formatter_1 = formatter.clone();
        let render_tx_1 = render_tx.clone();
        self.register_interrupt_handler(Interrupt::ExecuteAsync, move |state| {
            if state.discriminant_payload.as_ref().is_some_and(|p| *p <= 1)
                && let payload = state.discriminant_payload.take().unwrap()
                && let template = state.payload()
                && !template.is_empty()
            {
                let cmd = use_formatter(&formatter_1, state, template, None);
                if cmd.is_empty() {
                    return;
                }

                let vars = state.make_env_vars();
                let render_tx = render_tx_1.clone();

                tokio::spawn(async move {
                    let clip_cmd = vars.get("CLIPcmd").map(|x| x.to_string());
                    let mut child = match tokio_command_from_script(&cmd)
                        .envs(vars)
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(c) => c,
                        Err(e) => {
                            log::warn!("Failed to spawn copy command [{}]: {}", cmd, e);
                            return;
                        }
                    };

                    let mut text = String::new();
                    if let Some(mut stdout) = child.stdout.take() {
                        let _ = stdout.read_to_string(&mut text).await;
                    }

                    if !copy_trailing_newline && text.ends_with('\n') {
                        text.pop();

                        if text.ends_with('\r') {
                            text.pop();
                        }
                    }

                    let _ = child.wait().await;

                    if !text.is_empty() {
                        if payload == 1 {
                            if let Err(e) = set_host_clipboard_universal(&text) {
                                log::warn!("Failed to set host clipboard: {}", e);
                            }

                            if let Some(tx) = render_tx {
                                let _ = tx.send(RenderCommand::Action(Action::Redraw));
                            }
                        } else if let Some(clip_cmd) = clip_cmd {
                            // discriminant 0: use CLIPcmd
                            if !clip_cmd.is_empty() {
                                let mut child = match tokio_command_from_script(&clip_cmd)
                                    .stdin(Stdio::piped())
                                    .spawn()
                                {
                                    Ok(c) => c,
                                    Err(e) => {
                                        log::warn!("Failed to spawn CLIPcmd [{}]: {}", clip_cmd, e);
                                        return;
                                    }
                                };

                                if let Some(mut stdin) = child.stdin.take() {
                                    use tokio::io::AsyncWriteExt;
                                    let _ = stdin.write_all(text.as_bytes()).await;
                                    let _ = stdin.flush().await;
                                }
                                let _ = child.wait().await;
                            }
                        }
                    }
                });
            }
        });

        self.register_interrupt_handler(Interrupt::ExecuteSilent, move |state| {
            if state
                .discriminant_payload
                .as_ref()
                .is_some_and(|p| *p == 2 || *p == 3)
                && let payload = state.discriminant_payload.take().unwrap()
                && let template = state.payload()
                && !template.is_empty()
            {
                let cmd = use_formatter(&formatter, state, template, None);
                if cmd.is_empty() {
                    return;
                }

                let vars = state.make_env_vars();
                let clip_cmd = vars.get("CLIPcmd").map(|x| x.to_string());

                if let Some(contents) = Command::from_script(&cmd)
                    .envs(vars)
                    .read_to_string()
                    ._elog()
                {
                    let mut text = contents;

                    if !copy_trailing_newline && text.ends_with('\n') {
                        text.pop();

                        if text.ends_with('\r') {
                            text.pop();
                        }
                    }

                    if !text.is_empty() {
                        if payload == 3 {
                            if let Err(e) = set_host_clipboard_universal(&text) {
                                log::warn!("Failed to set host clipboard: {}", e);
                            }

                            if let Some(tx) = render_tx.as_ref() {
                                let _ = tx.send(RenderCommand::Action(Action::Redraw));
                            }
                        } else if let Some(clip_cmd) = clip_cmd {
                            // discriminant 2: use CLIPcmd
                            if !clip_cmd.is_empty() {
                                let Some(mut child) = Command::from_script(&clip_cmd)
                                    .stdin(Stdio::piped())
                                    ._spawn()
                                else {
                                    return;
                                };

                                if let Some(mut stdin) = child.stdin.take() {
                                    let _ = stdin.write_all(text.as_bytes());
                                    let _ = stdin.flush();
                                } else {
                                    log::error!("CLIPcmd had no stdin");
                                }

                                wait_with_timeout(child, std::time::Duration::from_millis(500));
                            }
                        }
                    }
                }
            }
        });
    }

    /// Causes [`Action::Become`] to cause the program to become the program specified by its payload.
    /// Note:
    /// - not intended for direct use.
    /// - Assumes preview and cmd formatter are the same.
    pub fn _register_become_handler(&mut self, formatter: AttachmentFormatter<T, D>) {
        let formatter_2 = formatter.clone();
        self.register_interrupt_handler(Interrupt::Become, move |state| {
            let template = state.payload().clone();
            if !template.is_empty() {
                let cmd = use_formatter(&formatter, state, &template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);
                debug!("Becoming: {cmd}");

                Command::from_script(&cmd).envs(vars)._exec()
            }
        });
        self.register_interrupt_handler(Interrupt::BecomeSilent, move |state| {
            let template = state.payload().clone();
            if !template.is_empty() {
                let cmd = use_formatter(&formatter_2, state, &template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter_2, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);
                debug!("Becoming: {cmd}");

                Command::from_script(&cmd).envs(vars)._exec()
            }
        });
    }
}

/// Causes the program to display a preview of the active result.
/// The Previewer can be connected to [`Matchmaker`] using [`PickOptions::previewer`]
pub fn make_previewer<T: SSS, S, D: 'static>(
    mm: &mut Matchmaker<T, S, D>,
    previewer_config: PreviewerConfig, // note: help_str is provided separately so help_colors is ignored
    formatter: AttachmentFormatter<T, D>,
    help_factory: Box<dyn Fn(&crate::config::HelpDisplayConfig) -> Text<'static> + Send + Sync>,
) -> Previewer {
    // initialize previewer
    let (previewer, tx) = Previewer::new(previewer_config.clone());
    let preview_tx = tx.clone();
    let formatter_clone = formatter.clone();

    let help_config = previewer_config.help.clone();

    // preview handler
    // important that PreviewSet events don't accidentally trigger this!
    mm.register_event_handler(Event::CursorChange | Event::PreviewChange | Event::Synced, move |state, _| {
            // don't clobber previewset events
            if state.contains(Event::PreviewSet) {
                // code logic-wise, recieve PreviewSet::None semantically => will recieve PreviewMessage::Unset => we should skip anyways (events is immutable), altho semantically such a state should actually trigger a new preview tho it would be niche
                return;
            }

            if state.preview_visible() &&
            let m = state.preview_payload().clone() &&
            let cmd = use_formatter(&formatter, state, &m, None) &&
            !cmd.is_empty()
            {
                let mut envs = state.make_env_vars();
                let extra = env_vars!(
                    "COLUMNS" => state.previewer_area().map_or("0".to_string(), |r| r.width.to_string()),
                    "LINES" => state.previewer_area().map_or("0".to_string(), |r| r.height.to_string()),
                );
                envs.extend(extra);

                let msg = PreviewMessage::Run(cmd.clone(), envs);
                if preview_tx.send(msg.clone()).is_err() {
                    warn!("Failed to send to preview: {}", msg)
                }

                // -----------------
                let target = state.preview_ui.as_ref().and_then(|p| p.config.initial.index.as_ref().and_then(|index_col| {
                    state.current_raw().and_then(|item| {
                        state.picker_ui.worker.format_with(item, index_col).and_then(|t| atoi::atoi(t.as_bytes()))
                    })
                }));

                _info!("previewui scroll target": target);

                if let Some(p) = state.preview_ui {
                    p.set_target(target);
                    p.jump = Default::default();
                };

            } else if preview_tx.send(PreviewMessage::Stop).is_err() {
                warn!("Failed to send to preview: stop")
            }

            state.preview_set_payload = None; // reset None here instead of on consume so that ::Help can toggle
        }
    );

    mm.register_event_handler(Event::PreviewSet, move |state, _event| {
        if state.preview_visible() {
            let payload = state.preview_set_payload();
            let msg = match payload {
                Some(Err(m)) => {
                    let m = if is_empty(&m) {
                        help_factory(&help_config)
                    } else {
                        m
                    };
                    PreviewMessage::Set(m)
                }
                None => PreviewMessage::Unset,
                Some(Ok(template)) => {
                    let cmd = use_formatter(&formatter_clone, state, &template, None);
                    if cmd.is_empty() {
                        PreviewMessage::Stop
                    } else {
                        let mut envs = state.make_env_vars();
                        let extra = env_vars!(
                            "COLUMNS" => state.previewer_area().map_or("0".to_string(), |r| r.width.to_string()),
                            "LINES" => state.previewer_area().map_or("0".to_string(), |r| r.height.to_string()),
                        );
                        envs.extend(extra);
                        PreviewMessage::Run(cmd, envs)
                    }
                }
            };

            if tx.send(msg.clone()).is_err() {
                warn!("Failed to send: {}", msg)
            }
        }
    });

    previewer
}

// ----------------------------

fn maybe_tty() -> Stdio {
    if let Ok(tty) = std::fs::File::open("/dev/tty") {
        // let _ = std::io::Write::flush(&mut tty); // does nothing but seems logical
        Stdio::from(tty)
    } else {
        log::error!("Failed to open /dev/tty");
        Stdio::inherit()
    }
}

pub fn set_host_clipboard_universal(text: &str) -> io::Result<()> {
    use base64::Engine;
    // 1. Encode the payload
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let sequence = format!("\x1b]52;c;{}\x07", encoded);

    // 2. Determine the direct TTY path
    // If we are over SSH, $SSH_TTY will be set to the exact device file.
    // Otherwise, we default to the current process's controlling terminal.
    let tty_path = env::var("SSH_TTY").unwrap_or_else(|_| "/dev/tty".to_string());

    // 3. Attempt to open the TTY file directly
    match OpenOptions::new().write(true).open(&tty_path) {
        Ok(mut tty_file) => {
            // Write directly to the TTY, completely bypassing standard output, Zellij, and tmux.
            write!(tty_file, "{}", sequence)?;
            tty_file.flush()?;
        }
        Err(_) => {
            // 4. Fallback if /dev/tty isn't available
            // If the direct TTY fails (e.g., on Windows), we fall back to standard output.
            // Here, we can still include the tmux check just in case.
            let fallback_sequence = if env::var("TMUX").is_ok() {
                format!("\x1bPtmux;\x1b\x1b]52;c;{}\x07\x1b\\", encoded)
            } else {
                sequence
            };

            let mut stdout = io::stdout();
            write!(stdout, "{}", fallback_sequence)?;
            stdout.flush()?;
        }
    }

    Ok(())
}
