use std::{
    ffi::OsString,
    process::{Command, Stdio},
};

use crate::{
    config::PagerConfig,
    execute::{self, ExitDetails},
    formatter::format_cli,
    pager,
};
use cba::{
    bait::ResultExt,
    broc::CommandExt,
};
use matchmaker::{
    Action, Matchmaker,
    action::ActionExt,
    config_mm::ConfigPreprocessedData,
    event::RenderSender,
    message::{Interrupt, RenderCommand},
    render::MMState,
};
use std::{
    env,
    fs::OpenOptions,
    io::{self, Write},
    process::Child,
    thread,
    time::{Duration, Instant},
};

#[easy_ext::ext(MMExt)]
impl<S> Matchmaker<String, S, ConfigPreprocessedData> {
    /// Causes [`MMAction::ShowPreview`] to page the preview command or static preview fullscreen.
    pub fn register_show_preview_handler(
        &mut self,
        preview_shell: Vec<OsString>,
        pager: PagerConfig,
        help_factory: matchmaker::HelpFactory,
        help_config: matchmaker::config::HelpDisplayConfig,
    ) {
        self.register_interrupt_handler(Interrupt::Execute, move |state| {
            if state.discriminant_payload != Some(4) {
                return;
            }
            state.discriminant_payload = None;
            state.clear_interrupt(); // clear the payload pre-empting subsequent execute handler

            // ShowPreview with a static preview payload pages the text directly
            if let Some(Err(text)) = state.preview_set_payload() {
                let resolved =
                    matchmaker::resolve_static_preview(&text, &help_factory, &help_config);
                let ansi_text = matchmaker::utils::text_to_ansi(&resolved);
                crate::pager::page_text(&ansi_text, &pager);
                return;
            }

            let vars = state.make_env_vars();
            let preview_template = if let Some(Ok(s)) = state.preview_set_payload() {
                s
            } else {
                state.preview_payload().clone()
            };
            let preview_cmd = format_cli(state, &preview_template, None);
            let Some((child, stdout)) = Command::from_script(&preview_cmd, &preview_shell)
                .envs(vars)
                .spawn_piped()
                ._elog()
            else {
                return;
            };

            if pager::minus_available() {
                #[cfg(feature = "pager")]
                pager::minus_page(stdout, child, &pager)
            } else {
                pager::external_pager(stdout, child)
            }
        });
    }

    /// Causes [`Action::Execute`] and [`Action::ExecuteSilent`] to execute the command specified by their payload.
    pub fn register_execute_handler(&mut self, shell: Vec<OsString>) {
        let execute_shell = shell.clone();
        let silent_shell = shell;

        // Foreground Execute handler
        self.register_interrupt_handler(Interrupt::Execute, move |state| {
            let discriminant = state.discriminant_payload.take();
            let template = state.payload();
            let vars = state.make_env_vars();

            let Some(strategy) = execute::classify(template)
                .template(state)
                .and_then(|s| s.resolve_relative(&vars)._elog())
            else {
                return;
            };
            #[cfg(feature = "mlua")]
            let lua_state = crate::lua::LuaState::from_mm(state);

            let Some(exit) = execute::run_execute(
                &strategy,
                &execute_shell,
                &vars,
                #[cfg(feature = "mlua")]
                &lua_state,
            ) else {
                return;
            };

            handle_execute_exit(state, discriminant, exit);
        });

        // run by spawning in background
        self.register_interrupt_handler(Interrupt::ExecuteSilent, move |state| {
            let template = state.payload().clone();
            if !template.is_empty() && state.discriminant_payload.is_none() {
                let vars = state.make_env_vars();
                let Some(strategy) = execute::classify(&template)
                    .template(state)
                    .and_then(|s| s.resolve_relative(&vars)._elog())
                else {
                    return;
                };
                #[cfg(feature = "mlua")]
                let lua_state = crate::lua::LuaState::from_mm(state);

                execute::run_execute_silent(
                    strategy,
                    silent_shell.clone(),
                    vars,
                    #[cfg(feature = "mlua")]
                    lua_state,
                );
            };
        });
    }

