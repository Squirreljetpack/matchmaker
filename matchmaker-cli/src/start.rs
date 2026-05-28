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
    paths::{last_key_path, presets_path},
    register::MMExt,
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
    Action, ConfigInjector, MatchError, Matchmaker, OddEnds, PickOptions, SSS, acs,
    binds::{BindMap, BindMapExt},
    config::{CommandSetting, EnvValue, MatcherConfig, StartConfig},
    event::{EventLoop, RenderSender},
    make_previewer,
    message::Interrupt,
    nucleo::{
        ColumnIndexable,
        injector::{AnsiInjector, Either, IndexedInjector, Injector, SegmentedInjector},
    },
    preview::AppendOnly,
    render::MMState,
    use_formatter,
};
use matchmaker_partial::Apply;

pub fn enter(cli: Cli, partial: PartialConfig) -> anyhow::Result<Config> {
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

    // apply overrides
    for mut p in cli.r#override {
        if p.is_relative() && p.extension().is_none() {
            let main_p = presets_path().join(&p).join("main.toml");
            p = if !main_p.exists() {
                presets_path().join(p.with_extension("toml"))
            } else {
                main_p
            };
        }
        // no recursion because tail bad
        let o: PartialConfig = load_type(&p, |s| toml::from_str(s))?;

        if let Some(q) = &o.source {
            let source = p.parent().as_ref().unwrap().join(q);
            let o: PartialConfig = load_type(source, |s| toml::from_str(s))?;
            if o.source.is_some() {
                _wbog!("Ignoring 'source' field in nested override.");
            }
            config.apply(o);
        }

        config.apply(o);
        config.envs.insert(
            "MM_OVERRIDE".to_string(),
            EnvValue::new(p.to_string_lossy().to_string()),
        );
    }

    #[cfg(debug_assertions)]
    {
        config.tui.clear_on_exit = false;
    }
    config.apply(partial); // resolve config.exit first

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
    config.binds = BindMap::default_binds().modify(|x| x.extend(config.binds));
    config.binds.check_cycles().map_err(anyhow::Error::msg)?;
    config.binds.retain(|_, actions| !actions.is_empty());
    config.binds.resolve_semantics();

    for actions in config.binds.values() {
        for a in actions {
            if let Action::Custom(mm) = &a {
                mm.validate()?;
            }
        }
    }

    debug!("Config computed: {config:?}");

    Ok(config)
}

