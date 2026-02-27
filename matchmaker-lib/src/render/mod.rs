mod dynamic;
mod state;

use crossterm::event::{MouseButton, MouseEventKind};
pub use dynamic::*;
pub use state::*;
// ------------------------------

use std::io::Write;

use log::{info, warn};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use tokio::sync::mpsc;

#[cfg(feature = "bracketed-paste")]
use crate::PasteHandler;
use crate::action::{Action, ActionExt};
use crate::config::{CursorSetting, ExitConfig, RowConnectionStyle};
use crate::event::EventSender;
use crate::message::{Event, Interrupt, RenderCommand};
use crate::tui::Tui;
use crate::ui::{DisplayUI, InputUI, OverlayUI, PickerUI, PreviewUI, ResultsUI, UI};
use crate::{ActionAliaser, ActionExtHandler, Initializer, MatchError, SSS, Selection};

fn apply_aliases<T: SSS, S: Selection, A: ActionExt>(
    buffer: &mut Vec<RenderCommand<A>>,
    aliaser: &mut ActionAliaser<T, S, A>,
    dispatcher: &mut MMState<'_, '_, T, S>,
) {
    let mut out = Vec::new();

    for cmd in buffer.drain(..) {
        match cmd {
            RenderCommand::Action(a) => out.extend(
                aliaser(a, dispatcher)
                    .into_iter()
                    .map(RenderCommand::Action),
            ),
            other => out.push(other),
        }
    }

    *buffer = out;
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn render_loop<'a, W: Write, T: SSS, S: Selection, A: ActionExt>(
    mut ui: UI,
    mut picker_ui: PickerUI<'a, T, S>,
    mut footer_ui: DisplayUI,
    mut preview_ui: Option<PreviewUI>,
    mut tui: Tui<W>,

    mut overlay_ui: Option<OverlayUI<A>>,
    exit_config: ExitConfig,

    mut render_rx: mpsc::UnboundedReceiver<RenderCommand<A>>,
    controller_tx: EventSender,

    dynamic_handlers: DynamicHandlers<T, S>,
    mut ext_handler: Option<ActionExtHandler<T, S, A>>,
    mut ext_aliaser: Option<ActionAliaser<T, S, A>>,
    initializer: Option<Initializer<T, S>>,
    #[cfg(feature = "bracketed-paste")] //
    mut paste_handler: Option<PasteHandler<T, S>>,
) -> Result<Vec<S>, MatchError> {
    let mut state = State::new();

    if let Some(handler) = initializer {
        handler(&mut state.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &controller_tx,
        ));
    }

    let mut click = Click::None;

    // place the initial command in the state where the preview listener can access
    if let Some(ref p) = preview_ui
        && p.is_show()
    {
        state.update_preview(p.get_initial_command());
    }

    let mut buffer = Vec::with_capacity(256);

    while render_rx.recv_many(&mut buffer, 256).await > 0 {
        if state.iterations == 0 {
            log::debug!("Render loop started");
        }
        let mut did_pause = false;
        let mut did_exit = false;
        let mut did_resize = false;

        // todo: why exactly can we not borrow the picker_ui mutably?
        if let Some(aliaser) = &mut ext_aliaser {
            apply_aliases(
                &mut buffer,
                aliaser,
                &mut state.dispatcher(
                    &mut ui,
                    &mut picker_ui,
                    &mut footer_ui,
                    &mut preview_ui,
                    &controller_tx,
                ),
            )
            // effects could be moved out for efficiency, but it seems more logical to add them as they come so that we can trigger interrupts
        };

        if state.should_quit {
            log::debug!("Exiting due to should_quit");
            let ret = picker_ui.selector.output().collect::<Vec<S>>();
            return if picker_ui.selector.is_disabled()
                && let Some((_, item)) = get_current(&picker_ui)
            {
                Ok(vec![item])
            } else if ret.is_empty() {
                Err(MatchError::Abort(0))
            } else {
                Ok(ret)
            };
        } else if state.should_quit_nomatch {
            log::debug!("Exiting due to should_quit_no_match");
            return Err(MatchError::NoMatch);
        }

        for event in buffer.drain(..) {
            state.clear_interrupt();

            if !matches!(event, RenderCommand::Tick) {
                info!("Recieved {event:?}");
            } else {
                // log::trace!("Recieved {event:?}");
            }

            match event {
                #[cfg(feature = "bracketed-paste")]
                RenderCommand::Paste(content) => {
                    if let Some(handler) = &mut paste_handler {
                        let content = {
                            handler(
                                content,
                                &state.dispatcher(
                                    &mut ui,
                                    &mut picker_ui,
                                    &mut footer_ui,
                                    &mut preview_ui,
                                    &controller_tx,
                                ),
                            )
                        };
                        if !content.is_empty() {
                            picker_ui.input.push_str(&content);
                        }
                    }
                }
                RenderCommand::Resize(area) => {
                    tui.resize(area);
                    ui.area = area;
                }
                RenderCommand::Refresh => {
                    tui.redraw();
                }
                RenderCommand::HeaderTable(columns) => {
                    picker_ui.header.header_table(columns);
                }
                RenderCommand::Mouse(mouse) => {
                    // we could also impl this in the aliasing step
                    let pos = Position::from((mouse.column, mouse.row));
                    let [preview, input, status, result] = state.layout;

                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            // todo: clickable column headers, clickable results, also, grouping?
                            if result.contains(pos) {
                                click = Click::ResultPos(mouse.row - result.top());
                            } else if input.contains(pos) {
                                // The X offset of the start of the visible text relative to the terminal
                                let text_start_x = input.x
                                    + picker_ui.input.prompt.width() as u16
                                    + picker_ui.input.config.border.left();

                                if pos.x >= text_start_x {
                                    let visual_offset = pos.x - text_start_x;
                                    picker_ui.input.set_at_visual_offset(visual_offset);
                                } else {
                                    picker_ui.input.set(None, 0);
                                }
                            } else if status.contains(pos) {
                                // todo
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            if preview.contains(pos) {
                                if let Some(p) = preview_ui.as_mut() {
                                    p.down(1)
                                }
                            } else {
                                picker_ui.results.cursor_next()
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if preview.contains(pos) {
                                if let Some(p) = preview_ui.as_mut() {
                                    p.up(1)
                                }
                            } else {
                                picker_ui.results.cursor_prev()
                            }
                        }
                        MouseEventKind::ScrollLeft => {
                            // todo
                        }
                        MouseEventKind::ScrollRight => {
                            // todo
                        }
                        // Drag tracking: todo
                        _ => {}
                    }
                }
                RenderCommand::QuitEmpty => {
                    return Ok(vec![]);
                }
                RenderCommand::Action(action) => {
                    if let Some(x) = overlay_ui.as_mut() {
                        if match action {
                            Action::Char(c) => x.handle_input(c),
                            _ => x.handle_action(&action),
                        } {
                            continue;
                        }
                    }
                    let PickerUI {
                        input,
                        results,
                        worker,
                        selector: selections,
                        ..
                    } = &mut picker_ui;
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
                        Action::ClearSelections => {
                            selections.clear();
                        }
                        Action::Accept => {
                            let ret = if selections.is_empty() {
                                if let Some(item) = get_current(&picker_ui) {
                                    vec![item.1]
                                } else if exit_config.allow_empty {
                                    vec![]
                                } else {
                                    continue;
                                }
                            } else {
                                selections.output().collect::<Vec<S>>()
                            };
                            return Ok(ret);
                        }
                        Action::Quit(code) => {
                            return Err(MatchError::Abort(code));
                        }

                        // Results
                        Action::ToggleWrap => {
                            results.wrap(!results.is_wrap());
                        }
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
                        Action::Pos(pos) => {
                            let pos = if pos >= 0 {
                                pos as u32
                            } else {
                                results.status.matched_count.saturating_sub((-pos) as u32)
                            };
                            results.cursor_jump(pos);
                        }
                        Action::QueryPos(pos) => {
                            let pos = if pos >= 0 {
                                pos as u16
                            } else {
                                (input.len() as u16).saturating_sub((-pos) as u16)
                            };
                            input.set(None, pos);
                        }
                        Action::HScroll(_n) => {
                            // todo
                        }
                        Action::PageDown => {
                            // todo
                        }
                        Action::PageUp => {
                            // todo
                        }

                        // Preview Navigation
                        Action::PreviewUp(n) => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.up(n)
                            }
                        }
                        Action::PreviewDown(n) => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.down(n)
                            }
                        }
                        Action::PreviewHalfPageUp => {
                            let n = (ui.area.height + 1) / 2;
                            if let Some(p) = preview_ui.as_mut() {
                                p.down(n)
                            }
                        }
                        Action::PreviewHalfPageDown => {
                            let n = (ui.area.height + 1) / 2;
                            if let Some(p) = preview_ui.as_mut() {
                                p.down(n)
                            }
                        }

                        Action::PreviewHScroll(x) | Action::PreviewScroll(x) => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.scroll(matches!(action, Action::PreviewHScroll(_)), x);
                            }
                        }
                        Action::PreviewJump => {
                            // todo
                        }

                        // Preview
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
                                    p.show(true);
                                }
                            };
                        }
                        Action::Help(context) => {
                            if let Some(p) = preview_ui.as_mut() {
                                // empty payload signifies help
                                if !state.update_preview_set(context) {
                                    state.update_preview_unset()
                                } else {
                                    p.show(true);
                                }
                            };
                        }
                        Action::SetPreview(idx) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if let Some(idx) = idx {
                                    p.set_layout(idx);
                                } else {
                                    state.update_preview(p.command());
                                }
                            }
                        }
                        Action::SwitchPreview(idx) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if let Some(idx) = idx {
                                    if !p.set_layout(idx) && !state.update_preview(p.command()) {
                                        p.toggle_show();
                                    }
                                } else {
                                    p.toggle_show()
                                }
                            }
                        }
                        Action::TogglePreviewWrap => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.wrap(!p.is_wrap());
                            }
                        }

                        // Programmable
                        Action::Execute(payload) => {
                            state.set_interrupt(Interrupt::Execute, payload);
                        }
                        Action::Become(payload) => {
                            state.set_interrupt(Interrupt::Become, payload);
                        }
                        Action::Reload(payload) => {
                            state.set_interrupt(Interrupt::Reload, payload);
                        }
                        Action::Print(payload) => {
                            state.set_interrupt(Interrupt::Print, payload);
                        }

                        // Columns
                        Action::Column(context) => {
                            results.toggle_col(context);
                        }
                        Action::CycleColumn => {
                            results.cycle_col();
                        }
                        Action::ColumnLeft => {}
                        Action::ColumnRight => {}
                        Action::ScrollLeft => {}
                        Action::ScrollRight => {}

                        // Edit
                        Action::SetQuery(context) => {
                            input.set(context, u16::MAX);
                        }
                        Action::ForwardChar => input.forward_char(),
                        Action::BackwardChar => input.backward_char(),
                        Action::ForwardWord => input.forward_word(),
                        Action::BackwardWord => input.backward_word(),
                        Action::DeleteChar => input.delete(),
                        Action::DeleteWord => input.delete_word(),
                        Action::DeleteLineStart => input.delete_line_start(),
                        Action::DeleteLineEnd => input.delete_line_end(),
                        Action::Cancel => input.cancel(),

                        // Other
                        Action::Redraw => {
                            tui.redraw();
                        }
                        Action::Overlay(index) => {
                            if let Some(x) = overlay_ui.as_mut() {
                                x.enable(index, &ui.area);
                                tui.redraw();
                            };
                        }
                        Action::Custom(e) => {
                            if let Some(handler) = &mut ext_handler {
                                handler(
                                    e,
                                    &mut state.dispatcher(
                                        &mut ui,
                                        &mut picker_ui,
                                        &mut footer_ui,
                                        &mut preview_ui,
                                        &controller_tx,
                                    ),
                                );
                            }
                        }
                        Action::Char(c) => picker_ui.input.push_char(c),
                    }
                }
                _ => {}
            }

            let interrupt = state.interrupt();

            match interrupt {
                Interrupt::None => continue,
                Interrupt::Execute => {
                    if controller_tx.send(Event::Pause).is_err() {
                        break;
                    }
                    tui.enter_execute();
                    did_exit = true;
                    did_pause = true;
                }
                Interrupt::Reload => {
                    picker_ui.worker.restart(false);
                    state.synced = [false; 2];
                }
                Interrupt::Become => {
                    tui.exit();
                }
                _ => {}
            }
            // Apply interrupt effect
            {
                let mut dispatcher = state.dispatcher(
                    &mut ui,
                    &mut picker_ui,
                    &mut footer_ui,
                    &mut preview_ui,
                    &controller_tx,
                );
                for h in dynamic_handlers.1.get(interrupt) {
                    h(&mut dispatcher);
                }

                if matches!(interrupt, Interrupt::Become) {
                    return Err(MatchError::Become(state.payload().clone()));
                }
            }

            if state.should_quit {
                log::debug!("Exiting due to should_quit");
                let ret = picker_ui.selector.output().collect::<Vec<S>>();
                return if picker_ui.selector.is_disabled()
                    && let Some((_, item)) = get_current(&picker_ui)
                {
                    Ok(vec![item])
                } else if ret.is_empty() {
                    Err(MatchError::Abort(0))
                } else {
                    Ok(ret)
                };
            } else if state.should_quit_nomatch {
                log::debug!("Exiting due to should_quit_nomatch");
                return Err(MatchError::NoMatch);
            }
        }

        // debug!("{state:?}");

        // ------------- update state + render ------------------------
        if state.filtering {
            picker_ui.update();
        } else {
            // nothing
        }
        // process exit conditions
        if exit_config.select_1
            && picker_ui.results.status.matched_count == 1
            && let Some((_, item)) = get_current(&picker_ui)
        {
            return Ok(vec![item]);
        }

        // resume tui
        if did_exit {
            tui.return_execute()
                .map_err(|e| MatchError::TUIError(e.to_string()))?;
            tui.redraw();
        }

        let mut overlay_ui_ref = overlay_ui.as_mut();
        let mut cursor_y_offset = 0;

        tui.terminal
            .draw(|frame| {
                let mut area = frame.area();

                render_ui(frame, &mut area, &ui);

                let mut _area = area;

                let full_width_footer = footer_ui.single()
                    && footer_ui.config.row_connection_style == RowConnectionStyle::Full;

                let mut footer =
                    if full_width_footer || preview_ui.as_ref().is_none_or(|p| !p.is_show()) {
                        split(&mut _area, footer_ui.height(), picker_ui.reverse())
                    } else {
                        Rect::default()
                    };

                let [preview, picker_area, footer] = if let Some(preview_ui) = preview_ui.as_mut()
                    && let Some(layout) = preview_ui.layout()
                {
                    let [preview, mut picker_area] = layout.split(_area);

                    if state.iterations == 0 && picker_area.width <= 5 {
                        warn!("UI too narrow, hiding preview");
                        preview_ui.show(false);

                        [Rect::default(), _area, footer]
                    } else {
                        if !full_width_footer {
                            footer =
                                split(&mut picker_area, footer_ui.height(), picker_ui.reverse());
                        }

                        [preview, picker_area, footer]
                    }
                } else {
                    [Rect::default(), _area, footer]
                };

                let [input, status, header, results] = picker_ui.layout(picker_area);

                // compare and save dimensions
                did_resize = state.update_layout([preview, input, status, results]);

                if did_resize {
                    picker_ui.results.update_dimensions(&results);
                    picker_ui.input.update_width(input.width);
                    footer_ui.update_width(
                        if footer_ui.config.row_connection_style == RowConnectionStyle::Capped {
                            area.width
                        } else {
                            footer.width
                        },
                    );
                    picker_ui.header.update_width(header.width);
                    // although these only want update when the whole ui change
                    ui.update_dimensions(area);
                    if let Some(x) = overlay_ui_ref.as_deref_mut() {
                        x.update_dimensions(&area);
                    }
                };

                cursor_y_offset = render_input(frame, input, &mut picker_ui.input).y;
                render_status(frame, status, &picker_ui.results, ui.area.width);
                render_results(frame, results, &mut picker_ui, &mut click);
                render_display(frame, header, &mut picker_ui.header, &picker_ui.results);
                render_display(frame, footer, &mut footer_ui, &picker_ui.results);
                if let Some(preview_ui) = preview_ui.as_mut() {
                    state.update_preview_visible(preview_ui);
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
        if did_resize && tui.config.redraw_on_resize && !did_exit {
            tui.redraw();
            tui.cursor_y_offset = Some(cursor_y_offset)
        }
        buffer.clear();

        // note: the remainder could be scoped by a conditional on having run?
        // ====== Event handling ==========
        state.update(&picker_ui, &overlay_ui);
        let events = state.events();

        // ---- Invoke handlers -------
        let mut dispatcher = state.dispatcher(
            &mut ui,
            &mut picker_ui,
            &mut footer_ui,
            &mut preview_ui,
            &controller_tx,
        );
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
                h(&mut dispatcher, &e)
            }
        }

        // ------------------------------
        // send events into controller
        for e in events.iter() {
            controller_tx
                .send(e)
                .unwrap_or_else(|err| eprintln!("send failed: {:?}", err));
        }
        // =================================

        if did_pause {
            log::debug!("Waiting for ack response to pause");
            if controller_tx.send(Event::Resume).is_err() {
                break;
            };
            // due to control flow, this does nothing, but is anyhow a useful safeguard to guarantee the pause
            while let Some(msg) = render_rx.recv().await {
                if matches!(msg, RenderCommand::Ack) {
                    log::debug!("Recieved ack response to pause");
                    break;
                }
            }
        }

        click.process(&mut buffer);
    }

    Err(MatchError::EventLoopClosed)
}

