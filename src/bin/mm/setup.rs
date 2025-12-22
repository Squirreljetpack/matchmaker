use std::{env, process::{Stdio, exit}};

use log::{debug, error};
use matchmaker::{
    MatchError, Matchmaker, OddEnds, PickBuilder, binds::display_binds, config::{Config, MatcherConfig, StartConfig, utils::{get_config, write_config}}, event::EventLoop, make_previewer, message::Interrupt, nucleo::{Segmented, injector::{IndexedInjector, Injector, SegmentedInjector}}, proc::{AppendOnly, map_chunks, map_reader, map_reader_lines, read_to_chunks, spawn}
};
use crate::Result;

use crate::parse::parse;


pub fn enter() -> Result<Config> {
    let args = env::args();
    let cli = parse(args.collect());
    log::debug!("{cli:?}");

    #[cfg(debug_assertions)]
    matchmaker::config::utils::write_config_dev(&cli.config)?;

    if cli.dump_config && atty::is(atty::Stream::Stdout) {
        write_config(&cli.config)?;
        exit(0);
    }
    if cli.test_keys {
        super::crokey::main();
        exit(0);
    }

    let mut config = get_config(&cli.config)?;
    cli.merge_config(&mut config)?;

    if cli.dump_config && ! atty::is(atty::Stream::Stdout) {
        let toml_str = toml::to_string_pretty(&config)
        .expect("failed to serialize to TOML");
        std::io::Write::write_all(&mut std::io::stdout(), toml_str.as_bytes())?;
        exit(0);
    }

    log::debug!("{config:?}");

    Ok(config)
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
    let previewer = make_previewer(previewer, &mut mm, formatter.clone(), help_str);

    // ---------------------- register handlers ---------------------------
    // print handler
    let print_formatter = mm.worker.make_format_fn::<false>(|item| std::borrow::Cow::Borrowed(&item.inner.inner));
    mm.register_interrupt_handler(
        matchmaker::message::Interrupt::Print("".into()),
        move |state, i| {
            if let Interrupt::Print(template) = i
            && let Some(t) = state.current_raw() {
                let s = print_formatter(t, template);
                print_handle.push(s);
            }
        },
    );

    // execute handlers
    mm.register_execute_handler(formatter.clone());
    mm.register_become_handler(formatter.clone());

    // reload handler
    let reload_formatter = formatter.clone();
    mm.register_interrupt_handler(Interrupt::Reload("".into()), move |state, interrupt| {
        let injector = state.injector();
        let injector= IndexedInjector::new(injector, ());
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
            if let Some(mut child) = spawn(&cmd, vars, Stdio::null(), Stdio::piped(), Stdio::null()) {
                if let Some(stdout) = child.stdout.take() {
                    let _handle = if let Some(delim) = delimiter {
                        tokio::spawn(async move {
                            map_chunks::<true>(read_to_chunks(stdout, delim), |line| injector.push(line).map_err(|e| e.into()))
                        })
                    } else {
                        tokio::spawn(async move {
                            map_reader_lines::<true>(stdout, |line| injector.push(line).map_err(|e| e.into()))
                        })
                    };
                } else {
                    error!("Failed to capture stdout");
                }
            }
        }
    });

    debug!("{mm:?}");

    // read stdin
    let handle = if !atty::is(atty::Stream::Stdin) {
        let stdin = std::io::stdin();
        map_reader(
            stdin,
            move |line| {
                injector.push(line).map_err(|e| e.into())
            },
            delimiter
        )
    } else if !default_command.is_empty() {
        if let Some(mut child) = spawn(&default_command, vec![], Stdio::null(), Stdio::piped(), Stdio::null())
        && let Some(stdout) = child.stdout.take() {
            map_reader(
                stdout,
                move |line| {
                    injector.push(line).map_err(|e| e.into())
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

    mm.pick_with(PickBuilder::new().event_loop(event_loop).matcher(matcher.0).previewer(previewer)).await
}