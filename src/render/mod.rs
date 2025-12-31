mod dynamic;
mod state;
mod state_effects;

pub use dynamic::*;
pub use state::*;
pub use state_effects::*;
// ------------------------------

use std::io::Write;

use anyhow::Result;
use log::{info, warn};
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc;

use crate::action::{Action, ActionExt, ActionAliaser, ActionExtHandler};
use crate::config::{CursorSetting, ExitConfig};
use crate::message::{Event, Interrupt, RenderCommand};
use crate::tui::Tui;
use crate::ui::{DisplayUI, InputUI, OverlayUI, PickerUI, PreviewUI, ResultsUI, UI};
use crate::{MatchError, SSS, Selection};

// todo: we can make it return a stack allocated smallvec ig
fn apply_aliases<T: SSS, S: Selection, A: ActionExt>(
    buffer: &mut Vec<RenderCommand<A>>,
    aliaser: ActionAliaser<T, S, A>,
    state: &MMState<'_, T, S>
) -> Effects {
    let mut out = Vec::new();
    let mut effects = Effects::new();

    for cmd in buffer.drain(..) {
        match cmd {
            RenderCommand::Action(a) => {
                out.extend(aliaser(a, state).0.into_iter().map(RenderCommand::Action))
            },
            RenderCommand::Effect(e) => {
                effects.insert(e);
            },
            other => out.push(other),
        }
    }

    *buffer = out;
    effects
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn render_loop<'a, W: Write, T: SSS, S: Selection, A: ActionExt>(
    mut ui: UI,
    mut picker_ui: PickerUI<'a, T, S>,
    mut preview_ui: Option<PreviewUI>,
    mut tui: Tui<W>,

    mut overlay_ui: Option<OverlayUI<A>>,
    exit_config: ExitConfig,

    mut render_rx: mpsc::UnboundedReceiver<RenderCommand<A>>,
    controller_tx: mpsc::UnboundedSender<Event>,

    dynamic_handlers: DynamicHandlers<T, S>,
    ext_handler: Option<ActionExtHandler<T, S, A>>,
    ext_aliaser: Option<ActionAliaser<T, S, A>>,
    initializer: Option<Box<dyn FnOnce()>>
    // signal_handler: Option<(&'static std::sync::atomic::AtomicUsize, SignalHandler<T, S>)>,
) -> Result<Vec<S>, MatchError> {
    if let Some(initializer) = initializer {
        initializer()
    }
    let mut buffer = Vec::with_capacity(256);

    let mut state: State<S> = State::new();

    if let Some(ref preview_ui) = preview_ui
    && !preview_ui.command().is_empty()
    {
        state.update_preview(preview_ui.command());
    }

    while render_rx.recv_many(&mut buffer, 256).await > 0 {
        let mut did_pause = false;
        let mut did_exit = false;
        let mut did_resize = false;

        // todo: why exactly can we not borrow the picker_ui mutably?
        let mut effects = if let Some(aliaser) = ext_aliaser {
            let state = state.dispatcher(&ui, &picker_ui, preview_ui.as_ref());
            apply_aliases(&mut buffer, aliaser, &state)
        } else {
            // buffer
            // .extract_if(.., |cmd|
            //     matches!(cmd, RenderCommand::Effect(_))
            // )
            // .map(|cmd|
            //     match cmd {
            //         RenderCommand::Effect(e) => e,
            //         _ => unreachable!(),
            //     }
            // )
            // .collect::<Effects>()
            Effects::new()
        };

        // todo: benchmark vs drain
        for event in buffer.drain(..) {
            let mut interrupt = Interrupt::None;

            let PickerUI {
                input,
                results,
                worker,
                selections,
                header,
                footer,
                ..
            } = &mut picker_ui;

            if !matches!(event, RenderCommand::Tick) {
                info!("Recieved {event:?}");
            }

            match event {
                RenderCommand::Input(c) => {
                    if let Some(x) = overlay_ui.as_mut() && x.handle_input(c) {
                        continue;
                    }
                    input.input.insert(input.cursor as usize, c);
                    input.cursor += 1;
                }
                RenderCommand::Resize(area) => {
                    tui.resize(area);
                    ui.area = area;
                }
                RenderCommand::Refresh => {
                    tui.redraw();
                }
                RenderCommand::Effect(_e) => {
                    effects.insert(_e);
                }
                RenderCommand::Action(action) => {
                    if let Some(x) = overlay_ui.as_mut() && x.handle_action(&action) {
                        continue;
                    }
                    match action {
                        Action::Select => {
                            if let Some(item) = worker.get_nth(results.index()) {
                                selections.sel(item);
                            }
                        }
                        Action::Deselect => {
                            if let Some(item) = worker.get_nth(results.index()) {
                                selections.desel(item);
                            }
                        }
                        Action::Toggle => {
                            if let Some(item) = worker.get_nth(results.index()) {
                                selections.toggle(item);
                            }
                        }
                        Action::CycleAll => {
                            selections.cycle_all_bg(worker.raw_results());
                        }
                        Action::Accept => {
                            if selections.is_empty() {
                                if let Some(item) = worker.get_nth(results.index())
                                {
                                    selections.sel(item);
                                } else if !exit_config.allow_empty {
                                    continue;
                                }
                            }
                            return Ok(selections.output().collect::<Vec<S>>());
                        }
                        Action::Quit(code) => {
                            return Err(MatchError::Abort(code.0));
                        }

                        // UI
                        Action::SetHeader(context) => {
                            if let Some(s) = context {
                                header.set(s);
                            } else {
                                todo!()
                            }
                        }
                        Action::SetFooter(context) => {
                            if let Some(s) = context {
                                footer.set(s);
                            } else {
                                todo!()
                            }
                        }
                        // this sometimes aborts the viewer on some files, why?
                        Action::CyclePreview => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.cycle_layout();
                                if !p.command().is_empty() {
                                    state.update_preview(p.command());
                                }
                            }
                        }
                        Action::Preview(context) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if !state.update_preview(context.as_str()) {
                                    p.toggle_show()
                                } else {
                                    p.show::<true>();
                                }
                            };
                        }
                        Action::Help(context) => {
                            if let Some(p) = preview_ui.as_mut() {
                                // empty payload signifies help
                                if !state.update_preview_set(context) {
                                    state.update_preview_unset()
                                } else {
                                    p.show::<true>();
                                }
                            };
                        }
                        Action::SwitchPreview(idx) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if let Some(idx) = idx {
                                    if !p.set_idx(idx) && !state.update_preview(p.command()) {
                                        p.toggle_show();
                                    }
                                } else {
                                    p.toggle_show()
                                }
                            }
                        }
                        Action::SetPreview(idx) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if let Some(idx) = idx {
                                    p.set_idx(idx);
                                } else {
                                    state.update_preview(p.command());
                                }
                            }
                        }
                        Action::ToggleWrap => {
                            results.wrap(!results.is_wrap());
                        }
                        Action::ToggleWrapPreview => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.wrap(!p.is_wrap());
                            }
                        }

                        // Programmable
                        Action::Execute(context) => {
                            interrupt = Interrupt::Execute(context);
                        }
                        Action::Become(context) => {
                            interrupt = Interrupt::Become(context);
                        }
                        Action::Reload(context) => {
                            interrupt = Interrupt::Reload(context);
                        }
                        Action::Print(context) => {
                            interrupt = Interrupt::Print(context);
                        }

                        Action::SetInput(context) => {
                            input.set(context, u16::MAX);
                        }
                        Action::Column(context) => {
                            results.toggle_col(context);
                        }
                        Action::CycleColumn => {
                            results.cycle_col();
                        }
                        // Edit
                        Action::ForwardChar => input.forward_char(),
                        Action::BackwardChar => input.backward_char(),
                        Action::ForwardWord => input.forward_word(),
                        Action::BackwardWord => input.backward_word(),
                        Action::DeleteChar => input.delete(),
                        Action::DeleteWord => input.delete_word(),
                        Action::DeleteLineStart => input.delete_line_start(),
                        Action::DeleteLineEnd => input.delete_line_end(),
                        Action::Cancel => input.cancel(),

                        // Navigation
                        Action::Up(x) | Action::Down(x) => {
                            let next = matches!(action, Action::Down(_)) ^ results.reverse();
                            for _ in 0..x.into() {
                                if next {
                                    results.cursor_next();
                                } else {
                                    results.cursor_prev();
                                }
                            }
                        }
                        Action::PreviewUp(n) => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.up(n.into())
                            }
                        }
                        Action::PreviewDown(n) => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.down(n.into())
                            }
                        }
                        Action::PreviewHalfPageUp => todo!(),
                        Action::PreviewHalfPageDown => todo!(),
                        Action::Pos(pos) => {
                            let pos = if pos >= 0 {
                                pos as u32
                            } else {
                                results.status.matched_count.saturating_sub((-pos) as u32)
                            };
                            results.cursor_jump(pos);
                        }
                        Action::InputPos(pos) => {
                            let pos = if pos >= 0 {
                                pos as u16
                            } else {
                                (input.len() as u16).saturating_sub((-pos) as u16)
                            };
                            input.cursor = pos;
                        }

                        // Experimental/Debugging
                        Action::Redraw => {
                            tui.redraw();
                        }
                        Action::Custom(e) => {
                            if let Some(handler) = ext_handler {
                                let mut dispatcher = state.dispatcher(&ui, &picker_ui, preview_ui.as_ref());
                                let effects = handler(e, &mut dispatcher);
                                state.apply_effects(effects, &mut ui, &mut picker_ui, &mut preview_ui, &mut overlay_ui, &mut tui);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }

            match interrupt {
                Interrupt::None => continue,
                Interrupt::Execute(_) => {
                    if controller_tx.send(Event::Pause).is_err() {
                        break
                    }
                    did_exit = true;
                    tui.enter_execute();
                    did_pause = true;
                }
                Interrupt::Reload(_) => {
                    picker_ui.worker.restart(false);
                }
                Interrupt::Become(_) => {
                    tui.exit();
                }
                _ => {}
            }

            state.update(&picker_ui);

            // Apply interrupt effect
            {
                let mut effects = Effects::new();
                let mut dispatcher = state.dispatcher(&ui, &picker_ui, preview_ui.as_ref());
                for h in dynamic_handlers.1.get(&interrupt) {
                    effects.append(
                        h(&mut dispatcher, &interrupt)
                    )
                }

                if let Interrupt::Become(context) = interrupt {
                    return Err(MatchError::Become(context));
                }
                state.apply_effects(effects, &mut ui, &mut picker_ui, &mut preview_ui, &mut overlay_ui, &mut tui);
            }
        }

        // debug!("{state:?}");

        // ------------- update state + render ------------------------
        picker_ui.update();
        // process exit conditions
        if exit_config.select_1 && picker_ui.results.status.matched_count == 1 {
            return Ok(vec![state.take_current().unwrap()]);
        }

        // resume tui
        if did_exit {
            tui.return_execute().map_err(|e| MatchError::TUIError(e.to_string()))?
        }

        let mut overlay_ui_ref = overlay_ui.as_mut();
        tui.terminal
        .draw(|frame| {
            let mut area = frame.area();

            render_ui(frame, &mut area, &ui);

            let [preview, picker_area] = if let Some(preview_ui) = preview_ui.as_mut()
            && let Some(layout) = preview_ui.layout()
            {
                let ret = layout.split(area);
                if state.iterations == 0 && ret[1].width <= 5 {
                    warn!("UI too narrow, hiding preview");
                    preview_ui.show::<false>();
                    [Rect::default(), area]
                } else {
                    ret
                }
            } else {
                [Rect::default(), area]
            };

            let [input, status, header, results, footer] = picker_ui.layout(picker_area);

            // compare and save dimensions
            did_resize = state.update_layout([preview, input, status, results]);

            if did_resize {
                picker_ui.results.update_dimensions(&results);
                // although these only want update when the whole ui change
                ui.update_dimensions(area);
                if let Some(x) = overlay_ui_ref.as_deref_mut() {
                    x.update_dimensions(&area);
                }
            };

            render_input(frame, input, &picker_ui.input);
            render_status(frame, status, &picker_ui.results);
            render_results(frame, results, &mut picker_ui);
            render_display(frame, header, &picker_ui.header, picker_ui.results.indentation());
            render_display(frame, footer, &picker_ui.footer, picker_ui.results.indentation());
            if let Some(preview_ui) = preview_ui.as_mut() {
                state.update_preview_ui(preview_ui);
                if did_resize {
                    preview_ui.update_dimensions(&preview);
                }
                render_preview(frame, preview, preview_ui);
            }
            if let Some(x) = overlay_ui_ref {
                x.draw(frame);
            }
        })
        .map_err(|e| MatchError::TUIError(e.to_string()))?;

        // useful to clear artifacts
        if did_resize && tui.config.redraw_on_resize {
            tui.redraw();
        }
        buffer.clear();

        // note: the remainder could be scoped by a conditional on having run?
        // ====== Event handling ==========
        state.update(&picker_ui);
        let events = state.events();

        // ---- Invoke handlers -------
        let mut dispatcher = state.dispatcher(&ui, &picker_ui, preview_ui.as_ref());
        // if let Some((signal, handler)) = signal_handler &&
        // let s = signal.load(std::sync::atomic::Ordering::Acquire) &&
        // s > 0
        // {
        //     handler(s, &mut dispatcher);
        //     signal.store(0, std::sync::atomic::Ordering::Release);
        // };

        // ping handlers with events
        for e in events.iter() {
            for h in dynamic_handlers.0.get(e) {
                effects.append(
                    h(&mut dispatcher, e)
                )
            }
        }

        // apply effects
        state.apply_effects(effects, &mut ui, &mut picker_ui, &mut preview_ui, &mut overlay_ui, &mut tui);

        // ------------------------------
        // send events into controller
        for e in events {
            controller_tx
            .send(e)
            .unwrap_or_else(|err| eprintln!("send failed: {:?}", err));
        }
        // =================================

        if did_pause {
            if controller_tx.send(Event::Resume).is_err() {
                break
            };
            // due to control flow, this does nothing, but is anyhow a useful safeguard to guarantee the pause
            while let Some(msg) = render_rx.recv().await {
                if matches!(msg, RenderCommand::Ack) {
                    break;
                }
            }
        }
    }

    Err(MatchError::EventLoopClosed)
}

