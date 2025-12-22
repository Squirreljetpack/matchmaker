use std::{env, process::{Stdio, exit}};

use log::{debug, error};
use matchmaker::{
    ConfigInjector, ConfigMatchmaker, Matchmaker, OddEnds, action::ActionExt, config::{Config, MatcherConfig, StartConfig, utils::{get_config, write_config}}, event::EventLoop, make_previewer, message::Interrupt, nucleo::injector::{IndexedInjector, Injector, SegmentedInjector}, proc::{AppendOnly, map_chunks, map_reader_lines, previewer::Previewer, read_to_chunks, spawn}
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

pub fn make_mm<A: ActionExt>(config: Config<A>, event_loop: &EventLoop<A>) -> (ConfigMatchmaker, ConfigInjector, nucleo::Matcher, Previewer, AppendOnly<String>) {
    let Config {
        render,
        tui,
        previewer,
        matcher: MatcherConfig {
            matcher,
            worker,
            start: StartConfig { input_separator: delimiter, .. }
        },
        ..
    } = config;


    // make nucleo matcher
    let matcher = nucleo::Matcher::new(matcher.0);
    // make matchmaker
    let (mut mm, injector, OddEnds { formatter, splitter }) = Matchmaker::new_from_config(render, tui, worker);
    // make previewer
    let previewer = make_previewer(previewer, &mut mm, formatter.clone(), event_loop);

    // ---------------------- register handlers ---------------------------
    // print handler
    let print = AppendOnly::new();
    let _print = print.clone();
    let print_formatter = mm.worker.make_format_fn::<false>(|item| std::borrow::Cow::Borrowed(&item.inner.inner));
    mm.register_interrupt_handler(
        matchmaker::message::Interrupt::Print("".into()),
        move |state, i| {
            if let Interrupt::Print(template) = i
            && let Some(t) = state.current_raw() {
                let s = print_formatter(t, template);
                _print.push(s);
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

    (mm, injector, matcher, previewer, print)
}
