use std::{env, process::{Stdio, exit}};

use log::{debug, error, info, warn};
use matchmaker::{
    ConfigInjector, ConfigMatchmaker, Matchmaker, OddEnds,
    config::{MainConfig, MatcherConfig, StartConfig, utils::{get_config, write_config}},
    env_vars,
    message::{Event, Interrupt},
    nucleo::injector::{IndexedInjector, Injector, SegmentedInjector},
    proc::{AppendOnly, exec, previewer::{PreviewMessage, Previewer}, spawn, tty_or_null, map_chunks, map_reader_lines, read_to_chunks},
};
use crate::Result;

use crate::parse::parse;

pub fn enter() -> Result<MainConfig> {
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
    if cli.test_keys {
        super::crokey::main();
        exit(0);
    }

    let mut config = get_config(&cli.config)?;
    cli.merge_config(&mut config);

    if cli.dump_config && ! atty::is(atty::Stream::Stdout) {
        let toml_str = toml::to_string_pretty(&config)
        .expect("failed to serialize to TOML");
        std::io::Write::write_all(&mut std::io::stdout(), toml_str.as_bytes())?;
        exit(0);
    }

    log::debug!("{config:?}");

    Ok(config)
}

pub fn make_mm(config: MainConfig) -> (ConfigMatchmaker, ConfigInjector, nucleo::Matcher, Previewer, AppendOnly<String>) {
    let MainConfig {
        config,
        previewer,
        matcher: MatcherConfig {
            matcher,
            mm,
            run: StartConfig { input_separator: delimiter, .. }
        }
    } = config;

    let matcher = nucleo::Matcher::new(matcher.0);

    let (mut previewer, tx) = Previewer::new(previewer);
    let preview = previewer.view();

    let (mut mm, injector, OddEnds { formatter, splitter }) = Matchmaker::new_from_config(config, mm);

    // clone formatters for moving
    let preview_formatter = formatter.clone();
    let execute_formatter = formatter.clone();
    let execute_preview_formatter = formatter.clone();
    let become_formatter = formatter.clone();
    let become_preview_formatter = formatter.clone();
    let reload_formatter = formatter.clone();

    // connect previewer
    previewer.connect_controller(mm.get_controller());
    mm.connect_preview(preview);

    // ---------------------- register handlers ---------------------------
    // preview handler
    mm.register_event_handler([Event::CursorChange, Event::PreviewChange], move |state, event| {
        match event {
            Event::CursorChange | Event::PreviewChange => {
                if state.preview_show &&
                let Some(t) = state.current_raw() &&
                !state.preview_payload().is_empty()
                {
                    let cmd = preview_formatter(t, state.preview_payload());
                    let mut envs = state.make_env_vars();
                    let extra = env_vars!(
                        "COLUMNS" => state.previewer_area.map_or("0".to_string(), |r| r.width.to_string()),
                        "LINES" => state.previewer_area.map_or("0".to_string(), |r| r.height.to_string()),
                    );
                    envs.extend(extra);

                    let msg = PreviewMessage::Run(cmd.clone(), vec![]);
                    if tx.send(msg.clone()).is_err() {
                        warn!("Failed to send: {}", msg)
                    }
                }
            },
            _ => {}
        }
    });

    // print handler
    let print = AppendOnly::new();
    let _print = print.clone();
    let print_formatter = mm.worker.make_format_fn::<false>(|item| &item.inner.inner);
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

    // execute handler
    mm.register_interrupt_handler(Interrupt::Execute("".into()), move |state, interrupt| {
        if let Interrupt::Execute(template) = interrupt &&
        let Some(t) = state.current_raw() {
            let cmd = execute_formatter(t, template);
            let mut vars = state.make_env_vars();
            let preview_cmd = execute_preview_formatter(t, state.preview_payload());
            let extra = env_vars!(
                "FZF_PREVIEW_COMMAND" => preview_cmd,
            );
            vars.extend(extra);
            let tty = tty_or_null();
            if let Some(mut child) = spawn(&cmd, vars, tty, Stdio::inherit(), Stdio::inherit()) {
                match child.wait() {
                    Ok(i) => {
                        info!("Command [{cmd}] exited with {i}")
                    },
                    Err(e) => {
                        info!("Failed to wait on command [{cmd}]: {e}")
                    }
                }
            }
        }
    });

    mm.register_interrupt_handler(Interrupt::Become("".into()), move |state, interrupt| {
        if let Interrupt::Become(template) = interrupt &&
        let Some(t) = state.current_raw() {
            let cmd = become_formatter(t, template);
            let mut vars = state.make_env_vars();
            let preview_cmd = become_preview_formatter(t, state.preview_payload());
            let extra = env_vars!(
                "FZF_PREVIEW_COMMAND" => preview_cmd,
            );
            vars.extend(extra);
            debug!("Becoming: {cmd}");
            exec(&cmd, vars);
        }
    });

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