    /// Causes [`Action::ExecuteAsync`] and [`Action::ExecuteThen`] to execute their payload without blocking, and for the remaining actions in the batch to depend on the execution result.
    pub fn register_execute_async_handler(&mut self, shell: Vec<OsString>) {
        self.register_interrupt_handler(Interrupt::ExecuteAsync, move |state| {
            if state.discriminant_payload.as_ref().is_some_and(|p| *p >= 2)
                && let payload = state.discriminant_payload.take().unwrap()
                && let template = state.payload()
                && !template.is_empty()
            {
                let vars = state.make_env_vars();
                let Some(strategy) = execute::classify(template)
                    .template(state)
                    .and_then(|s| s.resolve_relative(&vars)._elog())
                else {
                    return;
                };
                let id = payload / 2;
                let require_success = (payload % 2) == 1;

                // setup
                let closure_opt = state.take_actions(id);
                let shell = shell.clone();

                #[cfg(feature = "mlua")]
                if strategy.is_lua() {
                    let lua_state = crate::lua::LuaState::from_mm(state);
                    tokio::task::spawn_blocking(move || {
                        let exit = execute::run_lua_status(&strategy, &vars, &lua_state);
                        let success = exit.map(|e| e.success()).unwrap_or(false);

                        if (!require_success || success)
                            && let Some(closure) = closure_opt
                        {
                            closure();
                        }
                    });
                    return;
                }
                let Some(mut cmd) = execute::build_command(&strategy, &shell) else {
                    return;
                };
                cmd.envs(vars)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                tokio::task::spawn_blocking(move || {
                    let exit = cmd.status()._elog().map(ExitDetails::of);
                    let success = exit.map(|e| e.success()).unwrap_or(false);

                    if (!require_success || success)
                        && let Some(closure) = closure_opt
                    {
                        closure();
                    }
                });
            }
        });
    }

    /// Causes [`Action::Become`] to cause the program to become the program specified by its payload.
    pub fn register_become_handler(&mut self, shell: Vec<OsString>) {
        let shell_2 = shell.clone();
        self.register_interrupt_handler(Interrupt::Become, move |state| {
            let template = state.payload().clone();
            if !template.is_empty() {
                let vars = state.make_env_vars();
                let Some(strategy) = execute::classify(&template)
                    .template(state)
                    .and_then(|s| s.resolve_relative(&vars)._elog())
                else {
                    return;
                };

                if let Some(mut cmd_builder) = execute::build_command(&strategy, &shell) {
                    cmd_builder.envs(vars)._exec();
                }
            }
        });
        self.register_interrupt_handler(Interrupt::BecomeSilent, move |state| {
            let template = state.payload().clone();
            if !template.is_empty() {
                let vars = state.make_env_vars();
                let Some(strategy) = execute::classify(&template)
                    .template(state)
                    .and_then(|s| s.resolve_relative(&vars)._elog())
                else {
                    return;
                };

                if let Some(mut cmd_builder) = execute::build_command(&strategy, &shell_2) {
                    cmd_builder.envs(vars)._exec();
                }
            }
        });
    }

    /// Causes the Copy and CopyAsync actions to execute their payload, and copy the result to the clipboard.
    pub fn register_copy<A: ActionExt + Send + 'static>(
        &mut self,
        copy_trailing_newline: bool,
        render_tx: Option<RenderSender<A>>,
        _shell: Vec<OsString>,
    ) {
        let render_tx_1 = render_tx.clone();

        self.register_interrupt_handler(Interrupt::ExecuteAsync, move |state| {
            if state.discriminant_payload.as_ref().is_some_and(|p| *p <= 1)
                && let payload = state.discriminant_payload.take().unwrap()
                && let template = state.payload()
                && !template.is_empty()
            {
                let vars = state.make_env_vars();
                let Some(strategy) = execute::classify(template)
                    .template(state)
                    .and_then(|s| s.resolve_relative(&vars)._elog())
                else {
                    return;
                };
                let clip_cmd = vars.get("CLIPcmd").map(|x| x.to_string());
                let render_tx = render_tx_1.clone();
                #[cfg(feature = "mlua")]
                if strategy.is_lua() {
                    let lua_state = crate::lua::LuaState::from_mm(state);
                    tokio::task::spawn_blocking(move || {
                        let text = execute::run_lua_capture(&strategy, &vars, &lua_state)
                            .unwrap_or_default();
                        deliver_clipboard(
                            text,
                            payload,
                            clip_cmd.as_deref(),
                            copy_trailing_newline,
                            render_tx.as_ref(),
                        );
                    });
                    return;
                }
                let Some(mut cmd) = execute::build_command(&strategy, &[]) else {
                    return;
                };
                cmd.envs(vars.as_strs());
                tokio::task::spawn_blocking(move || {
                    let text = execute::run_command_capture(cmd).unwrap_or_default();
                    deliver_clipboard(
                        text,
                        payload,
                        clip_cmd.as_deref(),
                        copy_trailing_newline,
                        render_tx.as_ref(),
                    );
                });
            }
        });

        self.register_interrupt_handler(Interrupt::ExecuteSilent, move |state| {
            if state.discriminant_payload.as_ref().is_some_and(|p| *p <= 1)
                && let payload = state.discriminant_payload.take().unwrap()
                && let template = state.payload()
                && !template.is_empty()
            {
                let vars = state.make_env_vars();
                let Some(strategy) = execute::classify(template)
                    .template(state)
                    .and_then(|s| s.resolve_relative(&vars)._elog())
                else {
                    return;
                };

                let clip_cmd = vars.get("CLIPcmd").map(|x| x.to_string());
                #[cfg(feature = "mlua")]
                let lua_state = crate::lua::LuaState::from_mm(state);
                let contents = execute::run_capture(
                    &strategy,
                    &vars,
                    #[cfg(feature = "mlua")]
                    &lua_state,
                );

                if let Some(contents) = contents {
                    deliver_clipboard(
                        contents,
                        payload,
                        clip_cmd.as_deref(),
                        copy_trailing_newline,
                        render_tx.as_ref(),
                    );
                }
            }
        });
    }
}

