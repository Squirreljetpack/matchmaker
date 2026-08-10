use std::{
    collections::HashMap,
    env::set_current_dir,
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, bail};
use cba::{
    bait::ResultExt,
    broc::EnvVars,
    bo::{map_chunks, map_reader_lines, read_to_chunks},
    bog::BogOkExt,
    broc::CommandExt,
    ebog, ibog, wbog,
};
use matchmaker::{
    Matchmaker,
    action::{Action, Actions},
    binds::Trigger,
    config::{CommandSetting, EnvValue, MatcherConfig, StartConfig},
    config_mm::{ConfigPreprocessedData, OddEnds},
    nucleo::{new_snapshot, nucleo::Matcher},
    render::{MMState, State},
    ui::{DisplayUI, UI},
};
use matchmaker::nucleo::injector::Injector;
use tokio::sync::mpsc;

use crate::{
    action::MMAction,
    config::Config,
    formatter::format_cli,
    start::{COMMAND_ARGS, process_envs},
    utils::expand_tilde,
};

/// Implements `--list`: outputs what the populating command would have output,
/// without starting the matcher.
///
/// - Bare `--list` execs the command (replacing this process, so its
///   stdout/stderr and exit code are preserved).
/// - `--list=<ARG>` dispatches on the shape of `ARG`; see [`list_arg`].
pub fn list(config: Config) -> ! {
    let (command, envs, shell) = setup(&config).__ebog();
    Command::from_script(&command, &shell)
        .envs(&envs)
        .args(&*COMMAND_ARGS.lock().unwrap())
        ._exec()
}

/// `--list=<ARG>` modes, all operating on the items the populating command
/// would produce (index `N` is 0-based, i.e. the item Enter would accept first
/// with an empty query):
///
/// - `N@alias`: formats and runs the command actions bound to the semantic
///   alias — `Execute`/`ExecuteAsync`/`ExecuteThen`/`ExecuteSilent`,
///   `Become`/`BecomeSilent`, and the CLI's `ExecuteOrConfirm`/
///   `ExecuteAndQuit`/`BecomeOrConfirm`/`BecomeOrResume`. Nested aliases are
///   **not** followed and non-command actions are skipped; commands run in
///   order and stop at the first failure. (In a non-interactive context the
///   confirm/resume semantics are moot: the command is simply run.)
/// - `N-M`: formats preview layout `M`'s command with item `N` and runs it.
/// - `N:TEMPLATE`: formats `TEMPLATE` with item `N` and executes the result.
/// - `TEMPLATE`: formats `TEMPLATE` with the first item and prints it.
pub fn list_arg(config: Config, arg: &str) -> ! {
    let result = if let Some((n_str, alias_name)) = arg.split_once('@')
        && let Some(n) = parse_index(n_str)
    {
        alias(config, n, alias_name)
    } else if let Some((n_str, m_str)) = arg.split_once('-')
        && let Some(n) = parse_index(n_str)
    {
        preview(config, n, m_str)
    } else if let Some((n_str, template)) = arg.split_once(':')
        && let Some(n) = parse_index(n_str)
    {
        exec_template(config, n, template)
    } else {
        print_template(config, arg)
    };

    match result {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            ebog!("{e:#}");
            std::process::exit(1)
        }
    }
}

/// `--list=N@alias`: run the command actions bound to a semantic alias.
///
/// The alias's action array is used as-is: nested `@` aliases are not
/// followed, and only Execute/Become-style actions are run.
fn alias(config: Config, n: usize, alias: &str) -> anyhow::Result<()> {
    let alias = alias.trim();
    let trigger: Trigger = format!("@{alias}")
        .parse()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let actions: Actions<MMAction> = config
        .binds
        .get(&trigger)
        .with_context(|| format!("no bind found for @{alias}"))?
        .clone();

    let shell = config.start.shell.clone();
    let commands = with_item(config, n, |mm_state| {
        let vars = mm_state.make_env_vars();
        let mut commands = Vec::new();
        for action in &actions.0 {
            let Some(payload) = command_payload(action) else {
                log::debug!("--list: skipping non-command action: {action}");
                continue;
            };
            let cmd = format_cli(mm_state, payload, None);
            if !cmd.is_empty() {
                commands.push((cmd, vars.clone()));
            }
        }
        Ok(commands)
    })?;

    run_commands(&commands, &shell)
}

