mod dynamic;
mod state;

pub use dynamic::*;
pub use state::*;

// ------------------------------

use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use log::{info, warn};
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc;

use crate::action::Action;
use crate::config::{CursorSetting, ExitConfig};
use crate::message::{Event, Interrupt, RenderCommand};
use crate::tui::Tui;
use crate::ui::{InputUI, PickerUI, PreviewUI, ResultsUI, UI};
use crate::{MatchError, MMItem, Selection};

#[allow(clippy::too_many_arguments)]
pub async fn render_loop<'a, W: Write, T: MMItem, S: Selection, C>(
    mut ui: UI,
    mut picker_ui: PickerUI<'a, T, S, C>,
    mut preview_ui: Option<PreviewUI>,
    mut tui: Tui<W>,
    mut render_rx: mpsc::UnboundedReceiver<RenderCommand>,
    controller_tx: mpsc::UnboundedSender<Event>,
    context: Arc<C>,
    dynamic_handlers: DynamicHandlers<T, S, C>,
    exit_config: ExitConfig,
) -> Result<Vec<S>, MatchError> {
    let mut buffer = Vec::with_capacity(256);
    let mut state: State<S, C> = State::new(context);
    if let Some(ref preview_ui) = preview_ui
    && !preview_ui.command().is_empty()
    {
        state.update_preview(preview_ui.command());
    }

    while render_rx.recv_many(&mut buffer, 256).await > 0 {
        let mut did_pause = false;
        let mut did_exit = false;

        for event in &buffer {
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
                    input.input.insert(input.cursor as usize, *c);
                    input.cursor += 1;
                }
                RenderCommand::Resize(area) => {
                    tui.resize(*area);
                    ui.area = *area;
                }
                RenderCommand::Refresh => {
                    tui.redraw();
                }
                RenderCommand::Action(action) => {
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
                            return Err(MatchError::Abort(code.into()));
                        }

                        // UI
                        Action::SetHeader(context) => {
                            if let Some(s) = context {
                                header.set(s.into());
                            } else {
                                todo!()
                            }
                        }
                        Action::SetFooter(context) => {
                            if let Some(s) = context {
                                footer.set(s.into());
                            } else {
                                todo!()
                            }
                        }

                        Action::CyclePreview => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.cycle_layout();
                                if !p.command().is_empty() {
                                    state.update_preview(p.command().as_str());
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
                                    if !p.set_idx(*idx) && !state.update_preview(p.command()) {
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
                                    p.set_idx(*idx);
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
                            interrupt = Interrupt::Execute(context.into());
                        }
                        Action::Become(context) => {
                            interrupt = Interrupt::Become(context.into());
                        }
                        Action::Reload(context) => {
                            interrupt = Interrupt::Reload(context.into());
                        }
                        Action::Print(context) => {
                            interrupt = Interrupt::Print(context.into());
                        }

                        Action::SetInput(context) => {
                            input.set_input(context.into(), u16::MAX);
                        }
                        Action::Column(context) => {
                            results.toggle_col(*context);
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
                            let pos = if *pos >= 0 {
                                *pos as u32
                            } else {
                                results.status.matched_count.saturating_sub((-*pos) as u32)
                            };
                            results.cursor_jump(pos);
                        }

                        // Experimental/Debugging
                        Action::Redraw => {
                            tui.redraw();
                        }
                        _ => {}
                    }
                }
                _ => {}
            }

            if !matches!(interrupt, Interrupt::None) {
                match interrupt {
                    Interrupt::Execute(_) => {
                        if controller_tx.send(Event::Pause).is_err() {
                            break
                        }
                        did_exit = true;
                        tui.enter_execute();
                        did_pause = true;
                        while let Some(msg) = render_rx.recv().await {
                            if matches!(msg, RenderCommand::Ack) {
                                break;
                            }
                        }
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
                let (dispatcher, mut effects) = state.dispatcher(&ui, &picker_ui, preview_ui.as_ref());

                for h in dynamic_handlers.1.get(&interrupt) {
                    dispatcher.dispatch(h, &interrupt, &mut effects);
                }

                if let Interrupt::Become(context) = interrupt {
                    return Err(MatchError::Become(context));
                }
                state.process_effects(effects);
            };
        }

        // debug!("{state:?}");

        // ------------- update state + render ------------------------
        picker_ui.update();

        if did_exit {
            tui.return_execute().map_err(|e| MatchError::TUIError(e.to_string()))?
        }

        let mut resized = false;
        tui.terminal
        .draw(|frame| {
            let area = frame.area();

            let [preview, picker_area] = if let Some(preview_ui) = preview_ui.as_mut()
            && preview_ui.is_show()
            {
                let ret = preview_ui.layout().split(area);
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



            let [input, status, results] = picker_ui.layout(picker_area);

            resized = state.update_layout([preview, input, status, results]);

            // might be more efficient to always update, but logically this feels better
            if resized {
                picker_ui.results.update_dimensions(&results);
                ui.update_dimensions(area);
            };

            render_input(frame, input, &picker_ui.input);
            render_status(frame, status, &picker_ui.results);
            render_results(frame, results, &mut picker_ui);

            if let Some(preview_ui) = preview_ui.as_mut() {
                state.update_preview_ui(preview_ui);
                if resized {
                    preview_ui.update_dimensions(&preview);
                }
                render_preview(frame, preview, preview_ui);
            }
        })
        .map_err(|e| MatchError::TUIError(e.to_string()))?;
        // usefult o clear artifacts
        if resized {
            tui.redraw();
        }

        // update state
        if state.iterations == 0 {
            state.insert(Event::Start);
        }
        state.update(&picker_ui);
        let events = state.events();
        let (dispatcher, mut effects) = state.dispatcher(&ui, &picker_ui, preview_ui.as_ref());

        // process exit conditions
        if exit_config.select_1 && dispatcher.status().matched_count == 1 {
            return Ok(vec![state.take_current().unwrap()]);
        }

        // ping handlers with events
        for e in events.iter() {
            for h in dynamic_handlers.0.get(e) {
                dispatcher.dispatch(h, e, &mut effects);
            }
        }
        // send events to controller
        for e in events {
            controller_tx
            .send(e)
            .unwrap_or_else(|err| eprintln!("send failed: {:?}", err));
        }
        // process effects
        state.process_effects(effects);

        buffer.clear();

        if did_pause {
            if controller_tx.send(Event::Resume).is_err() {
                break
            };
            // due to control flow, this does nothing, but is a useful safeguard anyway
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

fn render_results<T: MMItem, S: Selection, C>(
    frame: &mut Frame,
    area: Rect,
    ui: &mut PickerUI<T, S, C>,
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
