use std::{
    collections::HashMap,
    env::set_current_dir,
    io::Read,
    path::Path,
    process::{Command, Stdio, exit},
    sync::Mutex,
};

use crate::{
    action::{ActionContext, MMAction, action_handler},
    clap::Cli,
    config::PartialConfig,
    formatter::format_cli,
    paths::{last_key_path, presets_path},
    utils::{expand_tilde, guess_clip_cmd, guess_editor_cmd, guess_pager_cmd},
};
use crate::{config::Config, paths::default_config_path};
use cba::{
    _wbog,
    bait::{OptionExt, ResultExt, TransformExt},
    bo::{
        MapReaderError, load_type_or_default, map_chunks, map_reader_lines, read_to_chunks,
        write_str,
    },
    bog::BogOkExt,
    ebog, ibog, prints, wbog,
};
use cba::{bo::load_type, broc::CommandExt};
use log::debug;
use matchmaker::{
    Action, ConfigInjector, MatchError, Matchmaker, OddEnds, PickOptions, SSS,
    binds::{BindMap, BindMapExt},
    config::{CommandSetting, EnvValue, MatcherConfig, StartConfig},
    event::{EventLoop, RenderSender},
    make_previewer,
    message::Interrupt,
    nucleo::injector::{Either, IndexedInjector, Injector},
    preview::AppendOnly,
    render::MMState,
    use_formatter,
};
use matchmaker_partial::Apply;

pub fn enter(cli: Cli, partial: Option<PartialConfig>) -> anyhow::Result<Config> {
    if cli.test_keys {
        super::crokey::main();
        exit(0);
    }

    let cfg_path = if let Some(p) = &cli.config {
        Path::new(p)
    } else {
        default_config_path()
    };

    if cli.dump_config && atty::is(atty::Stream::Stdout) {
        // if stdout: dump the default cfg with comments
        write_str(cfg_path, crate::config::DEFAULT_CONFIG)?;
        ibog!("Config written to {cfg_path:?}");
        exit(0)
    }

    #[cfg(debug_assertions)]
    if cli.config.is_none() {
        #[cfg(target_os = "windows")]
        write_str(cfg_path, include_str!("../assets/win.dev.toml")).unwrap();

        #[cfg(not(target_os = "windows"))]
        write_str(cfg_path, include_str!("../assets/dev.toml")).unwrap();
    }

    let mut config: Config = if cli.config.is_some() {
        load_type(cfg_path, |s| toml::from_str(s))._ebog().or_exit()
    } else {
        load_type_or_default(cfg_path, |s| toml::from_str(s))
    };
    // check config
    if config.source.is_some() {
        wbog!("'source' field is not supported in the main config.");
    }

    if config.render.status.template.is_empty() {
        config.render.status.template = r#"\m/\t"#.to_string();
    }

    #[cfg(not(debug_assertions))]
    log::trace!("Initial cfg: {config:?}");

    // apply overrides
    for mut p in cli.r#override {
        if p.is_relative() && p.extension().is_none() {
            let os = std::env::consts::OS;

            let main_p = presets_path().join(&p).join("main.toml");
            let main_os = presets_path().join(&p).join(format!("{}.main.toml", os));
            let exact = presets_path().join(p.with_extension("toml"));
            let mut os_name = std::ffi::OsString::from(format!("{}.", os));
            os_name.push(exact.file_name().unwrap_or_default());
            let exact_os = exact.with_file_name(os_name);

            p = if main_p.exists() {
                main_p
            } else if main_os.exists() {
                main_os
            } else if exact.exists() {
                exact
            } else if exact_os.exists() {
                exact_os
            } else {
                exact
            }
        }
        // no recursion because tail bad
        let o: PartialConfig = load_type(&p, |s| toml::from_str(s))?;

        config
            .envs
            .entry("MM_OVERRIDE".to_string())
            .or_insert_with(|| EnvValue::new(p.to_string_lossy().to_string()));

        if let Some(q) = &o.source {
            let source = p.parent().as_ref().unwrap().join(q);
            let o: PartialConfig = load_type(source, |s| toml::from_str(s))?;
            if o.source.is_some() {
                _wbog!("Ignoring 'source' field in nested override.");
            }
            config.apply(o);
        }

        config.apply(o);
    }

    #[cfg(debug_assertions)]
    {
        config.tui.clear_on_exit = false;
    }
    if let Some(partial) = partial {
        log::trace!("Applying cli overrides: {partial:?}");
        config.apply(partial); // resolve config.exit first
    }

    if !cli.args.is_empty() {
        if !atty::is(atty::Stream::Stdin) && !cli.no_read {
            eprintln!(
                "warning: trailing arguments provided but input is piped. ignoring trailing arguments."
            );
        }
        *COMMAND_ARGS.lock().unwrap() = cli.args;
    }

    // dispatch subcommands
    if cli.last_key {
        let path = config
            .exit
            .last_key_path
            .as_deref()
            .unwrap_or(last_key_path());

        let content = std::fs::read_to_string(path)._elog();
        if let Some(s) = content
            && let s = s.trim()
            && !s.is_empty()
        {
            prints!(s);
            exit(0);
        } else {
            exit(1)
        }
    }

    if cli.fullscreen {
        config.tui.layout = None;
    }

    if cli.dump_config {
        let contents = toml::to_string_pretty(&config).expect("failed to serialize to TOML");

        // if piped: dump the current cfg
        std::io::Write::write_all(&mut std::io::stdout(), contents.as_bytes())?;

        exit(0);
    }

    // check binds
    config.binds = BindMap::default_binds()
        .with_extras()
        .modify(|x| x.extend(config.binds));
    config.binds.check_cycles().map_err(anyhow::Error::msg)?;
    config.binds.retain(|_, actions| !actions.is_empty()); // enables disabling a bind via override
    // there is an additional step of resolve_semantics:

    for actions in config.binds.values() {
        for a in actions {
            if let Action::Custom(mm) = &a {
                mm.validate()?;
            }
        }
    }

    #[cfg(not(debug_assertions))]
    debug!("Config computed: {config:?}");

    Ok(config)
}