/// `--list=N-M`: run the M-th preview layout's command, formatted with item N.
fn preview(config: Config, n: usize, m_str: &str) -> anyhow::Result<()> {
    let m: usize = m_str.parse().context("invalid preview layout index")?;
    let command = config
        .render
        .preview
        .layout
        .get(m)
        .map(|p| p.command.clone())
        .with_context(|| format!("preview layout {m} is out of range"))?;

    // the TUI runs previews with the previewer's shell, mirror that
    let shell = config.previewer.shell.clone();
    let commands = with_item(config, n, |mm_state| {
        let cmd = format_cli(mm_state, &command, None);
        if cmd.is_empty() {
            log::debug!("--list: preview command is empty; nothing to execute");
            Ok(Vec::new())
        } else {
            let mut vars = mm_state.make_env_vars();
            // mirror the TUI: executed scripts can access the preview command
            vars.set("MM_PREVIEW_COMMAND", cmd.clone());
            Ok(vec![(cmd, vars)])
        }
    })?;

    run_commands(&commands, &shell)
}

/// `--list=N:TEMPLATE`: format `TEMPLATE` with item N and execute the result.
fn exec_template(config: Config, n: usize, template: &str) -> anyhow::Result<()> {
    if template.is_empty() {
        bail!("--list: empty template");
    }

    let shell = config.start.shell.clone();
    let commands = with_item(config, n, |mm_state| {
        let cmd = format_cli(mm_state, template, None);
        if cmd.is_empty() {
            // mirrors the TUI execute handler: an empty result means no action
            log::debug!("--list: formatted command is empty; nothing to execute");
            Ok(Vec::new())
        } else {
            Ok(vec![(cmd, mm_state.make_env_vars())])
        }
    })?;

    run_commands(&commands, &shell)
}

/// `--list=TEMPLATE`: format `TEMPLATE` with the first item and print it.
fn print_template(config: Config, template: &str) -> anyhow::Result<()> {
    with_item(config, 0, |mm_state| {
        let cmd = format_cli(mm_state, template, None);
        print!("{cmd}");
        Ok(())
    })
}

/// Runs the formatted commands in order, stopping at the first failure. A
/// single command is exec'd (preserving stdout, stderr and exit code); the
/// exit status of the last run command is used otherwise.
fn run_commands(commands: &[(String, EnvVars)], shell: &[OsString]) -> anyhow::Result<()> {
    if commands.is_empty() {
        return Ok(());
    }

    if commands.len() == 1 {
        let (cmd, vars) = &commands[0];
        ibog!("executing: {cmd}");
        Command::from_script(cmd, shell).envs(vars.clone())._exec()
    }

    for (cmd, vars) in commands {
        ibog!("executing: {cmd}");
        let status = Command::from_script(cmd, shell)
            .envs(vars.clone())
            .spawn()
            .with_context(|| format!("failed to spawn: {cmd}"))?
            .wait()
            .with_context(|| format!("failed to wait for: {cmd}"))?;
        if let Some(code) = status.code() {
            if code != 0 {
                std::process::exit(code)
            }
        } else {
            // terminated by a signal
            std::process::exit(1)
        }
    }

    Ok(())
}

/// Extracts the script payload of Execute/Become-style actions. Other actions
/// (navigation, events, nested semantic aliases, ...) return `None`.
fn command_payload(action: &Action<MMAction>) -> Option<&str> {
    use Action::*;
    Some(match action {
        Execute(s)
        | ExecuteAsync(s)
        | ExecuteThen(s)
        | ExecuteSilent(s)
        | Become(s)
        | BecomeSilent(s) => s,
        Custom(MMAction::ExecuteOrConfirm(s))
        | Custom(MMAction::ExecuteAndQuit(s))
        | Custom(MMAction::BecomeOrConfirm(s))
        | Custom(MMAction::BecomeOrResume(s)) => s,
        _ => return None,
    })
}

/// Parses a 0-based item (or preview layout) index. Rejects empty and
/// non-numeric prefixes so that plain templates may contain `@`, `-` or `:`.
fn parse_index(s: &str) -> Option<usize> {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
        s.parse().ok()
    } else {
        None
    }
}

