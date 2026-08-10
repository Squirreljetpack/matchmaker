use std::{
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
};

use cba::{
    broc::{CommandExt, EnvVars, SHELL, tty_or_inherit},
    bring::split::split_whitespace_preserve_single_quotes,
    env_vars,
};
use log::{debug, info};
use matchmaker::{
    AttachmentFormatter, Matchmaker, SSS, message::Interrupt, use_formatter,
};

#[easy_ext::ext(MMExt)]
impl<T: SSS, S, D: 'static> Matchmaker<T, S, D> {
    /// Causes [`Action::Execute`] to cause the program to execute the program specified by its payload.
    /// Note:
    /// - not intended for direct use.
    /// - Assumes preview and cmd formatter are the same.
    pub fn register_execute_handler(
        &mut self,
        formatter: AttachmentFormatter<T, D>,
        shell: Vec<OsString>,
    ) {
        let formatter_ = formatter.clone();
        let execute_shell = shell.clone();
        let silent_shell = shell;
        self.register_interrupt_handler(Interrupt::Execute, move |state| {
            let discriminant = state.discriminant_payload.take();
            let template = state.payload();

            if !template.is_empty() {
                let cmd = use_formatter(&formatter, state, template, None);
                if cmd.is_empty() {
                    return;
                }
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

                if let Some(mut cmd_builder) = command_from_script(&cmd, &execute_shell, &vars)
                    && let Some(mut child) = cmd_builder
                        .envs(vars)
                        .stdin(tty_or_inherit())
                        ._spawn()
                {
                    match child.wait() {
                        Ok(i) => {
                            info!("Command [{cmd}] exited with {i}");
                            match discriminant {
                                // signal termination don't prompt
                                Some(0) if i.code().is_some_and(|c| c != 0) => {
                                    println!("\nPress enter to continue...");
                                    let mut input = String::new();
                                    let _ = std::io::stdin().read_line(&mut input);
                                }
                                Some(1) if i.success() => {
                                    state.should_quit = true;
                                }
                                Some(2) => {
                                    #[cfg(unix)]
                                    let interrupted = {
                                        use std::os::unix::process::ExitStatusExt;
                                        i.signal().is_some_and(|x| [2, 3, 15].contains(&x))
                                    };

                                    #[cfg(windows)]
                                    let interrupted = i.code().is_some_and(|x| x == -1073741510); // 0xC000013A (Ctrl+C)

                                    #[cfg(not(any(unix, windows)))]
                                    let interrupted = i.code().is_none();

                                    if i.success() {
                                        state.should_quit = true;
                                    } else if i.code().is_some_and(|x| x == 100) || interrupted {
                                        // resume on _user_ termination signal
                                    } else {
                                        println!("\nPress enter to continue...");
                                        let mut input = String::new();
                                        let _ = std::io::stdin().read_line(&mut input);
                                    }
                                }
                                Some(3) => {
                                    if i.success() {
                                        state.should_quit = true;
                                    }

                                    // quit on **any abnormal** exit
                                    if i.code().is_none() {
                                        state.should_quit_nomatch = true;
                                    }

                                    #[cfg(unix)]
                                    {
                                        use std::os::unix::process::ExitStatusExt;
                                        if i.stopped_signal().is_some() {
                                            // better to propogate this signal but this is a standby for now
                                            state.should_quit_nomatch = true;
                                        }
                                    }

                                    #[cfg(windows)]
                                    {
                                        if let Some(code) = i.code() && code < 0 {
                                            log::error!("Child process suffered a system crash/abnormal exit: 0x{:X}", code);
                                            state.should_quit_nomatch = true;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            info!("Failed to wait on command [{cmd}]: {e}")
                        }
                    }
                }
            };
        });

        self.register_interrupt_handler(Interrupt::ExecuteSilent, move |state| {
            let template = state.payload().clone();
            if !template.is_empty() {
                let cmd = use_formatter(&formatter_, state, &template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter_, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);

                if let Some(mut cmd_builder) = command_from_script(&cmd, &silent_shell, &vars)
                    && let Some(mut _child) = cmd_builder
                        .envs(vars)
                        .stdin(tty_or_inherit())
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

                let shell = shell.clone();
                tokio::spawn(async move {
                    let Some(mut cmd_builder) = tokio_command_from_script(&cmd, &shell, &vars) else {
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
                            if (!require_success || s.success())
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

    /// Causes [`Action::Become`] to cause the program to become the program specified by its payload.
    /// Note:
    /// - not intended for direct use.
    /// - Assumes preview and cmd formatter are the same.
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

    let path = if Path::new(&path).is_absolute() {
        Path::new(&path).to_path_buf()
    } else {
        let Some(override_path) = envs.get("MM_OVERRIDE") else {
            log::error!("MM_OVERRIDE not set; skipping @ command: {path}");
            return None;
        };
        Path::new(override_path)
            .parent()
            .unwrap_or(Path::new(""))
            .join(path)
    };

    let mut argv = vec![shell
        .first()
        .cloned()
        .unwrap_or_else(|| OsString::from(SHELL.0.as_str()))];
    argv.push(path.into_os_string());
    argv.extend(words.map(OsString::from));
    Some(argv)
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

        assert!(tokio_command_from_script(
            "@stats.py",
            &[OsString::from("python")],
            &EnvVars::default(),
        )
        .is_none());
    }
}
