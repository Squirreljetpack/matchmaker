use bitflags::Flags;
use cba::{bait::TransformExt, broc::EnvVars, env_vars, unwrap};
use ratatui::text::Text;

use crate::{
    SSS, Selector,
    action::{ActionExt, Actions},
    event::{self, BindSender, EventSender},
    message::{BindDirective, Event, Interrupt},
    nucleo::{Status, injector::WorkerInjector},
    ui::{DisplayUI, OverlayUI, PickerUI, PreviewUI, Rect, UI},
};
use ratatui::layout::Position;

// --------------------------------------------------------------------
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub preview: Rect,
    pub input: Rect,
    pub status: Rect,
    pub header: Rect,
    pub results: Rect,
    pub footer: Rect,
}

/// In the "standard implementation", None represents unset, String: command, Text: display
pub type PreviewSetPayload = Option<Result<String, Text<'static>>>;

pub struct State {
    last_id: Option<u32>,
    interrupt: Interrupt,
    interrupt_payload: String,

    // Stores "last" state to emit events on change
    pub(crate) input: String,
    pub(crate) iteration: u32,
    pub(crate) preview_visible: bool,
    pub(crate) layout: Layout,
    pub(crate) dragging: Option<Position>,
    pub(crate) dragging_column: Option<(Position, usize)>,
    pub(crate) overlay_index: Option<usize>,
    pub(crate) synced: [bool; 2], // ran, synced

    pub(crate) events: Event,

    /// The String passed to SetPreview
    pub preview_set_payload: PreviewSetPayload,
    /// The payload left by [`crate::action::Action::Preview`]
    pub preview_payload: String,
    pub envs: EnvVars,
    /// A place to stash the preview visibility when overriding it
    stashed_preview_visibility: Option<bool>,
    /// Setting this to true finishes the picker with the contents of [`Selector`].
    /// If [`Selector`] is disabled, the picker finishes with the current item.
    /// If there are no items to finish with, the picker finishes with [`crate::errors::MatchError::Abort`]\(0).
    /// Note: this bypasses the accept hook.
    pub should_quit: bool,
    /// Setting this to true finishes the picker with [`crate::MatchError::NoMatch`].
    pub should_quit_nomatch: bool,
    pub filtering: bool,

    /// This field is never touched by the rendering loop and is reserved for
    /// callers to use to store values, such as distinguishing between multiple
    /// sources of a payload for an interrupt. The responsibility is on the caller
    /// to ensure the value is emptied by the handler corresponding to an interrupt.
    /// Update: This field is set by the rendering loop for ExecuteAsync and ExecuteThen. See [`crate::Matchmaker::_register_execute_handler`], which registers a handler that immediately consumes it.
    ///
    /// # Discriminants
    /// - (ExecuteAsync, 0): Copy (Async, Normal)
    /// - (ExecuteAsync, 1): Copy (Async, OSC52)
    /// - (ExecuteAsync, 2*id): ExecuteAsync (Async, remainder)
    /// - (ExecuteAsync, 2*id + 1): ExecuteThen (Async, remainder)
    /// - (ExecuteSilent, 2): Copy (Sync, Normal)
    /// - (ExecuteSilent, 3): Copy (Sync, OSC52)
    pub discriminant_payload: Option<u8>,