/// Spawns a tokio task mapping f to reader segments.
/// Read aborts on error. Read errors are logged.
pub fn map_reader<E: SSS + std::fmt::Display>(
    reader: impl Read + SSS,
    f: impl FnMut(String) -> Result<(), E> + SSS,
    input_separator: Option<char>,
    render_tx: RenderSender<MMAction>,
    abort_empty: bool,
    skip_invalid_lines: bool,
) -> tokio::task::JoinHandle<Result<usize, MapReaderError<E>>> {
    tokio::task::spawn_blocking(move || {
        let ret = if let Some(delim) = input_separator {
            map_chunks::<E>(read_to_chunks(reader, delim), f, skip_invalid_lines)
        } else {
            map_reader_lines::<E>(reader, f, skip_invalid_lines)
        }
        .elog();

        match &ret {
            Ok(0) => {
                if abort_empty {
                    let _ = render_tx.send(matchmaker::message::RenderCommand::NoMatch);
                }
            }
            Err(MapReaderError::ChunkError(_, _)) => {
                let _ = render_tx.send(matchmaker::message::RenderCommand::NoMatch);
            }
            _ => {}
        }

        log::trace!("All items pushed");
        ret
    })
}

pub static COMMAND_ARGS: Mutex<Vec<std::ffi::OsString>> = Mutex::new(Vec::new());

