use std::{
    io::Read,
    path::Path,
    process::{Command, exit},
    sync::Arc,
};

use crate::{clap::Cli, config::PartialConfig};
use crate::{config::Config, paths::default_config_path};
use cli_boilerplate_automation::{
    bait::{OptionExt, ResultExt},
    bo::{
        MapReaderError, load_type_or_default, map_chunks, map_reader_lines, read_to_chunks,
        write_str,
    },
    bog::BogOkExt,
};
use cli_boilerplate_automation::{bo::load_type, broc::CommandExt};
use log::debug;
use matchmaker::{
    ConfigInjector, MatchError, Matchmaker, OddEnds, PickOptions, SSS,
    action::NullActionExt,
    binds::display_binds,
    config::{MatcherConfig, StartConfig},
    event::{EventLoop, RenderSender},
    make_previewer,
    message::Interrupt,
    nucleo::{
        ColumnIndexable,
        injector::{AnsiInjector, IndexedInjector, Injector, SegmentedInjector},
    },
    preview::AppendOnly,
};
use matchmaker_partial::Apply;

pub fn enter(cli: Cli, partial: PartialConfig) -> anyhow::Result<Config> {
    if cli.test_keys {
        super::crokey::main();
        exit(0);
    }

    let (cfg_path, mut config): (_, Config) = {
        // parse cli arg as path or toml
        if let Some(cfg) = &cli.config {
            let p = Path::new(cfg);
            (
                p,
                if p.is_file() || p.to_str().is_none() {
                    load_type(p, |s| toml::from_str(s))._ebog().or_exit()
                } else {
                    toml::from_str(cfg.to_str().unwrap())?
                },
            )
        } else {
            // get config from default location or default config
            let p = default_config_path();
            #[cfg(debug_assertions)]
            write_str(p, include_str!("../assets/dev.toml")).unwrap();
            (p, load_type_or_default(p, |s| toml::from_str(s)))
        }
    };

    // let original = config.clone();
    config.apply(partial);
    // log::debug!("unchanged: {}", original == config);

    if cli.fullscreen {
        config.tui.layout = None;
    }

    if cli.dump_config {
        let contents = toml::to_string_pretty(&config).expect("failed to serialize to TOML");

        // if stdout: dump the default cfg with comments
        if atty::is(atty::Stream::Stdout) {
            write_str(cfg_path, include_str!("../assets/config.toml"))?;
        } else {
            // if piped: dump the current cfg
            std::io::Write::write_all(&mut std::io::stdout(), contents.as_bytes())?;
        }

        exit(0);
    }

    log::debug!("{config:?}");

    Ok(config)
}

/// Spawns a tokio task mapping f to reader segments.
/// Read aborts on error. Read errors are logged.
pub fn map_reader<E: SSS + std::fmt::Display>(
    reader: impl Read + SSS,
    f: impl FnMut(String) -> Result<(), E> + SSS,
    input_separator: Option<char>,
    abort_empty: Option<RenderSender<NullActionExt>>,
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
            let _ = render_tx.send(matchmaker::message::RenderCommand::QuitEmpty);
        }
        ret
    })
}

pub async fn start(config: Config, no_read: bool) -> Result<(), MatchError> {
    let Config {
        render,
        tui,
        previewer,
        matcher:
            MatcherConfig {
                matcher,
                worker,
                exit,
                start:
                    StartConfig {
                        input_separator,
                        command,
                        sync,
                        output_separator,
                        output_template,
                    },
            },
        binds,
    } = config;

    let abort_empty = exit.abort_empty;
    let header_lines = render.header.header_lines;
    let print_handle = AppendOnly::new();
    let output_separator = output_separator.clone().unwrap_or("\n".into());
    let ansi = worker.ansi;

    let event_loop = EventLoop::with_binds(binds).with_tick_rate(render.tick_rate());
    // make matcher and matchmaker with matchmaker-and-matcher-maker
    let (
        mut mm,
        injector,
        OddEnds {
            formatter,
            splitter,
        },
    ) = Matchmaker::new_from_config(render, tui, worker, exit);
    // make previewer
    let help_str = display_binds(&event_loop.binds, Some(&previewer.help_colors));
    let previewer = make_previewer(&mut mm, previewer, formatter.clone(), help_str);

    // ---------------------- register handlers ---------------------------
    // print handler (no quoting)
    let print_formatter = Arc::new(mm.worker.default_format_fn::<false>(|item| item.to_cow()));
    mm.register_print_handler(
        print_handle.clone(),
        output_separator.clone(),
        print_formatter.clone(),
    );

    // execute handlers
    mm.register_execute_handler(formatter.clone());
    mm.register_become_handler(formatter.clone());

    // reload handler
    let reload_formatter = formatter.clone();
    mm.register_interrupt_handler(Interrupt::Reload, move |state| {
        let injector = state.injector();
        let injector = IndexedInjector::new_globally_indexed(injector);
        let injector = SegmentedInjector::new(injector, splitter.clone());
        let injector = AnsiInjector::new(injector, ansi);

        if let Some(t) = state.current_raw() {
            let cmd = reload_formatter(t, state.payload());
            let vars = state.make_env_vars();
            debug!("Reloading: {cmd}");
            if let Some(stdout) = Command::from_script(&cmd).envs(vars).spawn_piped()._elog() {
                map_reader(
                    stdout,
                    move |line| injector.push(line),
                    input_separator,
                    None,
                );
            }
        }
    });

    debug!("{mm:?}");

    let mut options = PickOptions::new()
        .event_loop(event_loop)
        .matcher(matcher.0)
        .previewer(previewer);

    let render_tx = options.render_tx();

    // ----------- read -----------------------
    let push_fn = inject_line(header_lines, render_tx.clone(), injector);
    let handle = if !atty::is(atty::Stream::Stdin) && !no_read {
        let stdin = std::io::stdin();
        map_reader(
            stdin,
            push_fn,
            input_separator,
            abort_empty.then_some(render_tx),
        )
    } else if !command.is_empty()
        && let Some(stdout) = Command::from_script(&command).spawn_piped()._ebog()
    {
        map_reader(
            stdout,
            push_fn,
            input_separator,
            abort_empty.then_some(render_tx),
        )
    } else {
        eprintln!("error: no input detected.");
        std::process::exit(99)
    };

    if sync {
        let _ = handle.await; // ignore the mapreader error?
    }

    mm.pick(options).await.map(|v| {
        print_handle.map_to_vec(|s| print!("{}{}", s, output_separator));

        for item in v {
            let s = if let Some(_s) = &output_template {
                // print_formatter(
                //     &matchmaker::nucleo::Indexed {
                //         index: 0,
                //         inner: item,
                //     },
                //     s,
                // )
                todo!()
            } else {
                item.to_cow()
            };

            print!("{}{}", s, output_separator)
        }
    })
}

use matchmaker::nucleo::{Line, Span};

fn inject_line(
    header_lines: usize,
    render_tx: RenderSender,
    injector: ConfigInjector,
) -> impl FnMut(String) -> Result<(), matchmaker::nucleo::WorkerError> + Send {
    let mut header_buf = Vec::with_capacity(header_lines);
    let mut remaining = header_lines;
    let injector = injector;

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
                        (0..seg.len())
                            .map(move |i| {
                                let mut s = seg.get_text(i);
                                if s.lines.is_empty() {
                                    Line::default()
                                } else {
                                    to_static(s.lines.remove(0))
                                }
                            })
                            .collect()
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