    pub async_actions: [Option<Box<dyn FnOnce() + Send + Sync>>; 128],
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.async_actions.iter().filter(|x| x.is_some()).count();
        f.debug_struct("State")
            .field("last_id", &self.last_id)
            .field("interrupt", &self.interrupt)
            .field("interrupt_payload", &self.interrupt_payload)
            .field("input", &self.input)
            .field("iterations", &self.iteration)
            .field("async_actions_count", &count)
            .finish_non_exhaustive()
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        // this is the same as default
        Self {
            last_id: None,
            interrupt: Interrupt::None,
            interrupt_payload: String::new(),

            preview_payload: String::new(),
            envs: Default::default(),
            preview_set_payload: None,
            preview_visible: false,
            stashed_preview_visibility: None,
            layout: Layout::default(),
            dragging: None,
            dragging_column: None,
            overlay_index: None,

            input: String::new(),
            iteration: 0,
            synced: [false; 2],

            events: Event::empty(),
            should_quit: false,
            should_quit_nomatch: false,
            filtering: true,

            discriminant_payload: None,
            async_actions: std::array::from_fn(|_| None),
        }
    }
    // ------ properties -----------

    pub fn contains(&self, event: Event) -> bool {
        self.events.contains(event)
    }

    pub fn payload(&self) -> &String {
        &self.interrupt_payload
    }

    pub fn interrupt(&self) -> Interrupt {
        self.interrupt
    }

    pub fn set_interrupt(&mut self, interrupt: Interrupt, payload: String) {
        self.interrupt = interrupt;
        self.interrupt_payload = payload;
    }

    pub fn clear_interrupt(&mut self) {
        self.interrupt = Interrupt::None;
        self.interrupt_payload.clear();
    }

    pub fn insert(&mut self, event: Event) {
        self.events.insert(event);
    }

    pub fn overlay_index(&self) -> Option<usize> {
        self.overlay_index
    }
    pub fn preview_set_payload(&self) -> Option<Result<String, Text<'static>>> {
        self.preview_set_payload.clone()
    }
    pub fn preview_payload(&self) -> &String {
        &self.preview_payload
    }
    pub fn stashed_preview_visibility(&self) -> Option<bool> {
        self.stashed_preview_visibility
    }

    pub fn stash_actions<A: ActionExt + 'static>(
        &mut self,
        actions: Actions<A>,
        bind_tx: BindSender<A>,
    ) -> Option<u8> {
        let Some((idx, slot)) = self
            .async_actions
            .iter_mut()
            .enumerate()
            .skip(1)
            .find(|(_, x)| x.is_none())
        else {
            return None;
        };

        let closure = move || {
            for a in actions {
                let _ = bind_tx.send(BindDirective::Action(a));
            }
        };
        *slot = Some(Box::new(closure));
        Some(idx as u8)
    }

    pub fn take_actions(&mut self, id: u8) -> Option<Box<dyn FnOnce() + Send + Sync>> {
        self.async_actions
            .get_mut(id as usize)
            .and_then(|x| x.take())
    }

    // ------- updates --------------
    pub(crate) fn update_input(&mut self, new_input: &str) -> bool {
        let changed = self.input.cmp_replace(new_input.to_string());
        if changed {
            self.insert(Event::QueryChange);
        }
        changed
    }

    pub(crate) fn update_preview_payload(&mut self, context: &str) -> bool {
        let changed = self.preview_payload.cmp_replace(context.into());
        if changed {
            self.insert(Event::PreviewChange);
        }
        changed
    }

    pub fn update_preview_set(&mut self, context: Result<String, Text<'static>>) -> bool {
        let next = Some(context);
        let changed = self.preview_set_payload.cmp_replace(next);
        if changed {
            self.insert(Event::PreviewSet);
        }
        changed
    }

    pub(crate) fn update_preview_unset(&mut self) {
        let changed = self.preview_set_payload.cmp_replace(None);
        if changed {
            self.insert(Event::PreviewSet);
        }
    }

    pub(crate) fn update_layout(&mut self, new_layout: Layout) -> bool {
        let changed = self.layout.preview.width != new_layout.preview.width
            || self.layout.preview.height != new_layout.preview.height
            || self.layout.results.width != new_layout.results.width
            || self.layout.results.height != new_layout.results.height;

        self.layout = new_layout;

        if changed {
            self.insert(Event::Resize);
        }
        changed
    }

    /// Emit PreviewChange event on change to visible
    pub(crate) fn update_preview_visible(&mut self, preview_ui: &PreviewUI) -> bool {
        let visible = preview_ui.visible();
        let changed = self.preview_visible.cmp_replace(visible);
        if changed && visible {
            self.insert(Event::PreviewChange);
        }
        changed
    }

    pub(crate) fn update<'a, T: SSS, D: 'static, A: ActionExt>(
        &'a mut self,
        picker_ui: &'a mut PickerUI<T, D>,
        overlay_ui: &'a Option<OverlayUI<A>>,
    ) {
        if self.iteration == 0 {
            self.insert(Event::Start);
            self.input = picker_ui.query.input.clone();
        } else {
            if self.update_input(&picker_ui.query.input) {
                picker_ui.results.set_dirty();
            }
        }
        self.iteration += 1;

        let status = &picker_ui.results.status;
        self.synced[1] |= status.running;
        if status.changed {
            // add a synced event when worker stops running
            if !picker_ui.results.status.running {
                if !self.synced[0] {
                    // this is supposed to fire when all inputs have been loaded into nucleo although it clearly can't be race-free
                    if picker_ui.results.status.item_count > 0 {
                        self.insert(Event::Synced);
                        self.synced[0] = true;
                    }
                } else {
                    // this should be emitted every time input filter changes
                    // note that this will never emit on empty input
                    log::trace!("resynced on iteration {}", self.iteration);
                    self.insert(Event::Resynced);
                }
            }
        }

        if let Some(o) = overlay_ui {
            if self.overlay_index != o.index() {
                self.insert(Event::OverlayChange);
                self.overlay_index = o.index()
            }
            self.overlay_index = o.index()
        }

        let new_id = picker_ui.current_indexed().map(|x| x.0);
        let changed = self.last_id != picker_ui.current_indexed().map(|x| x.0);
        if changed {
            self.last_id = new_id;
            self.insert(Event::CursorChange);

            if self.last_id.is_none() {
                self.insert(Event::CursorLost);
            }
        }
        // log::trace!("{self:?}");
    }

    // ---------- flush -----------
    // public for tests only!
    pub fn dispatcher<'a, 'b: 'a, T: SSS, D>(
        &'a mut self,
        ui: &'a mut UI,
        picker_ui: &'a mut PickerUI<'b, T, D>,
        footer_ui: &'a mut DisplayUI,
        preview_ui: &'a mut Option<PreviewUI>,
        event_controller: &'a EventSender,
    ) -> MMState<'a, 'b, T, D> {
        MMState {
            state: self,
            ui,
            picker_ui,
            footer_ui,
            preview_ui,
            event_controller,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.events.clear();
    }

    pub fn events(&mut self) -> Event {
        self.events
    }
}