pub fn process_envs(mut envs: HashMap<String, EnvValue>) -> HashMap<String, String> {
    let mut processed_envs = HashMap::new();

    // todo: lowpri: should we provision this what is the cost of setting more env vars
    if envs.get("CLIPcmd").is_none() {
        if let Some(v) = std::env::var("CLIPcmd").ok()
            && !v.is_empty()
        {
            envs.insert("CLIPcmd".to_string(), EnvValue::new(v));
        } else {
            if let Some((clip, paste)) = guess_clip_cmd() {
                envs.insert("CLIPcmd".to_string(), EnvValue::new(clip));

                if envs.get("PASTEcmd").is_none()
                    && std::env::var("PASTEcmd").ok().is_none_or(|x| x.is_empty())
                {
                    envs.insert("PASTEcmd".to_string(), EnvValue::new(paste));
                }
            }
        }
    }

    if envs.get("PAGER").is_none() && std::env::var("PAGER").ok().is_none_or(|x| x.is_empty()) {
        let ev = EnvValue::new(guess_pager_cmd());
        envs.insert("PAGER".to_string(), ev);
    }

    if envs.get("EDITOR").is_none() && std::env::var("EDITOR").ok().is_none_or(|x| x.is_empty()) {
        let ev = EnvValue::new(guess_editor_cmd());
        envs.insert("PAGER".to_string(), ev);
    }

    // First pass: static envs
    for (k, v) in &envs {
        if !v.value.is_empty() && !v.exec && (v.force || std::env::var_os(k).is_none()) {
            processed_envs.insert(k.clone(), v.value.to_string());
        }
    }

    // Second pass: dynamic envs
    for (k, v) in &envs {
        if !v.value.is_empty() && v.exec && (v.force || std::env::var_os(k).is_none()) {
            if let Some(output) = Command::from_script(&v.value)
                .envs(&processed_envs)
                .read_to_string()
                ._elog()
            {
                processed_envs.insert(k.clone(), output.trim().to_string());
            } else {
                _wbog!("Failed to execute env command for {}: {}", k, v.value);
            }
        }
    }

    processed_envs
}

const START_ERROR: Result<(), MatchError> = Err(MatchError::Abort(11));

