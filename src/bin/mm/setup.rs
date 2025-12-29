use std::{env, io::Read, path::Path, process::{Stdio, exit}};

use cli_boilerplate_automation::{bo::{MapReaderError, map_chunks, map_reader_lines, read_to_chunks, write_str}};
use cli_boilerplate_automation::{bo::load_type, bog::BogUnwrapExt, broc::spawn_script};
use log::{debug, error};
use matchmaker::{
    MMItem, MatchError, Matchmaker, OddEnds, PickOptions, binds::display_binds, config::{MatcherConfig, StartConfig}, efx, event::EventLoop, make_previewer, message::Interrupt, nucleo::{Segmented, injector::{IndexedInjector, Injector, SegmentedInjector}}, preview::AppendOnly
};
use crate::{config::Config, types::default_config_path};
use crate::parse::parse;

pub fn enter() -> anyhow::Result<Config> {
    let args = env::args();
    let cli = parse(args.collect());
    log::debug!("{cli:?}");
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
                    load_type(p, |s| toml::from_str(s)).or_exit()
                } else {
                    toml::from_str(cfg.to_str().unwrap())?
                }
            )
        } else {
            // get config from default location or default config
            let p = default_config_path();

            // always update dev config in standard location of latest debug build
            #[cfg(debug_assertions)]
            write_str(p, include_str!("../../../assets/dev.toml")).unwrap();
            (
                p,
                if p.is_file() {
                    load_type(p, |s| toml::from_str(s)).or_exit()
                } else {
                    toml::from_str(include_str!("../../../assets/config.toml"))?
                }
            )
        }
    };

    // todo
    cli.merge_config(&mut config)?;

    if cli.dump_config {
        let contents = toml::to_string_pretty(&config)
        .expect("failed to serialize to TOML");

        // if stdout: dump the default cfg with comments
        if atty::is(atty::Stream::Stdout) {
            write_str(cfg_path, include_str!("../../../assets/config.toml"))?;
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
pub fn map_reader<E: MMItem>(reader: impl Read + MMItem, f: impl FnMut(String) -> Result<(), E> + MMItem, input_separator: Option<char>) -> tokio::task::JoinHandle<Result<(), MapReaderError<E>>> {
    if let Some(delim) = input_separator {
        tokio::spawn(async move {
            map_chunks::<true, E>(read_to_chunks(reader, delim), f)
        })
    } else {
        tokio::spawn(async move {
            map_reader_lines::<true, E>(reader, f)
        })
    }
}

pub async fn pick(config: Config, print_handle: AppendOnly<String>) -> Result<Vec<Segmented<String>>, MatchError> {
    let Config {
        render,
        tui,
        previewer,
        matcher: MatcherConfig {
            matcher,
            worker,
            start: StartConfig { input_separator: delimiter, default_command, sync, .. }
        },
        binds,
    } = config;

    let event_loop = EventLoop::with_binds(binds).with_tick_rate(render.tick_rate());
    // make matcher and matchmaker with matchmaker-and-matcher-maker
    let (mut mm, injector, OddEnds { formatter, splitter }) = Matchmaker::new_from_config(render, tui, worker);
    // make previewer
    let help_str = display_binds(&event_loop.binds, Some(&previewer.help_colors));
    let previewer = make_previewer(&mut mm, previewer, formatter.clone(), help_str);

    // ---------------------- register handlers ---------------------------
    // print handler
    let print_formatter = std::sync::Arc::new(mm.worker.make_format_fn::<false>(|item| std::borrow::Cow::Borrowed(&item.inner.inner)));
    mm.register_print_handler(print_handle, print_formatter);

    // execute handlers
    mm.register_execute_handler(formatter.clone());
    mm.register_become_handler(formatter.clone());

    // reload handler
    let reload_formatter = formatter.clone();
    mm.register_interrupt_handler(Interrupt::Reload("".into()), move |state, interrupt| {
        let injector = state.injector();
        let injector= IndexedInjector::new(injector, 0);
        let injector= SegmentedInjector::new(injector, splitter.clone());

        if let Interrupt::Reload(template) = interrupt
        && let Some(t) = state.current_raw() {
            let cmd = reload_formatter(t, template);
            let vars = vec![];
            // let extra = env_vars!(
            //     "FZF_PREVIEW_COMMAND" => preview_cmd,
            // );
            // vars.extend(extra);
            debug!("Reloading: {cmd}");
            if let Some(mut child) = spawn_script(&cmd, vars, Stdio::null(), Stdio::piped(), Stdio::null()) {
                if let Some(stdout) = child.stdout.take() {
                    map_reader(stdout, move |line| injector.push(line), delimiter);
                } else {
                    error!("Failed to capture stdout");
                }
            }
        }
        efx![]
    });

    debug!("{mm:?}");

    // ----------- read -----------------------
    let handle = if !atty::is(atty::Stream::Stdin) {
        let stdin = std::io::stdin();
        map_reader(
            stdin,
            move |line| {
                injector.push(line)
            },
            delimiter
        )
    } else if !default_command.is_empty() {
        if let Some(mut child) = spawn_script(&default_command, vec![], Stdio::null(), Stdio::piped(), Stdio::null())
        && let Some(stdout) = child.stdout.take() {
            map_reader(
                stdout,
                move |line| {
                    injector.push(line)
                },
                delimiter
            )
        } else {
            eprintln!("error: no stdout from default command.");
            exit(99)
        }
    } else {
        eprintln!("error: no input detected.");
        exit(99)
    };

    if sync {
        let _ = handle.await;
    }

    mm.pick(PickOptions::new().event_loop(event_loop).matcher(matcher.0).previewer(previewer)).await
}