// ----------------------------------------------------------------------
pub struct MMState<'a, 'b: 'a, T: SSS, D> {
    // access through deref/mut
    pub(crate) state: &'a mut State,

    pub ui: &'a mut UI,
    pub picker_ui: &'a mut PickerUI<'b, T, D>,
    pub footer_ui: &'a mut DisplayUI,
    pub preview_ui: &'a mut Option<PreviewUI>,
    pub event_controller: &'a EventSender,
}

impl<'a, 'b: 'a, T: SSS, D: 'static> MMState<'a, 'b, T, D> {
    pub fn previewer_area(&self) -> Option<Rect> {
        self.preview_ui.as_ref().map(|ui| {
            let mut ret = ui.area;
            if let Some(o) = ui.current_dimension {
                if ui.is_vertical() {
                    ret.height = o
                } else {
                    ret.width = o
                }
            }
            ret
        })
    }

    pub fn tui_area(&self) -> Rect {
        self.ui.full_area()
    }
    pub fn ui_size(&self) -> [u16; 2] {
        let q = &self.ui.area();
        [q.width, q.height]
    }

    /// Returns the nucleo index of the currently highlighted item, if any.
    pub fn current_index(&self) -> Option<u32> {
        self.picker_ui.current_indexed().map(|x| x.0)
    }

    /// Same as `current_index`, but returns a reference to the underlying item.
    pub fn current_raw(&self) -> Option<&T> {
        #[cfg(debug_assertions)]
        log::trace!("got: {}", self.picker_ui.results.index());
        self.picker_ui
            .worker
            .get_nth(self.picker_ui.results.index())
    }

    /// Maps all selected indices into a vector.
    ///
    /// Pure selection only:
    /// - No fallback
    /// - Uses `worker.get_by_idx`
    /// - Order follows `selector.iter()`
    pub fn map_selections_to_vec<U>(&self, mut f: impl FnMut(u32, &T) -> U) -> Vec<U> {
        self.picker_ui
            .selector
            .iter()
            .filter_map(|&idx| {
                self.picker_ui
                    .worker
                    .get_by_idx(idx)
                    .map(|item| f(idx, item))
            })
            .collect()
    }