pub async fn start(config: Config, no_read: bool) -> Result<(), MatchError> {
    let Config {
        render,
        tui,
        previewer,
        matcher: MatcherConfig { matcher, worker },
        columns,
        binds,
        start:
            StartConfig {
                input_separator,
                command: CommandSetting { separator, command },
                directory,
                sync,
                output_separator,
                output_template,
                ansi,
                trim,
                mut additional_commands,
                mode,
                save_orphans,
                skip_invalid_lines,
                on_accept,
            },
        mut exit,
        mut envs,
        source: _,
    } = config;

    // -------- determine command ------------
    if let Some(first) = additional_commands.first_mut()
        && first.is_empty()
    {
        *first = command.clone();
    }
    let additional_commands = additional_commands;

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

    let initial_cmd = (!command.is_empty() && atty::is(atty::Stream::Stdin) || no_read)
        .then_some(command.clone())
        .unwrap_or_default();

    // -------- set envs/directory -----------
    if !additional_commands.is_empty() {
        envs.insert(
            "MM_INDEX".to_string(),
            EnvValue::new(initial_index.to_string()),
        );
    }
    let envs = process_envs(envs);

    if !directory.value.is_empty() {
        let EnvValue { value, force, exec } = directory;

        let mut failed = false;
        if exec {
            if let Some(new_d) = Command::from_script(&value)
                .envs(&envs)
                .read_to_string()
                ._elog()
            {
                let new_d = Path::new(new_d.trim()).to_path_buf();
                if new_d.exists() {
                    failed = set_current_dir(&new_d)
                        .prefix(format!("Failed to switch to {new_d:?}"))
                        ._wbog()
                        .is_some();
                } else {
                    ebog!("Directory does not exist: {}", new_d.display());
                    failed = true;
                }
            } else {
                ebog!("Failed to execute script for directory: {}", value);
                failed = true;
            }
        } else {
            let path = expand_tilde(value.into());
            set_current_dir(&path)
                .prefix(format!("Failed to switch to {path:?}"))
                ._wbog();
        }

        if failed && force {
            return START_ERROR;
        }
    }

    // ---------------------------------

    let abort_empty = exit.abort_empty;
    let header_lines = render.header.header_lines;
    let print_handle = AppendOnly::new();
    let output_separator = output_separator.clone().unwrap_or("\n".into());
    let preprocess = (ansi, trim);

    if exit.last_key_path.is_none() {
        exit.last_key_path = Some(last_key_path().into())
    }

    // set event loop mode
    let mode = if let Some(m) = mode {
        m
    } else {
        match (
            !initial_cmd.is_empty(), // has command => stdin is terminal
            atty::is(atty::Stream::Stdout),
        ) {
            (true, true) => "0,1", // both stdin and stdout are terminals
            (true, false) => "0",  // only stdin is a terminal
            (false, true) => "1",  // only stdout is a terminal
            (false, false) => "",  // neither is a terminal (piped)
        }
        .to_string()
    };
    matchmaker::event::set_mode(&mode);

    let event_loop = EventLoop::with_binds(binds).with_tick_rate(render.tick_rate());

    // make matcher and matchmaker with matchmaker-and-matcher-maker
    let copy_trailing_newline = tui.copy_trailing_newline;
    let (
        mut mm,
        injector,
        OddEnds {
            hidden_columns,
            has_error,
        },
    ) = Matchmaker::new_from_config(render, tui, worker, columns, exit, preprocess);

    if has_error {
        return START_ERROR;
    }

    // make previewer
    if !event_loop.original_binds().check_traces() {
        // maybe abort with error
    }
    let cli_formatter = Either::Right(
        crate::formatter::format_cli
            as for<'a, 'b, 'c> fn(
                &'a MMState<'b, 'c, matchmaker::ConfigMMItem, matchmaker::nucleo::ConfigPreprocessedData, String>,
                &'a str,
                Option<&dyn Fn(String)>,
            ) -> String,
    );
    let binds_ptr = event_loop.get_binds_ptr();
    let previewer = make_previewer(
        &mut mm,
        previewer,
        cli_formatter.clone(),
        Box::new(move |config| matchmaker::binds::display_help(&binds_ptr.load(), config)),
    );

    // ---------------------- build options ---------------------------

    let bind_tx = event_loop.bind_controller();

    let envs_ = envs.clone();
    let mut options = PickOptions::new()
        .event_loop(event_loop)
        .matcher(matcher.0)
        .previewer(previewer)
        .hidden_columns(hidden_columns)
        .initializer(move |s| {
            s.envs.extend(envs_);
        });

    let render_tx = options.render_tx();
    let push_fn = inject_line(header_lines, render_tx.clone(), injector);

    // ----------- read -----------------------
    let mut last_child = None;
    let handle = if !atty::is(atty::Stream::Stdin) && !no_read {
        let stdin = std::io::stdin();
        map_reader(
            stdin,
            push_fn,
            input_separator,
            render_tx.clone(),
            abort_empty,
            skip_invalid_lines,
        )
    } else if !command.is_empty()
        && let Some((child, stdout)) = Command::from_script(&command)
            .envs(envs)
            .args(&*COMMAND_ARGS.lock().unwrap())
            .spawn_piped()
            ._elog()
    {
        last_child = Some(child);
        map_reader(
            stdout,
            push_fn,
            separator.or(input_separator),
            render_tx.clone(),
            abort_empty,
            skip_invalid_lines,
        )
    } else {
        ebog!("no input detected.");
        return START_ERROR;
    };

    // ---------------------- register handlers ---------------------------
    // print handler (no quoting)
    mm._register_print_handler(
        print_handle.clone(),
        output_separator.clone(),
        cli_formatter.clone(),
    );

    // execute handlers
    mm._register_execute_handler(cli_formatter.clone());
    mm._register_execute_async_handler(cli_formatter.clone());
    mm.register_copy(
        cli_formatter.clone(),
        copy_trailing_newline,
        Some(render_tx.clone()),
    );
    mm._register_become_handler(cli_formatter.clone());

    // reload handler
    let reload_formatter = cli_formatter.clone();
    let reload_render_tx = render_tx.clone();
    let mut cmd = initial_cmd;
    mm.register_interrupt_handler(Interrupt::Reload, move |state| {
        let injector = state.injector();
        let injector = IndexedInjector::new_globally_indexed(injector);

        let push_fn = inject_line(
            state.picker_ui.header.config.header_lines,
            reload_render_tx.clone(),
            injector,
        );

        if !state.payload().is_empty() {
            cmd = use_formatter(&reload_formatter, state, state.payload(), None);
        };

        if !cmd.is_empty() {
            let vars = state.make_env_vars();
            debug!("Reloading: {cmd}");
            state.picker_ui.selector.clear();

            if let Some(mut child) = last_child.take()
                && !save_orphans
            {
                child.kill()._elog();
            }

            if let Some((child, stdout)) = Command::from_script(&cmd)
                .envs(vars)
                .stdin(Stdio::null())
                .args(&*COMMAND_ARGS.lock().unwrap())
                .spawn_piped()
                ._elog()
            {
                map_reader(
                    stdout,
                    push_fn,
                    separator.or(input_separator),
                    reload_render_tx.clone(),
                    abort_empty,
                    skip_invalid_lines,
                );
                last_child = Some(child);
            }
        }
    });

    // debug!("{mm:?}");

    let mut action_context = ActionContext {
        bind_tx,
        render_tx: render_tx.clone(),
        additional_commands: (additional_commands, initial_index),
        // output_template,
        // print_handle: print_handle.clone(),
        // output_separator: output_separator.clone(),
    };

    let _output_separator = output_separator.clone();
    let _print_handle = print_handle.clone();

    options = options
        .ext_handler(move |x, y| action_handler(x, y, &mut action_context))
        .accept_hook(move |state| {
            if !on_accept.is_empty() {
                let cmd = format_cli(state, &on_accept, None);
                if cmd.is_empty() {
                    ebog!("Invalid command template");
                    return vec![];
                } else {
                    let vars = state.make_env_vars();
                    Command::from_script(&cmd).envs(vars)._exec()
                }
            }

            let repeat = |s: String| {
                if atty::is(atty::Stream::Stdout) {
                    _print_handle.push(s);
                } else {
                    print!("{}{}", s, _output_separator);
                }
            };

            if let Some(template) = &output_template {
                format_cli(state, template, Some(&repeat));
            } else {
                state.map_selected_to_vec(|_, x| repeat(x.as_str().to_string()));
            };

            vec![]
        });

    if sync {
        handle.await._wbog(); // warn the mapreader error (?)
    }

    let ret = mm.pick(options).await;

    log::trace!("dumping print handle: {} items", print_handle.len()); // this apparently helps with a race condition that erases output?
    print_handle.map_to_vec(|s| {
        // log::trace!("{s}");
        print!("{}{}", s, output_separator);
    });

    log::trace!("Print complete");

    ret.map(|_| {})
}