// ------------------------- HELPERS ----------------------------

pub enum Click {
    None,
    ResultPos(u16),
    ResultIdx(u32),
}

impl Click {
    fn process<A: ActionExt>(&mut self, buffer: &mut Vec<RenderCommand<A>>) {
        match self {
            Self::ResultIdx(u) => {
                buffer.push(RenderCommand::Action(Action::Pos(*u as i32)));
            }
            _ => {
                // todo
            }
        }
        *self = Click::None
    }
}

fn render_preview(frame: &mut Frame, area: Rect, ui: &mut PreviewUI) {
    // if ui.view.changed() {
    // doesn't work, use resize
    //     frame.render_widget(Clear, area);
    // } else {
    //     let widget = ui.make_preview();
    //     frame.render_widget(widget, area);
    // }
    if !ui.is_show() {
        return;
    }
    let widget = ui.make_preview();
    frame.render_widget(widget, area);
}

fn render_results<T: SSS, S: Selection>(
    frame: &mut Frame,
    mut area: Rect,
    ui: &mut PickerUI<T, S>,
    click: &mut Click,
) {
    let cap = matches!(
        ui.results.config.row_connection_style,
        RowConnectionStyle::Capped
    );
    let (widget, table_width) = ui.make_table(click);

    if cap {
        area.width = area.width.min(table_width);
    }

    frame.render_widget(widget, area);
}