    /// Maps selected items, falling back to current item if selection is empty.
    ///
    /// Delegates core work to [`MMState::map_selections_to_vec`].
    pub fn map_selected_to_vec<U>(&self, mut f: impl FnMut(u32, &T) -> U) -> Vec<U> {
        let mut out = self.map_selections_to_vec(&mut f);
        if out.is_empty()
            && let Some((idx, item)) = self.picker_ui.current_indexed()
        {
            out.push(f(idx, item));
        }
        out
    }

    pub fn injector(&self) -> WorkerInjector<T, D> {
        self.picker_ui.worker.injector()
    }

    /// Result column widths
    /// Note that the first width doesn't include the indentation.
    pub fn widths(&self) -> &Vec<u16> {
        self.picker_ui.results.widths()
    }

    pub fn status(&self) -> &Status {
        // replace StatusType with the actual type
        &self.picker_ui.results.status
    }

    pub fn selections(&self) -> &Selector {
        &self.picker_ui.selector
    }

    pub fn preview_visible(&self) -> bool {
        self.preview_ui.as_ref().is_some_and(|s| s.visible())
    }

    pub fn get_content_and_index(&self) -> (String, u32) {
        (
            self.picker_ui.query.input.clone(),
            self.picker_ui.results.index(),
        )
    }

    pub fn restart_worker(&mut self) {
        self.picker_ui.restart();
        self.state.synced = [false; 2];
    }

    pub fn make_env_vars(&self) -> EnvVars {
        let mut vars = env_vars! {
            "FZF_LINES" => self.tui_area().height.to_string(),
            "FZF_COLUMNS" => self.tui_area().width.to_string(),
            "FZF_TOTAL_COUNT" => self.status().item_count.to_string(),
            "FZF_MATCH_COUNT" => self.status().matched_count.to_string(),
            "FZF_SELECT_COUNT" => self.selections().len().to_string(),
            "FZF_POS" => self.picker_ui.current_indexed().map_or("".to_string(), |x| format!("{}", x.0)),
            "FZF_QUERY" => self.input.clone(),
            "FZF_MODE" => event::MODE
                .lock()
                .map(|m| m.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(","))
                .unwrap_or_default(),

            "MM_LINES" => self.tui_area().height.to_string(),
            "MM_COLUMNS" => self.tui_area().width.to_string(),
            "MM_TOTAL_COUNT" => self.status().item_count.to_string(),
            "MM_MATCH_COUNT" => self.status().matched_count.to_string(),
            "MM_SELECT_COUNT" => self.selections().len().to_string(),
            "MM_POS" => self.picker_ui.current_indexed().map_or("".to_string(), |x| format!("{}", x.0)),
            "MM_QUERY" => self.input.clone(),
            "MM_MODE" => event::MODE
                .lock()
                .map(|m| m.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(","))
                .unwrap_or_default(),
        };

        vars.extend(self.envs.clone());
        vars
    }

    // -------- other

    /// Some(s) -> Save current visibility, set visibility to s
    /// None -> Restore saved visibility
    pub fn stash_preview_visibility(&mut self, show: Option<bool>) {
        log::trace!("Called stash_preview_visibility with {show:?}");
        let p = unwrap!(self.preview_ui);
        if let Some(s) = show {
            self.state.stashed_preview_visibility = Some(p.visible());
            p.show(s);
        } else if let Some(s) = self.state.stashed_preview_visibility.take() {
            p.show(s);
        }
    }
}

// ----- BOILERPLATE -----------
impl<'a, 'b: 'a, T: SSS, D> std::ops::Deref for MMState<'a, 'b, T, D> {
    type Target = State;

    fn deref(&self) -> &Self::Target {
        self.state
    }
}

impl<'a, 'b: 'a, T: SSS, D> std::ops::DerefMut for MMState<'a, 'b, T, D> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state
    }
}