use matchmaker::nucleo::{Line, Span};

fn inject_line(
    header_lines: usize,
    render_tx: RenderSender<MMAction>,
    injector: ConfigInjector,
) -> impl FnMut(String) -> Result<(), matchmaker::nucleo::WorkerError> + Send {
    let mut header_buf = Vec::with_capacity(header_lines);
    let mut remaining = header_lines;
    let injector = injector;

    // For each row, take the first line of each segmented column, building a Vec<Vec<Line>>
    move |line: String| {
        if remaining > 0 {
            let item = injector.wrap(line).unwrap();
            header_buf.push(item);
            remaining -= 1;

            if remaining == 0 {
                let rows: Vec<Vec<Line>> = header_buf
                    .drain(..)
                    .map(|_item| {
                        // With the new architecture, items are Indexed<String>.
                        // The splitting into columns is handled by the column preprocessor.
                        // For the header, we just use the string as a single line.
                        let s = _item.inner.clone();
                        vec![Line::from(s)]
                    })
                    .collect();

                let _ = render_tx.send(matchmaker::message::RenderCommand::HeaderTable(rows));
            }

            Ok(())
        } else {
            injector.push(line)
        }
    }
}

fn trim_trailing_empty(mut row: Vec<Line>) -> Vec<Line> {
    while matches!(row.last(), Some(line) if line.iter().all(|x| x.content.is_empty())) {
        row.pop();
    }

    row
}

fn to_static(line: Line<'_>) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|span| {
                Span::styled(
                    span.content.into_owned(), // force ownership
                    span.style,
                )
            })
            .collect::<Vec<_>>(),
    )
}