// ------------------------- HELPERS ----------------------------
fn render_preview(frame: &mut Frame, area: Rect, ui: &mut PreviewUI) {
    // if ui.view.changed() {
    // doesn't work, use resize
    //     frame.render_widget(Clear, area);
    // } else {
    //     let widget = ui.make_preview();
    //     frame.render_widget(widget, area);
    // }
    let widget = ui.make_preview();
    frame.render_widget(widget, area);
}

fn render_results<T: SSS, S: Selection>(
    frame: &mut Frame,
    area: Rect,
    ui: &mut PickerUI<T, S>,
) {
    let widget = ui.make_table();

    frame.render_widget(widget, area);
}

fn render_input(frame: &mut Frame, area: Rect, ui: &InputUI) {
    let widget = ui.make_input();
    if let CursorSetting::Default = ui.config.cursor {
        frame.set_cursor_position(ui.cursor_offset(&area))
    };

    frame.render_widget(widget, area);
}

fn render_status(frame: &mut Frame, area: Rect, ui: &ResultsUI) {
    let widget = ui.make_status();

    frame.render_widget(widget, area);
}

fn render_display(frame: &mut Frame, area: Rect, ui: &DisplayUI, result_indentation: usize) {
    let widget = ui.make_display(result_indentation);

    frame.render_widget(widget, area);
}

fn render_ui(frame: &mut Frame, area: &mut Rect, ui: &UI) {
    let widget = ui.make_ui();
    frame.render_widget(widget, *area);
    *area = ui.inner_area(area);
}

// -----------------------------------------------------------------------------------

#[cfg(test)]
mod test {}

// #[cfg(test)]
// async fn send_every_second(tx: mpsc::UnboundedSender<RenderCommand>) {
//     let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));

//     loop {
//         interval.tick().await;
//         if tx.send(RenderCommand::quit()).is_err() {
//             break;
//         }
//     }
// }