/// Spawns a tokio task mapping f to reader segments.
/// Read aborts on error. Read errors are logged.
pub fn map_reader<E: SSS + std::fmt::Display>(
    reader: impl Read + SSS,
    f: impl FnMut(String) -> Result<(), E> + SSS,
    input_separator: Option<char>,
    abort_empty: Option<RenderSender<MMAction>>,
) -> tokio::task::JoinHandle<Result<usize, MapReaderError<E>>> {
    tokio::task::spawn_blocking(move || {
        let ret = if let Some(delim) = input_separator {
            map_chunks::<true, E>(read_to_chunks(reader, delim), f)
        } else {
            map_reader_lines::<true, E>(reader, f)
        }
        .elog();

        if let Some(render_tx) = abort_empty
            && matches!(ret, Ok(0))
        {
            let _ = render_tx.send(matchmaker::message::RenderCommand::NoMatch);
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
                    && std::env::var("PASTEcmd")
                        .ok()
                        .map_or(true, |x| x.is_empty())
                {
                    envs.insert("PASTEcmd".to_string(), EnvValue::new(paste));
                }
            }
        }
    }

    if envs.get("PAGER").is_none() && std::env::var("PAGER").ok().map_or(true, |x| x.is_empty()) {
        let ev = EnvValue::new(guess_pager_cmd());
        envs.insert("PAGER".to_string(), ev);
    }

    if envs.get("EDITOR").is_none() && std::env::var("EDITOR").ok().map_or(true, |x| x.is_empty()) {
        let ev = EnvValue::new(guess_editor_cmd());
        envs.insert("PAGER".to_string(), ev);
    }

    // First pass: static envs
    for (k, v) in &envs {
        if !v.value.is_empty() && !v.exec {
            if v.force || std::env::var_os(k).is_none() {
                processed_envs.insert(k.clone(), v.value.to_string());
            }
        }
    }

    // Second pass: dynamic envs
    for (k, v) in &envs {
        if !v.value.is_empty() && v.exec {
            if v.force || std::env::var_os(k).is_none() {
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
    }

    processed_envs
}

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
                additional_commands,
                mode,
            },
        mut exit,
        mut envs,
        source: _,
    } = config;

    // -------- determine command ------------
    let mut initial_index = 0;
    if additional_commands.len() > 1 {
        if let Ok(index_str) = std::env::var("MM_INDEX") {
            if let Ok(index) = index_str.parse::<usize>() {
                if index < additional_commands.len() {
                    initial_index = index;
                }
            }
        }
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
            std::process::exit(1);
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

    let mut event_loop = EventLoop::with_binds(binds).with_tick_rate(render.tick_rate());

    // set event loop mode
    let mode = if let Some(m) = mode {
        m
    } else {
        match (
            !initial_cmd.is_empty(), // has command => t0
            atty::is(atty::Stream::Stdout),
        ) {
            (true, true) => "tty",
            (true, false) => "t0",
            (false, true) => "piped",
            (false, false) => "t1",
        }
        .to_string()
    };
    log::trace!("mode: {}", mode);
    if let Ok(mut m) = matchmaker::MODE.lock() {
        *m = mode;
    }

    // make matcher and matchmaker with matchmaker-and-matcher-maker
    let copy_trailing_newline = tui.copy_trailing_newline;
    let (
        mut mm,
        injector,
        OddEnds {
            splitter,
            hidden_columns,
            has_error,
        },
    ) = Matchmaker::new_from_config(render, tui, worker, columns, exit, preprocess);

    if has_error {
        return Err(MatchError::Abort(1));
    }
    // make previewer

    if !event_loop.binds.strip_traces() {
        wbog!(
            "Action descriptions did not follow the required alternating (nonempty/empty) pattern."
        );
    }
    let cli_formatter = Either::Right(
        crate::formatter::format_cli
            as for<'a, 'b, 'c> fn(
                &'a MMState<'b, 'c, matchmaker::ConfigMMItem, matchmaker::ConfigMMInnerItem>,
                &'a str,
                Option<&dyn Fn(String)>,
            ) -> String,
    );
    let binds = event_loop.binds.clone();
    let previewer = make_previewer(
        &mut mm,
        previewer,
        cli_formatter.clone(),
        Box::new(move |config, mode| matchmaker::binds::display_help(&binds, config, Some(mode))),
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

    // ---------------------- register handlers ---------------------------
    // print handler (no quoting)
    mm._register_print_handler(
        print_handle.clone(),
        output_separator.clone(),
        cli_formatter.clone(),
    );

    // execute handlers
    mm.register_execute_handler(cli_formatter.clone());
    mm._register_execute_async_handler(cli_formatter.clone());
    mm._register_async_copy_handler(cli_formatter.clone(), copy_trailing_newline);
    mm._register_become_handler(cli_formatter.clone());

    // reload handler
    let reload_formatter = cli_formatter.clone();
    let reload_render_tx = render_tx.clone();

    let mut cmd = initial_cmd;
    mm.register_interrupt_handler(Interrupt::Reload, move |state| {
        let injector = state.injector();
        let injector = IndexedInjector::new_globally_indexed(injector);
        let injector = SegmentedInjector::new(injector, splitter.clone());
        let injector = AnsiInjector::new(injector, preprocess);

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

            if let Some(stdout) = Command::from_script(&cmd)
                .envs(vars)
                .stdin(Stdio::null())
                .args(&*COMMAND_ARGS.lock().unwrap())
                .spawn_piped()
                ._elog()
            {
                map_reader(stdout, push_fn, separator.or(input_separator), None);
            }
        }
    });

    debug!("{mm:?}");

    let mut action_context = ActionContext {
        bind_tx,
        render_tx: render_tx.clone(),
        additional_commands: (additional_commands, initial_index),
        output_template,
        print_handle: print_handle.clone(),
        output_separator: output_separator.clone(),
    };

    options = options
        .ext_handler(move |x, y| action_handler(x, y, &mut action_context))
        .ext_aliaser(|a, _state| match a {
            Action::Accept => acs![MMAction::Accept],
            _ => acs![a],
        });

    // ----------- read -----------------------
    let handle = if !atty::is(atty::Stream::Stdin) && !no_read {
        let stdin = std::io::stdin();
        map_reader(
            stdin,
            push_fn,
            input_separator,
            abort_empty.then_some(render_tx),
        )
    } else if !command.is_empty()
        && let Some(stdout) = Command::from_script(&command)
            .envs(envs)
            .args(&*COMMAND_ARGS.lock().unwrap())
            .spawn_piped()
            ._ebog()
    {
        map_reader(
            stdout,
            push_fn,
            separator.or(input_separator),
            abort_empty.then_some(render_tx),
        )
    } else {
        eprintln!("error: no input detected.");
        std::process::exit(99)
    };

    if sync {
        handle.await._wbog(); // warn the mapreader error (?)
    }

    let ret = mm.pick(options).await;

    print_handle.map_to_vec(|s| {
        log::trace!("{s}"); // this apparently helps with a race condition that erases output?
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
            let item = injector.injector.wrap(item).unwrap();
            header_buf.push(item);
            remaining -= 1;

            if remaining == 0 {
                let rows: Vec<Vec<Line>> = header_buf
                    .drain(..)
                    .map(|seg| {
                        let row = (0..seg.len())
                            .map(move |i| {
                                let mut s = seg.get_text(i);
                                if s.lines.is_empty() {
                                    Line::default()
                                } else {
                                    to_static(s.lines.remove(0))
                                }
                            })
                            .collect();
                        trim_trailing_empty(row)
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
