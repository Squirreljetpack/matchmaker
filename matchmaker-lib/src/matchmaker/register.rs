use std::sync::Arc;

use cba::{_info, define_either, env_vars};
use log::warn;
use ratatui::text::Text;

use crate::{
    Matchmaker, RenderFn, SSS,
    config::PreviewerConfig,
    message::{Event, Interrupt},
    preview::{
        AppendOnly,
        previewer::{PreviewMessage, Previewer},
    },
    render::MMState,
    utils::text::is_empty,
};

define_either! {
    #[derive(serde::Serialize, serde::Deserialize)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Either<L, R = L> {
        Left,
        Right
    }
}

pub type AttachmentFormatter<T, D> = Either<
    Arc<RenderFn<T>>,
    for<'a, 'b, 'c> fn(&'a MMState<'b, 'c, T, D>, &'a str, Option<&dyn Fn(String)>) -> String,
>;

// we could check if template is empty here to avoid allocating but feels like it might be a footgun
pub fn use_formatter<T: SSS, D: 'static>(
    formatter: &AttachmentFormatter<T, D>,
    state: &MMState<'_, '_, T, D>,
    template: &str,
    repeat: Option<&dyn Fn(String)>,
) -> String {
    match formatter {
        Either::Left(f) => {
            if let Some(t) = state.current_raw() {
                f(t, template)
            } else {
                String::new()
            }
        }
        Either::Right(f) => f(state, template, repeat),
    }
}

// todo: this static bound shouldn't be necessary on S i don't know why its needed

/// A set of methods for registering the "standard" functionality for various interrupts/events.
/// These methods are prefixed with _ to indicate that library users will often prefer to override them.
/// See `matchmaker-cli/src/register.rs` for more examples.
impl<T: SSS, S, D: 'static> Matchmaker<T, S, D> {
    // technically we don't need concurrency but the cost should be negligable
    /// Causes [`Action::Print`] to print to stdout.
    pub fn _register_print_handler(
        &mut self,
        print_handle: AppendOnly<String>,
        output_separator: String,
        formatter: AttachmentFormatter<T, D>,
    ) {
        self.register_interrupt_handler(Interrupt::Print, move |state| {
            let template = state.payload().clone();
            let repeat = |s: String| {
                if atty::is(atty::Stream::Stdout) {
                    print_handle.push(s);
                } else {
                    print!("{}{}", s, output_separator);
                }
            };
            let s = use_formatter(&formatter, state, &template, Some(&repeat));
            if !s.is_empty() {
                repeat(s)
            }
        });
    }
}

/// Causes the program to display a preview of the active result.
/// The Previewer can be connected to [`Matchmaker`] using [`PickOptions::previewer`]
pub fn make_previewer<T: SSS, S, D: 'static>(
    mm: &mut Matchmaker<T, S, D>,
    previewer_config: PreviewerConfig, // note: help_str is provided separately so help_colors is ignored
    formatter: AttachmentFormatter<T, D>,
    help_factory: Box<dyn Fn(&crate::config::HelpDisplayConfig) -> Text<'static> + Send + Sync>,
) -> Previewer {
    // initialize previewer
    let (previewer, tx) = Previewer::new(previewer_config.clone());
    let preview_tx = tx.clone();
    let formatter_clone = formatter.clone();

    let help_config = previewer_config.help.clone();

    // preview handler
    // important that PreviewSet events don't accidentally trigger this!
    mm.register_event_handler(Event::CursorChange | Event::PreviewChange | Event::Synced, move |state, _| {
            // don't clobber previewset events
            if state.contains(Event::PreviewSet) {
                // code logic-wise, recieve PreviewSet::None semantically => will recieve PreviewMessage::Unset => we should skip anyways (events is immutable), altho semantically such a state should actually trigger a new preview tho it would be niche
                return;
            }

            if state.preview_visible() &&
            let m = state.preview_payload().clone() &&
            let cmd = use_formatter(&formatter, state, &m, None) &&
            !cmd.is_empty()
            {
                let mut envs = state.make_env_vars();
                let extra = env_vars!(
                    "COLUMNS" => state.previewer_area().map_or("0".to_string(), |r| r.width.to_string()),
                    "LINES" => state.previewer_area().map_or("0".to_string(), |r| r.height.to_string()),
                );
                envs.extend(extra);

                let msg = PreviewMessage::Run(cmd.clone(), envs);
                if preview_tx.send(msg.clone()).is_err() {
                    warn!("Failed to send to preview: {}", msg)
                }

                // -----------------
                let target = state.preview_ui.as_ref().and_then(|p| p.config.initial.index.as_ref().and_then(|index_col| {
                    state.current_raw().and_then(|item| {
                        state.picker_ui.worker.format_with(item, index_col).and_then(|t| atoi::atoi(t.as_bytes()))
                    })
                }));

                _info!("previewui scroll target": target);

                if let Some(p) = state.preview_ui {
                    p.set_target(target);
                    p.jump = Default::default();
                };

            } else if preview_tx.send(PreviewMessage::Stop).is_err() {
                warn!("Failed to send to preview: stop")
            }

            state.preview_set_payload = None; // reset None here instead of on consume so that ::Help can toggle
        }
    );

    mm.register_event_handler(Event::PreviewSet, move |state, _event| {
        if state.preview_visible() {
            let payload = state.preview_set_payload();
            let msg = match payload {
                Some(Err(m)) => {
                    let m = if is_empty(&m) {
                        help_factory(&help_config)
                    } else {
                        m
                    };
                    PreviewMessage::Set(m)
                }
                None => PreviewMessage::Unset,
                Some(Ok(template)) => {
                    let cmd = use_formatter(&formatter_clone, state, &template, None);
                    if cmd.is_empty() {
                        PreviewMessage::Stop
                    } else {
                        let mut envs = state.make_env_vars();
                        let extra = env_vars!(
                            "COLUMNS" => state.previewer_area().map_or("0".to_string(), |r| r.width.to_string()),
                            "LINES" => state.previewer_area().map_or("0".to_string(), |r| r.height.to_string()),
                        );
                        envs.extend(extra);
                        PreviewMessage::Run(cmd, envs)
                    }
                }
            };

            if tx.send(msg.clone()).is_err() {
                warn!("Failed to send: {}", msg)
            }
        }
    });

    previewer
}
