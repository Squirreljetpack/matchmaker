mod dynamic;
mod state;

use cba::bait::ResultExt;
use cba::{_info, unwrap};
use crossterm::event::{MouseButton, MouseEventKind};
pub use dynamic::*;
pub use state::*;
// ------------------------------

use std::io::Write;

use log::{debug, info, warn};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use tokio::sync::mpsc;

#[cfg(feature = "bracketed-paste")]
use crate::PasteHandler;
use crate::action::{Action, ActionExt};
use crate::config::{CursorSetting, ExitConfig, RowConnectionStyle};
use crate::event::{BindSender, EventSender};
use crate::message::{BindDirective, Event, Interrupt, RenderCommand};
use crate::tui::Tui;
use crate::ui::{DisplayUI, OverlayUI, PickerUI, PreviewUI, QueryUI, ResultsUI, StatusUI, UI};
use crate::{AcceptHook, ActionAliaser, ActionExtHandler, Initializer, MatchError, SSS};

fn apply_aliases<T: SSS, D, A: ActionExt>(
    buffer: &mut Vec<RenderCommand<A>>,
    aliaser: &mut ActionAliaser<T, D, A>,
    dispatcher: &mut MMState<'_, '_, T, D>,
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
pub(crate) async fn render_loop<'a, W: Write, T: SSS, D: 'static, S, A: ActionExt>(
    mut ui: UI,
    mut picker_ui: PickerUI<'a, T, D>,
    mut footer_ui: DisplayUI,
    mut preview_ui: Option<PreviewUI>,
    mut tui: Tui<W>,

    mut overlay_ui: Option<OverlayUI<A, T, D>>,
    mut exit_config: ExitConfig,

    mut render_rx: mpsc::UnboundedReceiver<RenderCommand<A>>,
    controller_tx: EventSender,
    bind_tx: BindSender<A>,

    accept_hook: AcceptHook<T, D, S>,
    mut dynamic_handlers: DynamicHandlers<T, D>,
    mut ext_handler: Option<ActionExtHandler<T, D, A>>,
    mut ext_aliaser: Option<ActionAliaser<T, D, A>>,
    initializer: Option<Initializer<T, D>>,
    #[cfg(feature = "bracketed-paste")] //
    mut paste_handler: Option<PasteHandler<T, D>>,
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
    if let Some(ref p) = preview_ui {
        state.update_preview_payload(p.get_initial_command());
    }

    let mut buffer = Vec::with_capacity(256);

    while render_rx.recv_many(&mut buffer, 256).await > 0 {
        if state.iteration == 0 {
            log::debug!("Render loop started");
        }

        // process (per-batch) exit conditions
        // exit_config.first — use get_current to seed selector with current idx, then accept.
        if exit_config.first && picker_ui.results.status.matched_count == 1 {
            picker_ui.selector.clear();
            picker_ui.results.cursor_jump(0);
            log::trace!("Exiting due to exit.first on iteration {}", state.iteration);

            tui.exit(None);
            let mut dispatcher = state.dispatcher(
                &mut ui,
                &mut picker_ui,
                &mut footer_ui,
                &mut preview_ui,
                &controller_tx,
            );
            let ret = accept_hook(&mut dispatcher);

            return Ok(ret);
        }

        let (mut did_pause, mut did_reload, mut did_exit, mut did_cursor_wrap, mut did_tick) = (
            false, false, None, false, //
            None,  // ticked(only_tick)
        );

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
        };

        let mut events = buffer.drain(..);
        while let Some(event) = events.next() {
            state.clear_interrupt();
            if state.should_quit {
                log::debug!("Exiting due to should_quit");
                tui.exit(None);
                let mut dispatcher = state.dispatcher(
                    &mut ui,
                    &mut picker_ui,
                    &mut footer_ui,
                    &mut preview_ui,
                    &controller_tx,
                );
                let ret = accept_hook(&mut dispatcher);
                return Ok(ret);
            } else if state.should_quit_nomatch {
                log::debug!("Exiting due to should_quit_nomatch");
                return Err(MatchError::NoMatch);
            }

            if !matches!(event, RenderCommand::Tick) {
                info!("Received {event:?}");
                did_tick = Some(false);
            } else if did_tick.is_none() {
                did_tick = Some(true);
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
                            if let Some(x) = overlay_ui.as_mut()
                                && x.index().is_some()
                            {
                                let mut dispatcher = state.dispatcher(
                                    &mut ui,
                                    &mut picker_ui,
                                    &mut footer_ui,
                                    &mut preview_ui,
                                    &controller_tx,
                                );
                                for c in content.chars() {
                                    x.handle_input(c, &mut dispatcher);
                                }
                            } else {
                                picker_ui.query.push_str(&content);
                            }
                        }
                    }
                }
                RenderCommand::Resize(area) => {
                    tui.resize(area);
                    ui.update_dimensions(area);
                }
                RenderCommand::Refresh => {
                    picker_ui.header.init();
                    footer_ui.init();
                    picker_ui.query.set_prompt(None);
                    picker_ui.status.set(None);
                    picker_ui.status.init();
                    picker_ui.results.set_dirty();
                }
                RenderCommand::Redraw => {
                    picker_ui.results.invalidate_widths();
                    tui.flush();
                }
                RenderCommand::HeaderTable(columns) => {
                    picker_ui.header.header_table(columns);
                }
                RenderCommand::Mouse(mouse) => {
                    use crate::config::Side;
                    // we could also impl this in the aliasing step
                    let pos = Position::from((mouse.column, mouse.row));
                    let layout = state.layout;

                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            if let Some(p) = preview_ui.as_mut()
                                && p.visible()
                                && let drag_width = p.drag_width()
                                && drag_width > 0
                                && let Some(side) = p.setting().map(|s| &s.layout.side)
                                && match side {
                                    Side::Right => {
                                        let drag_area = Rect {
                                            x: layout.preview.x,
                                            y: layout.preview.y,
                                            width: drag_width,
                                            height: layout.preview.height,
                                        };
                                        drag_area.contains(pos)
                                    }
                                    Side::Left => {
                                        let drag_area = Rect {
                                            x: layout.preview.x
                                                + layout.preview.width.saturating_sub(drag_width),
                                            y: layout.preview.y,
                                            width: drag_width,
                                            height: layout.preview.height,
                                        };
                                        drag_area.contains(pos)
                                    }
                                    Side::Bottom => {
                                        let drag_area = Rect {
                                            x: layout.preview.x,
                                            y: layout.preview.y,
                                            width: layout.preview.width,
                                            height: drag_width,
                                        };
                                        drag_area.contains(pos)
                                    }
                                    Side::Top => {
                                        let drag_area = Rect {
                                            x: layout.preview.x,
                                            y: layout.preview.y
                                                + layout.preview.height.saturating_sub(drag_width),
                                            width: layout.preview.width,
                                            height: drag_width,
                                        };
                                        drag_area.contains(pos)
                                    }
                                }
                            {
                                state.dragging = Some(Err(pos));
                                _info!(state.dragging);
                            } else if layout.results.contains(pos) {
                                let relative_x = pos.x.saturating_sub(layout.results.x);
                                if let Some(idx) = picker_ui.results.get_gutter_col_idx(relative_x)
                                {
                                    _info!(state.dragging);
                                    state.dragging = Some(Ok((pos, idx)));
                                } else {
                                    let y = mouse.row - layout.results.top();
                                    debug!("Results clicked at: {y}");
                                    click = Click::ResultPos(y);
                                }
                            } else if layout.input.contains(pos) {
                                // The X offset of the start of the visible text relative to the terminal
                                let text_start_x = layout.input.x + picker_ui.query.left();

                                if pos.x >= text_start_x {
                                    let visual_offset = pos.x - text_start_x;
                                    picker_ui.query.set_at_visual_offset(visual_offset);
                                } else {
                                    picker_ui.query.set(None, 0);
                                }
                            } else if layout.status.contains(pos) {
                                let x = pos.x.saturating_sub(layout.status.x);
                                debug!("Status clicked at x: {x}");
                                if let Some(action) = find_interaction(
                                    &picker_ui.status.status_config.interactions,
                                    x,
                                ) {
                                    click = Click::Semantic(action);
                                }
                            } else if layout.header.contains(pos) {
                                let rel_x = pos.x.saturating_sub(layout.header.x);
                                let rel_y = pos.y.saturating_sub(layout.header.y);
                                debug!("Header clicked at x: {rel_x}, y: {rel_y}");

                                if let Some(setting) =
                                    picker_ui.header.config.interactions.get(rel_y as usize)
                                    && let Some(action) = find_interaction(setting, rel_x)
                                {
                                    click = Click::Semantic(action);
                                }
                            } else if layout.footer.contains(pos) {
                                let rel_x = pos.x.saturating_sub(layout.footer.x);
                                let rel_y = pos.y.saturating_sub(layout.footer.y);
                                debug!("Footer clicked at x: {rel_x}, y: {rel_y}");

                                if let Some(setting) =
                                    footer_ui.config.interactions.get(rel_y as usize)
                                    && let Some(action) = find_interaction(setting, rel_x)
                                {
                                    click = Click::Semantic(action);
                                }
                            }
                        }
                        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                            if layout.preview.contains(pos) {
                                if let Some(p) = preview_ui.as_mut() {
                                    if matches!(mouse.kind, MouseEventKind::ScrollDown) {
                                        p.down(1)
                                    } else {
                                        p.up(1)
                                    }
                                }
                            } else {
                                let next = matches!(mouse.kind, MouseEventKind::ScrollDown)
                                    ^ picker_ui.results.reverse();
                                did_cursor_wrap = if next {
                                    picker_ui.results.cursor_next()
                                } else {
                                    picker_ui.results.cursor_prev()
                                };
                            }
                        }
                        MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
                            let left = matches!(mouse.kind, MouseEventKind::ScrollLeft);
                            if layout.preview.contains(pos) {
                                if let Some(p) = preview_ui.as_mut() {
                                    p.scroll(true, if left { -1 } else { 1 })
                                }
                            } else {
                                if !left
                                    || picker_ui.results.hscroll > 0
                                    || !picker_ui.query.is_empty()
                                {
                                    picker_ui
                                        .results
                                        .current_scroll(if left { -1 } else { 1 }, true);
                                }
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some(Err(start_pos)) = &mut state.dragging
                                && let Some(p) = preview_ui.as_mut()
                            {
                                let side =
                                    p.setting().map(|s| &s.layout.side).unwrap_or(&Side::Right);
                                match side {
                                    Side::Right => {
                                        if pos.x < start_pos.x {
                                            p.expand(start_pos.x - pos.x);
                                        } else if pos.x > start_pos.x {
                                            p.shrink(pos.x - start_pos.x);
                                        }
                                    }
                                    Side::Left => {
                                        if pos.x > start_pos.x {
                                            p.expand(pos.x - start_pos.x);
                                        } else if pos.x < start_pos.x {
                                            p.shrink(start_pos.x - pos.x);
                                        }
                                    }
                                    Side::Bottom => {
                                        if pos.y < start_pos.y {
                                            p.expand(start_pos.y - pos.y);
                                        } else if pos.y > start_pos.y {
                                            p.shrink(pos.y - start_pos.y);
                                        }
                                    }
                                    Side::Top => {
                                        if pos.y > start_pos.y {
                                            p.expand(pos.y - start_pos.y);
                                        } else if pos.y < start_pos.y {
                                            p.shrink(start_pos.y - pos.y);
                                        }
                                    }
                                }
                                *start_pos = pos;
                            } else if let Some(Ok((start_pos, stored_column))) = &mut state.dragging
                            {
                                if pos.x > start_pos.x {
                                    picker_ui
                                        .results
                                        .resize_col((pos.x - start_pos.x) as i16, *stored_column);
                                } else if pos.x < start_pos.x {
                                    picker_ui.results.resize_col(
                                        -((start_pos.x - pos.x) as i16),
                                        *stored_column,
                                    );
                                }
                                *start_pos = pos;
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            state.dragging = None;
                        }
                        _ => {}
                    }
                }
                RenderCommand::NoMatch => {
                    return Err(MatchError::NoMatch);
                }
                RenderCommand::Empty => {
                    return Ok(vec![]);
                }
                RenderCommand::Action(action) => {
                    if let Some(x) = overlay_ui.as_mut()
                        && match action {
                            Action::Char(c) => x.handle_input(
                                c,
                                &mut state.dispatcher(
                                    &mut ui,
                                    &mut picker_ui,
                                    &mut footer_ui,
                                    &mut preview_ui,
                                    &controller_tx,
                                ),
                            ),
                            _ => x.handle_action(
                                &action,
                                &mut state.dispatcher(
                                    &mut ui,
                                    &mut picker_ui,
                                    &mut footer_ui,
                                    &mut preview_ui,
                                    &controller_tx,
                                ),
                            ),
                        }
                    {
                        continue;
                    }
                    let PickerUI {
                        query,
                        results,
                        worker,
                        selector,
                        filtering,
                        ..
                    } = &mut picker_ui;
                    match action {
                        Action::Select => {
                            if let Some((idx, _)) = worker.get_nth_indexed(results.index()) {
                                results.changed[0] = true;
                                selector.insert(idx);
                            }
                        }
                        Action::Deselect => {
                            if let Some((idx, _)) = worker.get_nth_indexed(results.index()) {
                                results.changed[0] = true;
                                selector.shift_remove(&idx);
                            }
                        }
                        Action::ToggleSelection => {
                            if let Some((idx, _)) = worker.get_nth_indexed(results.index()) {
                                results.changed[0] = true;
                                if selector.contains(&idx) {
                                    selector.shift_remove(&idx);
                                } else {
                                    selector.insert(idx);
                                }
                            }
                        }
                        Action::CycleSelections => {
                            results.changed[0] = true;
                            selector.cycle_all_bg(worker.matched_indices());
                        }
                        Action::ClearSelections => {
                            results.changed[0] = true;
                            selector.clear();
                        }
                        Action::Accept => {
                            if selector.is_empty()
                                && worker.get_nth(results.index()).is_none()
                                && !exit_config.allow_empty
                            {
                                continue;
                            };
                            tui.exit(None);
                            let mut dispatcher = state.dispatcher(
                                &mut ui,
                                &mut picker_ui,
                                &mut footer_ui,
                                &mut preview_ui,
                                &controller_tx,
                            );
                            let ret = accept_hook(&mut dispatcher);
                            return Ok(ret);
                        }
                        Action::Quit(code) => {
                            return Err(MatchError::Abort(code));
                        }

                        // Results
                        Action::ToggleWrap => {
                            results.wrap(!results.is_wrap());
                        }
                        Action::ToggleHeaderWrap => {
                            picker_ui.header.wrap(!picker_ui.header.is_wrap());
                        }
                        Action::Up(x) | Action::Down(x) => {
                            let next = matches!(action, Action::Down(_)) ^ results.reverse();
                            for _ in 0..x.into() {
                                did_cursor_wrap = if next {
                                    results.cursor_next()
                                } else {
                                    results.cursor_prev()
                                };
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
                                (query.len() as u16).saturating_sub((-pos) as u16)
                            };
                            query.set(None, pos);
                        }
                        Action::HScroll(n) | Action::VScroll(n) => {
                            if let Some(p) = &mut preview_ui
                                && !p.config.wrap
                                && false
                            // track mouse location?
                            {
                                p.scroll(true, n);
                            } else if !matches!(action, Action::HScroll(_))
                                || n >= 0
                                || results.hscroll > 0
                                || !query.is_empty()
                            {
                                results.current_scroll(n, matches!(action, Action::HScroll(_)));
                            }
                        }
                        Action::HalfPageDown | Action::HalfPageUp => {
                            let x = results.height().div_ceil(2);
                            let next = matches!(action, Action::HalfPageDown) ^ results.reverse();
                            for _ in 0..x.into() {
                                did_cursor_wrap = if next {
                                    results.cursor_next()
                                } else {
                                    results.cursor_prev()
                                };
                            }
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
                        Action::ExpandPreview(n) => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.expand(n)
                            }
                        }
                        Action::ShrinkPreview(n) => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.shrink(n)
                            }
                        }
                        Action::PreviewHalfPageUp | Action::PreviewHalfPageDown => {
                            if let Some(p) = preview_ui.as_mut() {
                                let n = p.area.height.div_ceil(2);

                                if matches!(action, Action::PreviewHalfPageUp) {
                                    p.up(n)
                                } else {
                                    p.down(n)
                                }
                            }
                        }

                        Action::PreviewHScroll(x) | Action::PreviewScroll(x) => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.scroll(matches!(action, Action::PreviewHScroll(_)), x);
                            }
                        }
                        Action::PreviewJump => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.jump()
                            }
                        }

                        // Preview
                        Action::NextPreview | Action::PrevPreview => {
                            if let Some(p) = preview_ui.as_mut() {
                                p.cycle_layout(matches!(action, Action::PrevPreview));
                                if !p.command().is_empty() {
                                    state.update_preview_payload(p.command());
                                }
                            }
                        }

                        Action::Preview(context) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if !state.update_preview_payload(context.as_str()) {
                                    p.toggle_show()
                                } else {
                                    p.show(true);
                                }
                            };
                        }
                        Action::Help(context) => {
                            if let Some(p) = preview_ui.as_mut() {
                                // empty payload signifies help
                                if !state.update_preview_set(Err(context.into())) {
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
                                    state.update_preview_payload(p.command());
                                }
                            }
                        }
                        Action::SwitchPreview(idx) => {
                            if let Some(p) = preview_ui.as_mut() {
                                if let Some(idx) = idx {
                                    if !p.set_layout(idx)
                                        && !state.update_preview_payload(p.command())
                                    {
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
                        Action::ExecuteAsync(ref payload) | Action::ExecuteThen(ref payload) => {
                            let is_async = matches!(action, Action::ExecuteAsync(_));
                            let payload = payload.clone();

                            let mut remainder = crate::action::Actions::default();
                            for cmd in events.by_ref() {
                                if let RenderCommand::Action(a) = cmd {
                                    remainder.push(a);
                                }
                            }

                            if let Some(id) = state.stash_actions(remainder, bind_tx.clone()) {
                                state.set_interrupt(Interrupt::ExecuteAsync, payload);
                                state.discriminant_payload =
                                    Some(2 * id + (if is_async { 0 } else { 1 }));
                            } else {
                                log::error!("No free slots left: remaining actions dropped");
                            }
                        }
                        Action::ExecuteSilent(payload) => {
                            state.set_interrupt(Interrupt::ExecuteSilent, payload);
                        }
                        Action::Store(payload) => {
                            state.envs.set("MM_STORE", payload);
                        }
                        Action::Become(payload) => {
                            state.set_interrupt(Interrupt::Become, payload);
                        }
                        Action::BecomeSilent(payload) => {
                            state.set_interrupt(Interrupt::BecomeSilent, payload);
                        }
                        Action::Reload(payload) => {
                            state.set_interrupt(Interrupt::Reload, payload);
                        }
                        Action::Print(payload) => {
                            state.set_interrupt(Interrupt::Print, payload);
                        }

                        // Columns
                        Action::SwitchColumn(col_name) => {
                            if !*filtering {
                                continue;
                            }

                            if worker.query.active_column_name(query.str_at_cursor()) != col_name
                                && worker.columns.iter().any(|c| *c.name == col_name)
                            {
                                query.prepare_column_change();
                                query.push_str(&format!("%{} ", col_name));
                            } else {
                                log::warn!("Column {} not found in worker columns", col_name);
                            }
                        }
                        Action::NextColumn | Action::PrevColumn => {
                            if !picker_ui.filtering {
                                continue;
                            }
                            let active_idx = picker_ui.active_column_index_raw();

                            let num_columns = picker_ui.worker.columns.len();
                            if num_columns > 0 {
                                picker_ui.query.prepare_column_change();

                                let next_idx = match action {
                                    Action::NextColumn => picker_ui
                                        .results
                                        .hidden_cols()
                                        .next_gap_wrapping(active_idx + 1),
                                    Action::PrevColumn => picker_ui
                                        .results
                                        .hidden_cols()
                                        .prev_gap_wrapping(active_idx)
                                        .unwrap_or(active_idx),
                                    _ => unreachable!(),
                                };

                                let col_name = &picker_ui.worker.columns[next_idx].name;
                                picker_ui.query.push_str(&format!("%{} ", col_name));
                            }
                        }
                        // todo: take (Option) strings for both hide/unhide/expand/shrink, check if string is int treat as int, otherwise search for name
                        // todo: instead of clearing preferred_width, simply
                        Action::HideColumn(col_name) => {
                            let idx = if let Some(name) = col_name {
                                unwrap!(worker.columns.iter().position(|c| *c.name == name); continue)
                            } else {
                                picker_ui.active_column_index()
                            };

                            if idx == picker_ui.worker.query.primary_column_index() {
                                log::error!("Cannot hide default column");
                                continue;
                            } else {
                                while picker_ui.active_column_index_raw() == idx {
                                    let last = picker_ui.query.input();
                                    picker_ui.query.prepare_column_change();
                                    if picker_ui.query.input() == last {
                                        picker_ui.query.clear();
                                        break;
                                    }
                                }
                            }

                            log::info!("Hiding col: {idx}");

                            picker_ui.results.hc_set(idx);
                        }

                        Action::UnhideColumn(col_name) => {
                            if let Some(name) = col_name {
                                let idx = unwrap!(worker.columns.iter().position(|c| *c.name == name); continue);
                                results.hc_unset(idx);
                            } else {
                                results.hc_pop();
                            }
                        }
                        Action::ExpandColumn(ref col_idx) | Action::ShrinkColumn(ref col_idx) => {
                            let delta: i16 = if matches!(action, Action::ExpandColumn(_)) {
                                1
                            } else {
                                -1
                            };

                            // `col_idx` is already in the non-hidden columns
                            // space and applies directly. None means "act on
                            // the active column", which requires a lookup in
                            // the all-columns space and then translation.
                            let v_idx = if let Some(idx) = col_idx {
                                Some(*idx)
                            } else {
                                let idx = picker_ui.active_column_index();
                                picker_ui.results.shrink_idx(idx)
                            };

                            if let Some(v) = v_idx {
                                picker_ui.results.resize_col(delta, v);
                            }
                        }

                        // Edit
                        Action::SetQuery(context) => {
                            query.set(context, u16::MAX);
                        }
                        Action::InsertQuery(context) => {
                            query.insert_str(&context);
                        }
                        Action::ForwardChar => query.forward_char(),
                        Action::BackwardChar => query.backward_char(),
                        Action::ForwardWord => query.forward_word(),
                        Action::BackwardWord => query.backward_word(),
                        Action::DeleteChar => query.delete(),
                        Action::DeleteWord => query.delete_word(),
                        Action::DeleteLineStart => query.delete_line_start(),
                        Action::DeleteLineEnd => query.delete_line_end(),
                        Action::ClearQuery => query.clear(),

                        // Other
                        Action::Redraw => {
                            tui.flush();
                        }
                        Action::ToggleExitFirst(x) => {
                            exit_config.first = match x {
                                None => !exit_config.first,
                                Some(x) => x,
                            }
                        }
                        Action::Overlay(index) => {
                            if let Some(x) = overlay_ui.as_mut() {
                                let area = ui.area();
                                x.enable(
                                    index,
                                    &area,
                                    &mut state.dispatcher(
                                        &mut ui,
                                        &mut picker_ui,
                                        &mut footer_ui,
                                        &mut preview_ui,
                                        &controller_tx,
                                    ),
                                );
                                tui.flush();
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
                        Action::Char(c) => picker_ui.query.push_char(c),

                        // unreachable
                        Action::PrintKey => {}
                        Action::Semantic(_) => {}
                        Action::Trace(_) => {}
                    }
                }
                _ => {}
            }

            let interrupt = state.interrupt();

            match interrupt {
                Interrupt::None => continue,
                Interrupt::Execute => {
                    // because of this, we don't want to send controller events until after resuming at batch end
                    if controller_tx.send(Event::Pause).is_err() {
                        break;
                    }
                    tui.enter_execute();
                    if did_exit.is_none() {
                        did_exit = Some(true);
                    }
                    did_pause = true;
                }
                Interrupt::Reload => {
                    picker_ui.restart();
                    state.synced = [false, false, true];
                    did_reload = true;
                }
                Interrupt::Become => {
                    tui.exit(None);
                }
                Interrupt::BecomeSilent => {
                    tui.exit(None);
                    // tui.exit_lite();
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
                for h in dynamic_handlers.1.get_mut(interrupt) {
                    h(&mut dispatcher);
                }

                if matches!(interrupt, Interrupt::Become) {
                    return Err(MatchError::Become(state.payload().clone()));
                }
            }
        }

        // debug!("{state:?}");

        // ------------- update state + render ------------------------
        picker_ui.update();
        if did_cursor_wrap {
            log::trace!("cursor wrapped"); // todo: event handler?
        }

        // resume tui
        if let Some(clear) = did_exit {
            tui.return_execute(clear)
                .map_err(|e| MatchError::TUIError(e.to_string()))?;
            tui.flush();
        }

        #[allow(unused)]
        let mut cursor_y_offset = 0;

        // 2. Compute layout geometry & update state outside the draw closure
        let layout = update_layout_and_state(
            tui.area,
            &mut state,
            &mut picker_ui,
            &mut footer_ui,
            preview_ui.as_mut(),
            &mut ui,
            overlay_ui.as_mut(),
        );

        if state.update_input(&picker_ui.query.input()) {
            picker_ui.results.set_dirty();
            if picker_ui.query.config.reset_cursor_on_query_change {
                picker_ui.results.cursor_jump(0);
            }
            state.insert(Event::QueryChange)
        }

        picker_ui.results.update_table(
            &mut picker_ui.worker,
            &picker_ui.selector,
            picker_ui.matcher,
        );

        state.update(&mut picker_ui, &overlay_ui);

        if did_tick.is_some() {
            // 3. Pure rendering phase
            tui.terminal
                .draw(|frame| {
                    let area = frame.area();

                    render_ui(frame, area, &ui);

                    cursor_y_offset = render_input(frame, layout.input, &mut picker_ui.query).y;

                    render_status(
                        frame,
                        layout.status,
                        &picker_ui.status,
                        &picker_ui.results,
                        ui.area().width,
                    );

                    render_results(frame, layout.results, &picker_ui);
                    render_display(
                        frame,
                        layout.header,
                        &mut picker_ui.header,
                        &picker_ui.results,
                    );
                    render_display(frame, layout.footer, &mut footer_ui, &picker_ui.results);

                    if let Some(preview_ui) = preview_ui.as_mut() {
                        if preview_ui.visible() {
                            render_preview(frame, layout.preview, preview_ui);
                        }
                    }

                    if let Some(x) = overlay_ui.as_mut() {
                        x.draw(frame);
                    }
                })
                .map_err(|e| MatchError::TUIError(e.to_string()))?;
        }

        drop(events);
        buffer.clear();

        // note: the remainder could be scoped by a conditional on having run?
        // ====== Event handling ==========
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
        for h in dynamic_handlers.0.try_all(events) {
            h(&mut dispatcher, &events)
        }

        // send events into event loop controller
        if did_tick != Some(true) || !events.is_empty() {
            controller_tx.send(events)._elog();
        }

        state.reset();

        // =================================

        if did_pause {
            log::debug!("Waiting for ack response to pause");
            if controller_tx.send(Event::Resume).is_err() {
                break;
            };
            // due to control flow, this does nothing, but is anyhow a useful safeguard to guarantee the pause
            while let Some(msg) = render_rx.recv().await {
                if matches!(msg, RenderCommand::Ack) {
                    log::debug!("Received ack response to pause");
                    break;
                }
            }
        }
        if did_reload {
            controller_tx.send(Event::Reloaded)._elog();
        }

        click.process(&mut picker_ui.results, &mut buffer, &bind_tx);
    }

    Err(MatchError::EventLoopClosed)
}

// ------------------------- HELPERS ----------------------------

pub enum Click {
    None,
    ResultPos(u16),
    Semantic(String),
}

impl Click {
    fn process<A: ActionExt>(
        &mut self,
        results: &mut ResultsUI,
        _buffer: &mut Vec<RenderCommand<A>>,
        bind_tx: &BindSender<A>,
    ) {
        match self {
            Click::ResultPos(y) => {
                if let Some(idx) = results.get_index_of_row(*y) {
                    results.cursor_jump(idx);
                }
            }
            Click::Semantic(s) => {
                bind_tx
                    .send(BindDirective::Action(Action::Semantic(s.clone())))
                    ._elog();
                log::debug!("Click triggered: @{s}");
            }
            _ => {}
        }
        *self = Click::None
    }
}

fn find_interaction(setting: &crate::config::InteractionRegionSetting, x: u16) -> Option<String> {
    setting
        .iter()
        .rev()
        .find(|(start, _)| x >= *start as u16)
        .map(|(_, action)| action.clone())
        .filter(|a| !a.is_empty())
}

fn render_preview(frame: &mut Frame, area: Rect, ui: &mut PreviewUI) {
    // if ui.view.changed() {
    // doesn't work, use resize
    //     frame.render_widget(Clear, area);
    // } else {
    //     let widget = ui.make_preview();
    //     frame.render_widget(widget, area);
    // }
    assert!(ui.visible()); // don't call if not visible.
    let widget = ui.make_preview();
    frame.render_widget(widget, area);
}

fn render_results<T: SSS, D: 'static>(
    frame: &mut Frame,
    mut area: Rect,
    picker_ui: &PickerUI<T, D>,
) {
    let cap = matches!(
        picker_ui.results.config.row_connection,
        RowConnectionStyle::Capped
    );

    let (table, width) = picker_ui.results.get_table();

    if cap {
        area.width = area.width.min(width);
    }

    frame.render_widget(table, area);
}

