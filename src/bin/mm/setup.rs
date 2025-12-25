use std::{env, path::Path, process::{Stdio, exit}};

use log::{debug, error};
use matchmaker::{
    MatchError, Matchmaker, OddEnds, PickBuilder, binds::display_binds, config::{MatcherConfig, StartConfig}, event::EventLoop, make_previewer, message::Interrupt, nucleo::{Segmented, injector::{IndexedInjector, Injector, SegmentedInjector}}, proc::{AppendOnly, map_reader, spawn}
};
use crate::{Result, config::{Config, get_config, write_config}, types::config_file};

#[cfg(debug_assertions)]
use crate::config::write_config_dev;
use crate::parse::parse;


pub fn enter() -> Result<Config> {
    let args = env::args();
    let cli = parse(args.collect());
    log::debug!("{cli:?}");

    let cfg_path = {
        if let Some(cfg) = &cli.config && let p = Path::new(cfg) && p.is_file() {
            p
        } else {
            config_file()
        }
    };

    #[cfg(debug_assertions)]
    write_config_dev(cfg_path)?;

    if cli.dump_config && atty::is(atty::Stream::Stdout) {
        write_config(cfg_path)?;
        exit(0);
    }
    if cli.test_keys {
        super::crokey::main();
        exit(0);
    }

    let mut config = if cli.config.as_ref().is_none_or(|x| x.to_str().is_none() || Path::new(x).is_file()) {
        get_config(cfg_path)?
    } else {
        toml::from_str(cli.config.as_ref().unwrap().to_str().unwrap())?
    };
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
            if let Some(mut child) = spawn(&cmd, vars, Stdio::null(), Stdio::piped(), Stdio::null()) {
                if let Some(stdout) = child.stdout.take() {
                    map_reader(stdout, move |line| injector.push(line), delimiter);
                } else {
                    error!("Failed to capture stdout");
                }
            }
        }
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
        if let Some(mut child) = spawn(&default_command, vec![], Stdio::null(), Stdio::piped(), Stdio::null())
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

    mm.pick(PickBuilder::new().event_loop(event_loop).matcher(matcher.0).previewer(previewer)).await
}