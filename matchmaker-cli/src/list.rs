use std::{
    collections::HashMap,
    env::set_current_dir,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, bail};
use cba::{
    bait::ResultExt,
    bo::{map_chunks, map_reader_lines, read_to_chunks},
    bog::BogOkExt,
    broc::CommandExt,
    ebog, ibog, wbog,
};
use matchmaker::{
    Matchmaker,
    config::{CommandSetting, EnvValue, MatcherConfig, StartConfig},
    config_mm::OddEnds,
    nucleo::{new_snapshot, nucleo::Matcher},
    render::State,
    ui::{DisplayUI, UI},
};
use matchmaker::nucleo::injector::Injector;
use tokio::sync::mpsc;

use crate::{
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
/// - `--list=<N:TEMPLATE>` formats TEMPLATE with the N-th item (0-based;
///   defaults to the first item when `N:` is omitted) and executes the result.
pub fn list(config: Config) -> ! {
    let (command, envs) = setup(&config).__ebog();
    Command::from_script(&command)
        .envs(&envs)
        .args(&*COMMAND_ARGS.lock().unwrap())
        ._exec()
}

/// `--list=<N:TEMPLATE>`: formats TEMPLATE with the N-th item (0-based) and
/// executes the result. Never starts the matcher.
pub fn template(config: Config, list_arg: &str) -> ! {
    template_inner(config, list_arg).__ebog();
    unreachable!("--list template execs or exits, so this is unreachable")
}

fn template_inner(config: Config, list_arg: &str) -> anyhow::Result<()> {
    let (command, envs) = setup(&config)?;

    // "N:" selects the N-th item (0-based); without it, the first item is used.
    let (n, template) = parse_list_arg(list_arg)?;
    if template.is_empty() {
        bail!("--list: empty template");
    }

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
    let mut child = Command::from_script(&command)
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

    // -------- build an offline state and format the template ------------
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

    let cmd = format_cli(&mm_state, template, None);
    if cmd.is_empty() {
        // mirrors the TUI execute handler: an empty result means no action
        log::debug!("--list: formatted command is empty; nothing to execute");
        std::process::exit(0)
    }

    ibog!("executing: {cmd}");
    // Exec replaces this process, preserving stdout and stderr.
    Command::from_script(&cmd)
        .envs(mm_state.make_env_vars())
        ._exec()
}

/// Resolves the command and its envs, mirroring start.rs: `additional_commands`
/// and `_MM_INDEX` selection, `MM_INDEX` env, `process_envs`, and the
/// `directory` EnvValue handling (exec script or tilde-expanded path).
fn setup(config: &Config) -> anyhow::Result<(String, HashMap<String, String>)> {
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
            if let Some(new_d) = Command::from_script(value)
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

    Ok((command, envs))
}

/// Parses `N:TEMPLATE` into a 0-based item index and the template. Without a
/// leading `N:`, the first item (index 0) is used.
fn parse_list_arg(list_arg: &str) -> anyhow::Result<(usize, &str)> {
    if let Some((n_str, template)) = list_arg.split_once(':')
        && !n_str.is_empty()
        && n_str.bytes().all(|b| b.is_ascii_digit())
    {
        Ok((
            n_str.parse().context("invalid --list item index")?,
            template,
        ))
    } else {
        Ok((0, list_arg))
    }
}