/// Returns the offset of the cursor against the drawing area
fn render_input(frame: &mut Frame, area: Rect, ui: &mut InputUI) -> Position {
    ui.scroll_to_cursor();
    let widget = ui.make_input();
    let p = ui.cursor_offset(&area);
    if let CursorSetting::Default = ui.config.cursor {
        frame.set_cursor_position(p)
    };

    frame.render_widget(widget, area);

    p
}

fn render_status(frame: &mut Frame, area: Rect, ui: &ResultsUI, full_width: u16) {
    if ui.status_config.show {
        let widget = ui.make_status(full_width);
        frame.render_widget(widget, area);
    }
}

fn render_display(frame: &mut Frame, area: Rect, ui: &mut DisplayUI, results_ui: &ResultsUI) {
    if !ui.show {
        return;
    }
    let widget = ui.make_display(
        results_ui.indentation() as u16,
        results_ui.widths().to_vec(),
        results_ui.config.column_spacing.0,
    );

    frame.render_widget(widget, area);

    if ui.single() {
        let widget = ui.make_full_width_row(results_ui.indentation() as u16);
        frame.render_widget(widget, area);
    }
}

fn render_ui(frame: &mut Frame, area: &mut Rect, ui: &UI) {
    let widget = ui.make_ui();
    frame.render_widget(widget, *area);
    *area = ui.inner_area(area);
}

fn split(rect: &mut Rect, height: u16, cut_top: bool) -> Rect {
    let h = height.min(rect.height);

    if cut_top {
        let offshoot = Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: h,
        };

        rect.y += h;
        rect.height -= h;

        offshoot
    } else {
        let offshoot = Rect {
            x: rect.x,
            y: rect.y + rect.height - h,
            width: rect.width,
            height: h,
        };

        rect.height -= h;

        offshoot
    }
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