/// Returns the offset of the cursor against the drawing area
fn render_input(frame: &mut Frame, area: Rect, ui: &mut QueryUI) -> Position {
    let widget = ui.make_input();
    let p = ui.cursor_offset(&area);
    if let CursorSetting::Default = ui.config.cursor {
        frame.set_cursor_position(p)
    };

    frame.render_widget(widget, area);

    p
}

fn render_status(
    frame: &mut Frame,
    area: Rect,
    ui: &StatusUI,
    results_ui: &ResultsUI,
    full_width: u16,
) {
    if ui.status_config.show {
        let widget = ui.make_status(results_ui, full_width);
        frame.render_widget(widget, area);
    }
}

fn render_display(frame: &mut Frame, area: Rect, ui: &mut DisplayUI, results_ui: &ResultsUI) {
    if !ui.show {
        return;
    }
    let widths = results_ui.width_limits().to_vec();

    let widget = ui.make_display((
        results_ui.indentation() as u16 + results_ui.config.border.left(),
        results_ui.config.column_spacing.0,
        widths,
    ));

    frame.render_widget(widget, area);

    if ui.is_single_column() {
        let widget = ui.make_full_width_row(results_ui.indentation() as u16);
        frame.render_widget(widget, area);
    }
}

fn render_ui(frame: &mut Frame, area: Rect, ui: &UI) {
    // outer container border, drawn over the whole terminal area
    frame.render_widget(ui.make_ui(), area);
    // picker pane border, drawn over the picker pane (including its border area)
    frame.render_widget(ui.border().as_block(), ui.picker_area());
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

fn update_layout_and_state<T: SSS, D: 'static, A: ActionExt>(
    area: Rect,
    state: &mut State,
    picker_ui: &mut PickerUI<T, D>,
    footer_ui: &mut DisplayUI,
    mut preview_ui: Option<&mut PreviewUI>,
    ui: &mut UI,
    overlay_ui: Option<&mut OverlayUI<A, T, D>>,
) -> Layout {
    // Calculate layout areas
    let full_width_footer =
        footer_ui.is_single_column() && footer_ui.config.row_connection == RowConnectionStyle::Full;

    // The layout sits inside the outer border; the picker pane is additionally
    // inset by the picker border.
    let mut _area = ui.outer_border().inner_of(area);

    let mut footer = if full_width_footer || preview_ui.as_ref().is_none_or(|p| !p.visible()) {
        split(&mut _area, footer_ui.height(), picker_ui.reverse())
    } else {
        Rect::default()
    };

    let [preview, picker, picker_area, footer] = if let Some(preview_ui) = preview_ui.as_mut()
        && preview_ui.visible()
    {
        let [preview, picker] = preview_ui.split(_area);
        let mut picker_area = ui.border().inner_of(picker);

        let hide_preview = if preview_ui.is_vertical() {
            picker_area.width <= crate::ui::RESULTS_MIN_W
        } else {
            picker_area.height <= crate::ui::RESULTS_MIN_H
        };

        if hide_preview {
            warn!("UI too small, hiding preview");
            preview_ui.show(false);
            [Rect::default(), _area, ui.border().inner_of(_area), footer]
        } else {
            if !full_width_footer {
                footer = split(&mut picker_area, footer_ui.height(), picker_ui.reverse());
            }
            [preview, picker, picker_area, footer]
        }
    } else {
        [Rect::default(), _area, ui.border().inner_of(_area), footer]
    };

    let [input, status, mut header, mut results] = picker_ui.layout(picker_area);
    let mut footer = footer;

    if results.height <= crate::ui::RESULTS_MIN_H {
        let mut needed = crate::ui::RESULTS_MIN_H.saturating_sub(results.height);
        if needed > 0 && footer.height > 0 {
            let take = needed.min(footer.height);
            footer.height -= take;
            if !picker_ui.reverse() {
                footer.y += take;
            }
            results.height += take;
            needed -= take;
        }
        if needed > 0 && header.height > 0 {
            let take = needed.min(header.height);
            header.height -= take;
            if !picker_ui.reverse() {
                results.y -= take;
            } else {
                header.y += take;
            }
            results.height += take;
        }
    }

    let layout = Layout {
        preview,
        input,
        status,
        header,
        results,
        footer,
    };

    ui.update_picker_area(picker);

    // Update state layout
    if state.update_layout(layout) {
        picker_ui.results.update_dimensions(results);
        picker_ui.query.update_width(input.width);
        footer_ui.update_width(
            if footer_ui.config.row_connection == RowConnectionStyle::Capped {
                area.width
            } else {
                footer.width
            },
        );
        picker_ui.header.update_width(header.width);
        ui.update_dimensions(area);

        if let Some(x) = overlay_ui {
            x.update_dimensions(&area);
        }
        if let Some(preview_ui) = preview_ui.as_mut() {
            preview_ui.update_dimensions(&preview);
        }
    }

    if let Some(preview_ui) = preview_ui.as_mut() {
        state.update_preview_visible(preview_ui);
    }

    layout
}

