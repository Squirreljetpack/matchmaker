use std::{
    ffi::{OsStr, OsString},
    process::{Command, ExitStatus, Stdio},
};

use crate::{
    action::DISCRIMINANT_SHOW_PREVIEW,
    config::PagerConfig,
    lua::{self, LuaState},
    script::{self, CommandStrategy},
};
use cba::{
    bait::ResultExt,
    bring::split::split_whitespace_preserve_single_quotes,
    broc::{CommandExt, EnvVars, SHELL, tty_or_inherit},
    env_vars, unwrap,
};
use log::{debug, info};
use matchmaker::{
    Action, AttachmentFormatter, Matchmaker, SSS,
    action::ActionExt,
    event::RenderSender,
    message::{Interrupt, RenderCommand},
    nucleo::Render,
    render::MMState,
    use_formatter,
};
use std::{
    env,
    fs::OpenOptions,
    io::{self, Write},
    process::Child,
    thread,
    time::{Duration, Instant},
};
use tokio::io::AsyncReadExt;

#[easy_ext::ext(MMExt)]
impl<T: SSS + Render, S, D: 'static> Matchmaker<T, S, D> {
    /// Causes [`Action::Execute`] to cause the program to execute the program specified by its payload.
    pub fn register_execute_handler(
        &mut self,
        formatter: AttachmentFormatter<T, D>,
        shell: Vec<OsString>,
        preview_shell: Vec<OsString>,
        pager: PagerConfig,
        help_factory: matchmaker::HelpFactory,
        help_config: matchmaker::config::HelpDisplayConfig,
    ) {
        let formatter_ = formatter.clone();
        let execute_shell = shell.clone();
        let silent_shell = shell;
        self.register_interrupt_handler(Interrupt::Execute, move |state| {
            let discriminant = state.discriminant_payload.take();
            let template = state.payload();

            // ShowPreview with a static preview payload pages the text directly
            // (like the PreviewSet handler's Set branch); no command runs and
            // no env is needed.
            if discriminant == Some(DISCRIMINANT_SHOW_PREVIEW)
                && let Some(Err(text)) = state.preview_set_payload()
            {
                let resolved = matchmaker::resolve_static_preview(&text, &help_factory, &help_config);
                let ansi_text = matchmaker::utils::text_to_ansi(&resolved);
                crate::pager::page_text(&ansi_text, &pager);
                return;
            }

            if template.is_empty() {
                return;
            }

            let Some(payload) = classify_payload(template, state, &formatter) else {
                return;
            };
            let mut vars = state.make_env_vars();

            let preview_template = if let Some(Ok(s)) = state.preview_set_payload() {
                s
            } else {
                state.preview_payload().clone()
            };
            let preview_cmd = use_formatter(&formatter, state, &preview_template, None);
            let extra = env_vars!(
                "MM_PREVIEW_COMMAND" => preview_cmd,
            );
            vars.extend(extra);

            // ShowPreview pages the preview command fullscreen instead of
            // executing the payload; the picker always resumes afterwards.
            if discriminant == Some(DISCRIMINANT_SHOW_PREVIEW) {
                let Some(mut child) = command_from_script(&preview_cmd, &preview_shell, &vars)
                    .and_then(|mut builder| {
                        builder
                            .envs(vars)
                            .stdin(Stdio::null())
                            .stdout(Stdio::piped())
                            .stderr(Stdio::inherit())
                            ._spawn()
                    })
                else {
                    log::error!("ShowPreview: failed to spawn preview command [{preview_cmd}]");
                    return;
                };
                let Some(stdout) = child.stdout.take() else {
                    log::error!("ShowPreview: preview command produced no stdout");
                    return;
                };
                // minus draws on stdout when it is a terminal, or on the
                // controlling tty (/dev/tty, via matchmaker-minus's configurable
                // output sink) when stdout is redirected; otherwise the external
                // pager chain displays on the controlling tty so a redirected
                // stdout is not polluted with pager UI.
                let use_minus = crate::pager::minus_available();
                if use_minus {
                    #[cfg(feature = "pager")]
                    return crate::pager::minus_page(stdout, child, &pager);
                }
                crate::pager::external_pager(stdout, child);
                return;
            }

            match payload {
                Payload::Command(cmd) => {
                    if let Some(mut cmd_builder) = command_from_script(&cmd, &execute_shell, &vars)
                        && let Some(mut child) =
                            cmd_builder.envs(vars).stdin(tty_or_inherit())._spawn()
                    {
                        match child.wait() {
                            Ok(i) => {
                                info!("Command [{cmd}] exited with {i}");
                                handle_exit(state, discriminant, ExitDetails::of(i));
                            }
                            Err(e) => {
                                info!("Failed to wait on command [{cmd}]: {e}")
                            }
                        }
                    }
                }
                Payload::LuaFile { path, args } => {
                    let Some(file) = script::resolve_at_path(&path, &vars) else {
                        return;
                    };
                    match lua::run_file(&file, args, &vars, &LuaState::from_mm(state)) {
                        Ok(code) => {
                            handle_exit(state, discriminant, ExitDetails::code(code));
                        }
                        Err(e) => {
                            log::error!("Lua script @{} failed: {e}", file.display());
                            handle_exit(state, discriminant, ExitDetails::error());
                        }
                    }
                }
                Payload::LuaInline(code) => {
                    match lua::run_inline(&code, &vars, &LuaState::from_mm(state)) {
                        Ok(code) => {
                            handle_exit(state, discriminant, ExitDetails::code(code));
                        }
                        Err(e) => {
                            log::error!("Lua command failed: {e}");
                            handle_exit(state, discriminant, ExitDetails::error());
                        }
                    }
                }
            }
        });

        self.register_interrupt_handler(Interrupt::ExecuteSilent, move |state| {
            let template = state.payload().clone();
            if !template.is_empty() && state.discriminant_payload.is_none() {
                let Some(payload) = classify_payload(&template, state, &formatter_) else {
                    return;
                };
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter_, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);

                match payload {
                    Payload::Command(cmd) => {
                        if let Some(mut cmd_builder) =
                            command_from_script(&cmd, &silent_shell, &vars)
                            && let Some(mut _child) =
                                cmd_builder.envs(vars).stdin(tty_or_inherit())._spawn()
                        {
                        }
                    }
                    Payload::LuaFile { path, args } => {
                        let Some(file) = script::resolve_at_path(&path, &vars) else {
                            return;
                        };
                        let lua_state = LuaState::from_mm(state);
                        std::thread::spawn(move || {
                            if let Err(e) = lua::run_file(&file, args, &vars, &lua_state) {
                                log::error!("Lua script @{} failed: {e}", file.display());
                            }
                        });
                    }
                    Payload::LuaInline(code) => {
                        let lua_state = LuaState::from_mm(state);
                        std::thread::spawn(move || {
                            if let Err(e) = lua::run_inline(&code, &vars, &lua_state) {
                                log::error!("Lua command failed: {e}");
                            }
                        });
                    }
                }
            };
        });
    }

    /// Causes [`Action::ExecuteAsync`] and [`Action::ExecuteThen`] to execute their payload without blocking, and for the remaining actions in the batch to depend on the execution result.
    pub fn register_execute_async_handler(
        &mut self,
        formatter: AttachmentFormatter<T, D>,
        shell: Vec<OsString>,
    ) {
        self.register_interrupt_handler(Interrupt::ExecuteAsync, move |state| {
            if state.discriminant_payload.as_ref().is_some_and(|p| *p >= 2)
                && let payload = state.discriminant_payload.take().unwrap()
                && let template = state.payload()
                && !template.is_empty()
            {
                let Some(kind) = classify_payload(template, state, &formatter) else {
                    return;
                };

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

                let shell = shell.clone();
                let lua_state = LuaState::from_mm(state);
                tokio::spawn(async move {
                    let success = match kind {
                        Payload::Command(cmd) => {
                            let Some(mut cmd_builder) =
                                tokio_command_from_script(&cmd, &shell, &vars)
                            else {
                                return; // skip: error already logged
                            };
                            let mut child = match cmd_builder
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
                                    s.success()
                                }
                                Err(e) => {
                                    log::warn!("Failed to wait on async command [{}]: {}", cmd, e);
                                    return;
                                }
                            }
                        }
                        Payload::LuaFile { path, args } => {
                            let Some(file) = script::resolve_at_path(&path, &vars) else {
                                return;
                            };
                            let file_log = file.to_string_lossy().into_owned();
                            let st = lua_state.clone();
                            match tokio::task::spawn_blocking(move || {
                                lua::run_file(&file, args, &vars, &st)
                            })
                            .await
                            {
                                Ok(Ok(code)) => code == 0,
                                Ok(Err(e)) => {
                                    log::error!("Lua script @{file_log} failed: {e}");
                                    false
                                }
                                Err(e) => {
                                    log::error!("Lua script @{file_log} panicked: {e}");
                                    false
                                }
                            }
                        }
                        Payload::LuaInline(code) => {
                            let st = lua_state.clone();
                            match tokio::task::spawn_blocking(move || {
                                lua::run_inline(&code, &vars, &st)
                            })
                            .await
                            {
                                Ok(Ok(code)) => code == 0,
                                Ok(Err(e)) => {
                                    log::error!("Lua command failed: {e}");
                                    false
                                }
                                Err(e) => {
                                    log::error!("Lua command panicked: {e}");
                                    false
                                }
                            }
                        }
                    };

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
    pub fn register_become_handler(
        &mut self,
        formatter: AttachmentFormatter<T, D>,
        shell: Vec<OsString>,
    ) {
        let formatter_2 = formatter.clone();
        let shell_2 = shell.clone();
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

                if let Some(mut cmd_builder) = command_from_script(&cmd, &shell, &vars) {
                    cmd_builder.envs(vars)._exec();
                }
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

                if let Some(mut cmd_builder) = command_from_script(&cmd, &shell_2, &vars) {
                    cmd_builder.envs(vars)._exec();
                }
            }
        });
    }

    /// Causes the Copy and CopyAsync actions to execute their payload, and copy the result to the clipboard.
    pub fn register_copy<A: ActionExt + Send + 'static>(
        &mut self,
        formatter: AttachmentFormatter<T, D>,
        copy_trailing_newline: bool,
        render_tx: Option<RenderSender<A>>,
        shell: Vec<OsString>,
    ) {
        let formatter_1 = formatter.clone();
        let render_tx_1 = render_tx.clone();
        let shell_2 = shell.clone();

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
                #[cfg_attr(not(feature = "mlua"), allow(unused_variables))]
                let lua_state = LuaState::from_mm(state);
                let render_tx = render_tx_1.clone();

                let shell = shell.clone();

                tokio::spawn(async move {
                    let clip_cmd = vars.get("CLIPcmd").map(|x| x.to_string());
                    let mut text = match script::classify(&cmd) {
                        #[cfg(feature = "mlua")]
                        CommandStrategy::Lua(code) => {
                            let vs = vars.clone();
                            let st = lua_state.clone();
                            match tokio::task::spawn_blocking(move || {
                                lua::run_inline_value(&code, &vs, &st)
                            })
                            .await
                            {
                                Ok(Ok(Some(v))) => v,
                                Ok(Ok(None)) => String::new(),
                                Ok(Err(e)) => {
                                    log::error!("Lua command failed: {e}");
                                    return;
                                }
                                Err(e) => {
                                    log::error!("Lua command panicked: {e}");
                                    return;
                                }
                            }
                        }
                        #[cfg(feature = "mlua")]
                        CommandStrategy::LuaFile { path, args: file_args } => {
                            let Some(file) = script::resolve_at_path(&path, &vars) else {
                                return;
                            };
                            let vs = vars.clone();
                            let st = lua_state.clone();
                            let file_log = file.to_string_lossy().into_owned();
                            match tokio::task::spawn_blocking(move || {
                                lua::run_file_value(&file, file_args, &vs, &st)
                            })
                            .await
                            {
                                Ok(Ok(Some(v))) => v,
                                Ok(Ok(None)) => String::new(),
                                Ok(Err(e)) => {
                                    log::error!("Lua script @{file_log} failed: {e}");
                                    return;
                                }
                                Err(e) => {
                                    log::error!("Lua script @{file_log} panicked: {e}");
                                    return;
                                }
                            }
                        }
                        _ => {
                            let mut child = match unwrap!(tokio_command_from_script(&cmd, &shell, &vars))
                                .envs(vars.clone())
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
                            let _ = child.wait().await;
                            text
                        }
                    };

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
                        } else if let Some(clip_cmd) = clip_cmd {
                            // discriminant 0: use CLIPcmd
                            if !clip_cmd.is_empty() {
                                let mut child = match unwrap!(tokio_command_from_script(
                                    &clip_cmd, &shell, &vars
                                ))
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
                .is_some_and(|p| *p == 1 || *p == 0)
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

                if let Some(contents) = Command::from_script(&cmd, &shell_2)
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
                        if payload == 1 {
                            if let Err(e) = set_host_clipboard_universal(&text) {
                                log::warn!("Failed to set host clipboard: {}", e);
                            }

                            if let Some(tx) = render_tx.as_ref() {
                                let _ = tx.send(RenderCommand::Action(Action::Redraw));
                            }
                        } else if let Some(clip_cmd) = clip_cmd {
                            // discriminant 2: use CLIPcmd
                            if !clip_cmd.is_empty() {
                                let Some(mut child) = Command::from_script(&clip_cmd, &[])
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
}

// ------------- HELPERS -----------------

/// A classified payload ready for execution.
#[cfg_attr(not(feature = "mlua"), allow(dead_code))]
enum Payload {
    /// Run through the shell / `@` direct-exec path.
    Command(String),
    /// `@file.lua …` — the file runs on the lua engine, always.
    LuaFile { path: OsString, args: Vec<OsString> },
    /// `#!lua …` — formatted inline lua source.
    LuaInline(String),
}

/// Classify a raw payload template for execution. `@` payloads skip
/// formatting (formatter quoting would mangle direct-exec payloads); inline
/// `#!lua` payloads and plain shell commands are formatted. Returns `None` to
/// skip execution (already logged).
fn classify_payload<T: SSS, D: 'static>(
    template: &str,
    state: &MMState<'_, T, D>,
    formatter: &AttachmentFormatter<T, D>,
) -> Option<Payload> {
    match script::classify(template) {
        #[cfg(feature = "mlua")]
        CommandStrategy::Lua(code) => {
            let code = use_formatter(formatter, state, &code, None);
            if code.is_empty() {
                None
            } else {
                Some(Payload::LuaInline(code))
            }
        }
        #[cfg(feature = "mlua")]
        CommandStrategy::LuaFile { path, args } => Some(Payload::LuaFile { path, args }),
        CommandStrategy::File => {
            if template.is_empty() {
                None
            } else {
                Some(Payload::Command(template.to_owned()))
            }
        }
        CommandStrategy::Shell => {
            let cmd = use_formatter(formatter, state, template, None);
            if cmd.is_empty() {
                None
            } else {
                Some(Payload::Command(cmd))
            }
        }
    }
}


/// Outcome of an executed command, normalized across shell children and lua
/// scripts for the shared exit → quit/prompt policy.
struct ExitDetails {
    /// Exit code when the command ran to completion.
    code: Option<i32>,
    /// User termination: signal (unix) or Ctrl+C (windows).
    interrupted: bool,
    /// Crash: stopped signal (unix) or negative exit code (windows).
    abnormal: bool,
}

impl ExitDetails {
    fn of(status: ExitStatus) -> Self {
        #[cfg(unix)]
        let interrupted = {
            use std::os::unix::process::ExitStatusExt;
            status.signal().is_some_and(|x| [2, 3, 15].contains(&x))
        };

        #[cfg(windows)]
        let interrupted = status.code().is_some_and(|x| x == -1073741510); // 0xC000013A (Ctrl+C)

        #[cfg(not(any(unix, windows)))]
        let interrupted = status.code().is_none();

        #[cfg(unix)]
        let abnormal = {
            use std::os::unix::process::ExitStatusExt;
            status.stopped_signal().is_some()
        };

        #[cfg(windows)]
        let abnormal = status.code().is_some_and(|x| x < 0);

        #[cfg(not(any(unix, windows)))]
        let abnormal = false;

        Self {
            code: status.code(),
            interrupted,
            abnormal,
        }
    }

    /// A lua script that ran to completion with `code`.
    fn code(code: i32) -> Self {
        Self {
            code: Some(code),
            interrupted: false,
            abnormal: false,
        }
    }

    /// A lua script error: a failure without a code.
    fn error() -> Self {
        Self {
            code: Some(1),
            interrupted: false,
            abnormal: false,
        }
    }
}

/// Apply the exit → quit/prompt policy shared by shell children and lua
/// scripts: `Some(0)` prompts on a non-zero exit, `Some(1)` quits on success,
/// `Some(2)` resumes on user termination, `Some(3)` quits on any abnormal exit.
fn handle_exit<T: SSS, D: 'static>(
    state: &mut MMState<'_, T, D>,
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
            } else if exit.code == Some(100) || exit.interrupted {
                // resume on _user_ termination signal
            } else {
                prompt_to_continue();
            }
        }
        Some(3) => {
            if exit.code == Some(0) {
                state.should_quit = true;
            }

            // quit on **any abnormal** exit
            if exit.code.is_none() || exit.abnormal {
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
/// Build a command from a script, mirroring `Command::from_script`, with
/// `@`-prefixed direct-execution support:
///
/// - `@path arg...` splits on whitespace (preserving single quotes), resolves
///   `path` (relative paths against the parent of `MM_OVERRIDE` from `envs`),
///   and runs it directly as the first argument of `shell[0]` — or
///   [`SHELL`].0 when `shell` is empty — with the remaining words as
///   arguments. No shell interpretation is performed.
/// - Returns `None` (and logs an error) to skip execution when `MM_OVERRIDE`
///   is unset for a relative `@` path, or the `@` command is empty.
fn command_from_script(script: &str, shell: &[OsString], envs: &EnvVars) -> Option<Command> {
    let Some(s) = script.strip_prefix('@') else {
        return Some(Command::from_script(script, shell));
    };

    let argv = at_argv(s, shell, envs)?;
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    Some(cmd)
}

/// [`tokio::process::Command`] counterpart of [`command_from_script`].
fn tokio_command_from_script(
    script: &str,
    shell: &[OsString],
    envs: &EnvVars,
) -> Option<tokio::process::Command> {
    let Some(s) = script.strip_prefix('@') else {
        return Some(tokio_from_script(script, shell));
    };

    let argv = at_argv(s, shell, envs)?;
    let mut cmd = tokio::process::Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    Some(cmd)
}

/// Mirror of `cba::broc::CommandExt::from_script` for [`tokio::process::Command`].
fn tokio_from_script(script: &str, shell: &[OsString]) -> tokio::process::Command {
    let (def_sh, def_arg) = &*SHELL;

    let mut ret = if shell.is_empty() {
        let mut ret = tokio::process::Command::new(def_sh);
        ret.arg(def_arg);
        ret
    } else {
        let mut ret = tokio::process::Command::new(&shell[0]);
        ret.args(&shell[1..]);
        ret
    };

    ret.arg(script);

    #[cfg(unix)]
    if shell.is_empty() {
        ret.arg("");
    }

    ret
}

/// Split an `@` command (without the prefix) into an argv:
/// `shell[0]` (or [`SHELL`].0 when `shell` is empty), then the resolved path,
/// then the remaining words. A relative path is resolved against the parent of
/// `MM_OVERRIDE`. `None` means execution should be skipped (already logged).
fn at_argv(s: &str, shell: &[OsString], envs: &EnvVars) -> Option<Vec<OsString>> {
    let mut words = split_whitespace_preserve_single_quotes(s).into_iter();
    let Some(path) = words.next() else {
        log::error!("Empty @ command");
        return None;
    };

    let path = script::resolve_at_path(OsStr::new(&path), envs)?;

    let mut argv = vec![
        shell
            .first()
            .cloned()
            .unwrap_or_else(|| OsString::from(SHELL.0.as_str())),
    ];
    argv.push(path.into_os_string());
    argv.extend(words.map(OsString::from));
    Some(argv)
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
    use super::*;

    fn argv_of(cmd: &Command) -> Vec<OsString> {
        std::iter::once(cmd.get_program().to_os_string())
            .chain(cmd.get_args().map(OsString::from))
            .collect()
    }

    fn envs_with_override() -> EnvVars {
        env_vars!(
            "MM_OVERRIDE" => "/home/u/presets/ai/pi_extensions.toml",
        )
    }

    #[test]
    fn at_relative_resolves_against_mm_override_parent() {
        let cmd = command_from_script(
            "@pi_extensions_set.py global --verbose",
            &[OsString::from("python"), OsString::from("-c")],
            &envs_with_override(),
        )
        .unwrap();
        assert_eq!(
            argv_of(&cmd),
            vec![
                OsString::from("python"),
                OsString::from("/home/u/presets/ai/pi_extensions_set.py"),
                OsString::from("global"),
                OsString::from("--verbose"),
            ]
        );
    }

    #[test]
    fn at_relative_skips_when_mm_override_missing() {
        assert!(command_from_script("@script.sh", &[], &EnvVars::default()).is_none());
    }

    #[test]
    fn at_absolute_uses_path_directly() {
        let cmd = command_from_script(
            "@/opt/bin/tool.sh --id 42",
            &[OsString::from("bash"), OsString::from("-c")],
            &envs_with_override(),
        )
        .unwrap();
        assert_eq!(
            argv_of(&cmd),
            vec![
                OsString::from("bash"),
                OsString::from("/opt/bin/tool.sh"),
                OsString::from("--id"),
                OsString::from("42"),
            ]
        );
    }

    #[test]
    fn at_empty_shell_uses_shell_default() {
        let cmd = command_from_script("@script.sh", &[], &envs_with_override()).unwrap();
        let argv = argv_of(&cmd);
        assert_eq!(argv[0], OsString::from(SHELL.0.as_str()));
        assert_eq!(argv[1], OsString::from("/home/u/presets/ai/script.sh"));
    }

    #[test]
    fn at_keeps_single_quoted_args() {
        let cmd = command_from_script(
            "@preview.py 'my ext'",
            &[OsString::from("python"), OsString::from("-c")],
            &envs_with_override(),
        )
        .unwrap();
        assert_eq!(
            argv_of(&cmd),
            vec![
                OsString::from("python"),
                OsString::from("/home/u/presets/ai/preview.py"),
                OsString::from("my ext"),
            ]
        );
    }

    #[test]
    fn at_empty_command_returns_none() {
        assert!(command_from_script("@", &[], &envs_with_override()).is_none());
    }

    #[test]
    fn plain_script_falls_back_to_from_script() {
        let cmd = command_from_script(
            "echo hi",
            &[OsString::from("bash"), OsString::from("-c")],
            &envs_with_override(),
        )
        .unwrap();
        assert_eq!(
            argv_of(&cmd),
            vec![
                OsString::from("bash"),
                OsString::from("-c"),
                OsString::from("echo hi"),
            ]
        );
    }

    #[test]
    fn tokio_variant_resolves_at_commands() {
        let cmd = tokio_command_from_script(
            "@stats.py --price",
            &[OsString::from("python"), OsString::from("-c")],
            &envs_with_override(),
        );
        assert!(cmd.is_some());

        assert!(
            tokio_command_from_script(
                "@stats.py",
                &[OsString::from("python")],
                &EnvVars::default(),
            )
            .is_none()
        );
    }

    #[cfg(feature = "mlua")]
    #[test]
    fn classify_detects_lua_file_and_inline() {
        let CommandStrategy::LuaFile { path, args } =
            script::classify("@clean.lua 'a b' c")
        else {
            panic!("expected lua file payload");
        };
        assert_eq!(path, OsString::from("clean.lua"));
        assert_eq!(args, vec![OsString::from("a b"), OsString::from("c")]);

        let CommandStrategy::Lua(code) = script::classify("#!lua print({q})") else {
            panic!("expected lua inline payload");
        };
        assert_eq!(code, "print({q})");
    }

    #[test]
    fn classify_detects_direct_exec_and_shell() {
        assert!(matches!(
            script::classify("@script.sh"),
            CommandStrategy::File
        ));
        assert!(matches!(
            script::classify("@script.py arg"),
            CommandStrategy::File
        ));
        assert!(matches!(script::classify("echo hi"), CommandStrategy::Shell));
        assert!(matches!(script::classify("@"), CommandStrategy::File));
    }

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
        let resolved_custom = matchmaker::resolve_static_preview(&custom, &help_factory, &help_config);
        assert_eq!(resolved_custom.to_string(), "Custom Static Text");
    }
}