/// Reads the populating command's output into an offline matcher state with the
/// cursor on item `n` (0-based), then applies `f` to the formatted state.
fn with_item<T>(
    config: Config,
    n: usize,
    f: impl FnOnce(&MMState<'_, '_, String, ConfigPreprocessedData>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let (command, envs, shell) = setup(&config)?;

    let Config {
        render,
        tui,
        matcher: MatcherConfig { matcher, worker },
        columns,
        start:
            StartConfig {
                input_separator,
                command: CommandSetting { separator, .. },
                preprocess,
                skip_invalid_lines,
                ..
            },
        exit,
        ..
    } = config;

    // -------- read the command's output into the worker ------------
    let (mut mm, injector, OddEnds { hidden_columns, .. }) =
        Matchmaker::new_from_config(render, tui, worker, columns, exit, preprocess);

    // stdout is captured (it provides the items); stderr is inherited so it
    // stays visible.
    let mut child = Command::from_script(&command, &shell)
        .envs(&envs)
        .args(&*COMMAND_ARGS.lock().unwrap())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("Failed to spawn command: {command}"))?;

    let stdout = child
        .stdout
        .take()
        .context("Failed to capture command stdout")?;

    let push = |line: String| injector.push(line);
    let total = if let Some(delim) = separator.or(input_separator) {
        map_chunks(read_to_chunks(stdout, delim), push, skip_invalid_lines)
    } else {
        map_reader_lines(stdout, push, skip_invalid_lines)
    }
    .context("Failed to read command output")?;

    // No more pushes: let the worker index everything we queued. Dropping the
    // injector first ensures the snapshot stops reporting a running state.
    drop(injector);
    let status = loop {
        let (_, s) = new_snapshot(&mut mm.worker.nucleo);
        if s.item_count as usize == total && !s.running {
            break s;
        }
    };
    let item_count = status.item_count;
    let matched_count = status.matched_count;

    // -------- build an offline state and move the cursor to item n ------------
    let mut matcher = Matcher::new(matcher.0);
    let (mut ui, mut picker_ui) =
        UI::new_offline(mm.render_config, &mut matcher, mm.worker, hidden_columns);
    let mut footer_ui = DisplayUI::default();
    let mut preview_ui = None;
    picker_ui.results.status = status;

    if n as u32 >= matched_count {
        bail!(
            "--list: item {n} is out of range (command produced {item_count} item{})",
            if item_count == 1 { "" } else { "s" },
        );
    }
    picker_ui.results.cursor_jump(n as u32);

    let mut state_obj = State::new();
    state_obj.envs.extend(envs);
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let mm_state = state_obj.dispatcher(
        &mut ui,
        &mut picker_ui,
        &mut footer_ui,
        &mut preview_ui,
        &event_tx,
    );

    if mm_state.current_raw().is_none() {
        bail!(
            "--list: item {n} is out of range (command produced {item_count} item{})",
            if item_count == 1 { "" } else { "s" },
        );
    }

    f(&mm_state)
}

/// Resolves the command and its envs, mirroring start.rs: `additional_commands`
/// and `_MM_INDEX` selection, `MM_INDEX` env, `process_envs`, and the
/// `directory` EnvValue handling (exec script or tilde-expanded path).
fn setup(config: &Config) -> anyhow::Result<(String, HashMap<String, String>, Vec<OsString>)> {
    let start = &config.start;
    let command = start.command.command.clone();
    let mut additional_commands = start.additional_commands.clone();

    if let Some(first) = additional_commands.first_mut()
        && first.is_empty()
    {
        *first = command.clone();
    }

    let mut initial_index = 0;
    if additional_commands.len() > 1
        && let Ok(index_str) = std::env::var("_MM_INDEX")
        && let Ok(index) = index_str.parse::<usize>()
        && index < additional_commands.len()
    {
        initial_index = index;
    }

    let command = if initial_index > 0 {
        additional_commands[initial_index].clone()
    } else {
        command
    };

    if command.is_empty() {
        bail!("--list requires a command: config.start.command is not set");
    }

    let mut envs = config.envs.clone();
    if !additional_commands.is_empty() {
        envs.insert(
            "MM_INDEX".to_string(),
            EnvValue::new(initial_index.to_string()),
        );
    }
    let envs = process_envs(envs);

    if !start.directory.value.is_empty() {
        let EnvValue { value, force, exec } = &start.directory;

        let mut failed = false;
        if *exec {
            if let Some(new_d) = Command::from_script(value, &[])
                .envs(&envs)
                .read_to_string()
                ._elog()
            {
                ibog!("directory script output: {}", new_d.trim());
                let new_d = Path::new(new_d.trim()).to_path_buf();
                if new_d.exists() {
                    failed = match set_current_dir(&new_d)
                        .prefix(format!("Failed to switch to {new_d:?}"))
                    {
                        Err(e) => {
                            if *force {
                                ebog!("{e}")
                            } else {
                                wbog!("{e}")
                            }
                            true
                        }
                        _ => false,
                    };
                } else {
                    ebog!("Directory does not exist: {}", new_d.display());
                    failed = true;
                }
            } else {
                ebog!("Failed to execute script for directory: {}", value);
                failed = true;
            }
        } else {
            let path = expand_tilde(value.clone().into());
            set_current_dir(&path)
                .prefix(format!("Failed to switch to {path:?}"))
                ._wbog();
        }

        if failed && *force {
            bail!("failed to switch directory");
        }
    }

    Ok((command, envs, config.start.shell.clone()))
}