// -----------------------------------------------------------------------------------

/// Collects selected items in match order. Scans `snapshot.get_matched_item(n)`
/// for each n and yields (nucleo_idx, &T) for matches present in the selector.
// pub fn get_selected<'a, T: SSS, D>(picker_ui: &'a PickerUI<'_, T, D>) -> Vec<(u32, &'a T)> {
//     let snapshot = picker_ui.worker.nucleo.snapshot();
//     let mc = snapshot.matched_item_count();
//     (0..mc)
//         .filter_map(|n| {
//             let item = snapshot.get_matched_item(n)?;
//             let idx = snapshot.matches().get(n as usize)?.idx;
//             picker_ui
//                 .selector
//                 .contains(&idx)
//                 .then_some((idx, item.data))
//         })
//         .collect()
// }

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        action::NullActionExt,
        config::{
            BorderSetting, DisplayConfig, PreviewConfig, PreviewLayout, PreviewSetting,
            RenderConfig, ShowCondition, Side, StringOrVec,
        },
        nucleo::Worker,
        preview::{AppendOnly, Preview},
        utils::Percentage,
    };
    use nucleo::Matcher;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        text::Text,
        widgets::Borders,
    };
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    fn rect(width: u16, height: u16) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    fn setup<'a>(
        config: RenderConfig,
        matcher: &'a mut Matcher,
    ) -> (UI, PickerUI<'a, &'static str, ()>, State, DisplayUI) {
        let worker = Worker::<&'static str, ()>::new_single_column();
        let (ui, picker) = UI::new_offline(config, matcher, worker);
        (ui, picker, State::new(), DisplayUI::default())
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_of(
        ui: &mut UI,
        picker: &mut PickerUI<'_, &'static str, ()>,
        state: &mut State,
        footer: &mut DisplayUI,
        preview: Option<&mut PreviewUI>,
        area: Rect,
    ) -> Layout {
        update_layout_and_state::<&'static str, (), NullActionExt>(area, state, picker, footer, preview, ui, None)
    }

    fn test_preview() -> PreviewUI {
        let config = PreviewConfig {
            show: ShowCondition::Bool(true),
            layout: vec![PreviewSetting {
                layout: PreviewLayout {
                    side: Side::Right,
                    percentage: Percentage::new(60),
                    min: 15,
                    max: 50,
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        PreviewUI::new(
            Preview::new(
                AppendOnly::new(),
                Arc::new(Mutex::new(Some(Text::raw("preview")))),
                Arc::new(AtomicBool::new(false)),
            ),
            config,
            [80, 24],
        )
    }

    #[test]
    fn split_cut_top() {
        let mut rect = rect(80, 24);
        let offshoot = split(&mut rect, 5, true);
        assert_eq!(offshoot, Rect { x: 0, y: 0, width: 80, height: 5 });
        assert_eq!(rect, Rect { x: 0, y: 5, width: 80, height: 19 });
    }

    #[test]
    fn split_bottom() {
        let mut rect = rect(80, 24);
        let offshoot = split(&mut rect, 5, false);
        assert_eq!(offshoot, Rect { x: 0, y: 19, width: 80, height: 5 });
        assert_eq!(rect, Rect { x: 0, y: 0, width: 80, height: 19 });
    }

    #[test]
    fn split_height_capped_at_rect_height() {
        let mut rect = rect(80, 24);
        let offshoot = split(&mut rect, 100, false);
        assert_eq!(offshoot, Rect { x: 0, y: 0, width: 80, height: 24 });
        assert_eq!(rect.height, 0);
    }

    #[test]
    fn split_zero_height() {
        let mut rect = Rect {
            x: 5,
            y: 5,
            width: 80,
            height: 24,
        };
        let offshoot = split(&mut rect, 0, false);
        assert_eq!(offshoot, Rect { x: 5, y: 29, width: 80, height: 0 });
        assert_eq!(rect, Rect { x: 5, y: 5, width: 80, height: 24 });
    }

    #[test]
    fn layout_no_preview_default() {
        let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
        let (mut ui, mut picker, mut state, mut footer) = setup(RenderConfig::default(), &mut matcher);
        let area = rect(80, 24);

        let layout = layout_of(&mut ui, &mut picker, &mut state, &mut footer, None, area);

        // without borders the picker pane is the whole area
        assert_eq!(ui.picker_area(), area);
        // default DisplayUI has no content, so the footer is a zero-height bar
        // at the bottom edge
        assert_eq!(layout.footer, Rect { x: 0, y: 24, width: 80, height: 0 });

        assert_eq!(layout.input, Rect { x: 0, y: 0, width: 80, height: 1 });
        assert_eq!(layout.status, Rect { x: 0, y: 1, width: 80, height: 1 });
        assert_eq!(layout.header, Rect { x: 0, y: 2, width: 80, height: 0 });
        assert_eq!(layout.results, Rect { x: 0, y: 2, width: 80, height: 22 });
        assert_eq!(ui.area(), area);
    }

    #[test]
    fn layout_insets_by_outer_and_picker_borders() {
        let mut config = RenderConfig::default();
        config.ui.border = BorderSetting {
            sides: Some(Borders::ALL),
            ..Default::default()
        };
        config.ui.outer_border = BorderSetting {
            sides: Some(Borders::ALL),
            ..Default::default()
        };
        let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
        let (mut ui, mut picker, mut state, mut footer) = setup(config, &mut matcher);
        let area = rect(80, 24);

        let layout = layout_of(&mut ui, &mut picker, &mut state, &mut footer, None, area);

        // the picker pane sits inside the outer border
        assert_eq!(ui.picker_area(), Rect { x: 1, y: 1, width: 78, height: 22 });
        // content sits inside the picker border
        assert_eq!(layout.input, Rect { x: 2, y: 2, width: 76, height: 1 });
        assert_eq!(layout.status, Rect { x: 2, y: 3, width: 76, height: 1 });
        assert_eq!(layout.results, Rect { x: 2, y: 4, width: 76, height: 18 });
        // the ui area is inset by the outer border only (not the picker border)
        assert_eq!(ui.area(), Rect { x: 1, y: 1, width: 78, height: 22 });
    }

    #[test]
    fn layout_with_preview() {
        let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
        let (mut ui, mut picker, mut state, mut footer) = setup(RenderConfig::default(), &mut matcher);
        let mut preview = test_preview();
        let area = rect(80, 24);

        let layout = layout_of(
            &mut ui,
            &mut picker,
            &mut state,
            &mut footer,
            Some(&mut preview),
            area,
        );

        assert!(preview.visible());
        // side Right, 60% of 80 = 48, clamped to [15, 50] (+2 border padding)
        assert_eq!(layout.preview, Rect { x: 32, y: 0, width: 48, height: 24 });
        assert_eq!(ui.picker_area(), Rect { x: 0, y: 0, width: 32, height: 24 });
        assert_eq!(layout.results, Rect { x: 0, y: 2, width: 32, height: 22 });
    }

    #[test]
    fn layout_hides_preview_when_too_small() {
        let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
        let (mut ui, mut picker, mut state, mut footer) = setup(RenderConfig::default(), &mut matcher);
        let mut preview = test_preview();
        let area = rect(10, 24);

        let layout = layout_of(
            &mut ui,
            &mut picker,
            &mut state,
            &mut footer,
            Some(&mut preview),
            area,
        );

        // the preview pane would be wider than the results area, so it is hidden
        assert!(!preview.visible());
        assert_eq!(layout.preview, Rect::default());
        assert_eq!(ui.picker_area(), area);
    }

    #[test]
    fn layout_full_width_footer_spans_outer_area() {
        let mut config = RenderConfig::default();
        config.ui.border = BorderSetting {
            sides: Some(Borders::ALL),
            ..Default::default()
        };
        let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
        let (mut ui, mut picker, mut state, _footer) = setup(config, &mut matcher);
        let mut footer = DisplayUI::new(DisplayConfig {
            content: Some(StringOrVec::String("footer".into())),
            ..Default::default()
        });
        let area = rect(80, 24);

        let layout = layout_of(
            &mut ui,
            &mut picker,
            &mut state,
            &mut footer,
            None,
            area,
        );

        // the full-width footer is split off the outer area, below the picker pane
        assert_eq!(layout.footer, Rect { x: 0, y: 23, width: 80, height: 1 });
        assert_eq!(ui.picker_area(), Rect { x: 0, y: 0, width: 80, height: 23 });
        assert_eq!(layout.input, Rect { x: 1, y: 1, width: 78, height: 1 });
    }

    #[test]
    fn render_ui_draws_outer_and_picker_borders() {
        let mut config = RenderConfig::default();
        config.ui.border = BorderSetting {
            sides: Some(Borders::ALL),
            ..Default::default()
        };
        config.ui.outer_border = BorderSetting {
            sides: Some(Borders::ALL),
            ..Default::default()
        };
        let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
        let (mut ui, _picker, _state, _footer) = setup(config, &mut matcher);

        let area = rect(80, 24);
        ui.update_picker_area(Rect { x: 1, y: 1, width: 78, height: 22 });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| render_ui(frame, area, &ui))
            .unwrap();

        let buffer = terminal.backend().buffer();
        // outer border corners at the terminal edges
        assert_eq!(buffer[(0, 0)].symbol(), "┌");
        assert_eq!(buffer[(79, 0)].symbol(), "┐");
        assert_eq!(buffer[(0, 23)].symbol(), "└");
        assert_eq!(buffer[(79, 23)].symbol(), "┘");
        assert_eq!(buffer[(40, 0)].symbol(), "─");
        assert_eq!(buffer[(0, 12)].symbol(), "│");
        // the picker border is drawn around a pane that does not cover the
        // whole area, separating it from the preview side
        ui.update_picker_area(Rect { x: 0, y: 0, width: 32, height: 24 });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| render_ui(frame, area, &ui))
            .unwrap();

        let buffer = terminal.backend().buffer();
        // outer border corners at the terminal edges
        assert_eq!(buffer[(0, 0)].symbol(), "┌");
        assert_eq!(buffer[(79, 0)].symbol(), "┐");
        assert_eq!(buffer[(0, 23)].symbol(), "└");
        assert_eq!(buffer[(79, 23)].symbol(), "┘");
        assert_eq!(buffer[(40, 0)].symbol(), "─");
        assert_eq!(buffer[(0, 12)].symbol(), "│");
        // picker border corners at the picker pane edges
        assert_eq!(buffer[(0, 0)].symbol(), "┌");
        assert_eq!(buffer[(31, 0)].symbol(), "┐");
        assert_eq!(buffer[(0, 23)].symbol(), "└");
        assert_eq!(buffer[(31, 23)].symbol(), "┘");
        // the picker border's right edge separates the panes
        assert_eq!(buffer[(31, 12)].symbol(), "│");
    }

    #[test]
    fn render_ui_without_borders_draws_nothing() {
        let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
        let (mut ui, _picker, _state, _footer) = setup(RenderConfig::default(), &mut matcher);

        let area = rect(80, 24);
        ui.update_picker_area(area);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| render_ui(frame, area, &ui))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), " ");
        assert_eq!(buffer[(79, 23)].symbol(), " ");
    }
}

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