fn deliver_clipboard<A: ActionExt + Send + 'static>(
    mut text: String,
    payload: u8,
    clip_cmd: Option<&str>,
    copy_trailing_newline: bool,
    render_tx: Option<&RenderSender<A>>,
) {
    if !copy_trailing_newline && text.ends_with('\n') {
        text.pop();

        if text.ends_with('\r') {
            text.pop();
        }
    }

    if !text.is_empty() {
        if payload == 1 {
            if let Err(e) = set_host_clipboard_universal(&text) {
                log::warn!("Failed to set host clipboard: {}", e);
            }

            if let Some(tx) = render_tx {
                let _ = tx.send(RenderCommand::Action(Action::Redraw));
            }
        } else if let Some(clip_cmd) = clip_cmd
            && !clip_cmd.is_empty()
        {
            let Some(mut child) = Command::from_script(clip_cmd, &[])
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

/// Apply the exit → quit/prompt policy shared by shell children and lua
/// scripts: `Some(0)` prompts on a non-zero exit, `Some(1)` quits on success,
/// `Some(2)` resumes on user termination, `Some(3)` quits on any abnormal exit.
fn handle_execute_exit(
    state: &mut MMState<'_, String, ConfigPreprocessedData>,
    discriminant: Option<u8>,
    exit: ExitDetails,
) {
    match discriminant {
        // signal termination don't prompt
        Some(0) if exit.code.is_some_and(|c| c != 0) => {
            prompt_to_continue();
        }
        Some(1) if exit.code == Some(0) => {
            state.should_quit = true;
        }
        Some(2) => {
            if exit.code == Some(0) {
                state.should_quit = true;
            } else if exit.interrupted == Some(true) {
                // resume on user termination signal or exit 100
            } else {
                prompt_to_continue();
            }
        }
        Some(3) => {
            if exit.code == Some(0) {
                state.should_quit = true;
            }

            // quit on **any abnormal** exit
            if exit.code.is_none() || exit.interrupted == Some(false) {
                #[cfg(windows)]
                if let Some(code) = exit.code
                    && code < 0
                {
                    log::error!(
                        "Child process suffered a system crash/abnormal exit: 0x{:X}",
                        code
                    );
                }
                state.should_quit_nomatch = true;
            }
        }
        _ => {}
    }
}

fn prompt_to_continue() {
    println!("\nPress enter to continue...");
    let mut input = String::new();
    let _ = std::io::stdin().read_line(&mut input);
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

pub(crate) fn wait_with_timeout(mut child: Child, timeout: Duration) {
    let start = Instant::now();
    let handle = thread::spawn(move || {
        let _ = child.wait();
    });

    while start.elapsed() < timeout {
        if handle.is_finished() {
            return;
        }

        thread::sleep(Duration::from_millis(10));
    }

    log::warn!("CLIPcmd timed out");

    // there is a crate for this but for simplicity just forget about it
    // let _ = child.kill();
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_resolve_static_preview_delegates_to_help_factory() {
        let help_config = matchmaker::config::HelpDisplayConfig::default();
        let help_factory = |_: &matchmaker::config::HelpDisplayConfig| {
            matchmaker::Text::from("Resolved Help Content")
        };

        // Empty text delegates to help_factory
        let empty = matchmaker::Text::raw("");
        let resolved = matchmaker::resolve_static_preview(&empty, &help_factory, &help_config);
        assert_eq!(resolved.to_string(), "Resolved Help Content");

        // Non-empty static text is preserved
        let custom = matchmaker::Text::from("Custom Static Text");
        let resolved_custom =
            matchmaker::resolve_static_preview(&custom, &help_factory, &help_config);
        assert_eq!(resolved_custom.to_string(), "Custom Static Text");
    }
}
