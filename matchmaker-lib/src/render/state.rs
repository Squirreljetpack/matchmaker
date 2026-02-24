use cli_boilerplate_automation::{broc::EnvVars, env_vars};

use crate::{
    SSS, Selection, Selector,
    action::ActionExt,
    event::EventSender,
    message::{Event, Interrupt},
    nucleo::{Status, injector::WorkerInjector},
    ui::{DisplayUI, OverlayUI, PickerUI, PreviewUI, Rect, UI},
};

// --------------------------------------------------------------------
#[derive(Default)]
pub struct State {
    last_id: Option<u32>,
    interrupt: Interrupt,
    interrupt_payload: String,

    // Stores "last" state to emit events on change
    pub(crate) input: String,
    pub(crate) col: Option<usize>,
    pub(crate) iterations: u32,
    pub(crate) preview_visible: bool,
    pub(crate) layout: [Rect; 4], //preview, input, status, results
    pub(crate) overlay_index: Option<usize>,
    pub(crate) synced: bool,

    pub(crate) events: Event,

    /// The String passed to SetPreview
    pub preview_set_payload: Option<String>,
    /// The payload left by [`crate::action::Action::Preview`]
    pub preview_payload: String,
    /// A place to stash the preview visibility when overriding it
    stashed_preview_visibility: Option<bool>,
    /// Setting this to true finishes the picker with the contents of [`Selector`].
    /// If [`Selector`] is disabled, the picker finishes with the current item.
    /// If there are no items to finish with, the picker finishes with [`crate::errors::MatchError::Abort`]\(0).
    pub should_quit: bool,
    /// Setting this to true finishes the picker with [`crate::MatchError::NoMatch`].
    pub should_quit_nomatch: bool,
    pub filtering: bool,
}

impl State {
    pub fn new() -> Self {
        // this is the same as default
        Self {
            last_id: None,
            interrupt: Interrupt::None,
            interrupt_payload: String::new(),

            preview_payload: String::new(),
            preview_set_payload: None,
            preview_visible: false,
            stashed_preview_visibility: None,
            layout: [Rect::default(); 4],
            overlay_index: None,
            col: None,

            input: String::new(),
            iterations: 0,
            synced: true,

            events: Event::empty(),
            should_quit: false,
            should_quit_nomatch: false,
            filtering: true,
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
    pub fn preview_set_payload(&self) -> Option<String> {
        self.preview_set_payload.clone()
    }
    pub fn preview_payload(&self) -> &String {
        &self.preview_payload
    }
    pub fn stashed_preview_visibility(&self) -> Option<bool> {
        self.stashed_preview_visibility
    }

    // ------- updates --------------
    pub(crate) fn update_input(&mut self, new_input: &str) -> bool {
        let changed = self.input != new_input;
        if changed {
            self.input = new_input.to_string();
            self.insert(Event::QueryChange);
        }
        changed
    }

    pub(crate) fn update_preview(&mut self, context: &str) -> bool {
        let changed = self.preview_payload != context;
        if changed {
            self.preview_payload = context.into();
            self.insert(Event::PreviewChange);
        }
        changed
    }

    pub(crate) fn update_preview_set(&mut self, context: String) -> bool {
        let next = Some(context);
        let changed = self.preview_set_payload != next;
        if changed {
            self.preview_set_payload = next;
            self.insert(Event::PreviewSet);
        }
        changed
    }

    pub(crate) fn update_preview_unset(&mut self) {
        self.preview_set_payload = None;
        self.insert(Event::PreviewSet);
    }

    pub(crate) fn update_layout(&mut self, new_layout: [Rect; 4]) -> bool {
        let changed = self.layout != new_layout;
        if changed {
            self.insert(Event::Resize);
            self.layout = new_layout;
        }
        changed
    }

    /// Emit PreviewChange event on change to visible
    pub(crate) fn update_preview_ui(&mut self, preview_ui: &PreviewUI) -> bool {
        let changed = self.preview_visible != preview_ui.is_show();
        // todo: cache to make up for this?
        if changed && preview_ui.is_show() {
            self.insert(Event::PreviewChange);
            self.preview_visible = true;
        };
        changed
    }

    pub(crate) fn update<'a, T: SSS, S: Selection, A: ActionExt>(
        &'a mut self,
        picker_ui: &'a PickerUI<T, S>,
        overlay_ui: &'a Option<OverlayUI<A>>,
    ) {
        if self.iterations == 0 {
            self.insert(Event::Start);
        }
        self.iterations += 1;

        self.update_input(&picker_ui.input.input);
        self.col = picker_ui.results.col();

        let status = &picker_ui.results.status;
        if status.changed {
            // add a synced event
            if !picker_ui.results.status.running {
                if !self.synced {
                    self.insert(Event::Synced);
                    self.synced = true;
                } else {
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

        let new_id = get_current(picker_ui).map(|x| x.0);
        let changed = self.last_id != get_current(picker_ui).map(|x| x.0);
        if changed {
            self.last_id = new_id;
            self.insert(Event::CursorChange);
        }
    }

    // ---------- flush -----------
    pub(crate) fn dispatcher<'a, 'b: 'a, T: SSS, S: Selection>(
        &'a mut self,
        ui: &'a mut UI,
        picker_ui: &'a mut PickerUI<'b, T, S>,
        footer_ui: &'a mut DisplayUI,
        preview_ui: &'a mut Option<PreviewUI>,
        event_controller: &'a EventSender,
    ) -> MMState<'a, 'b, T, S> {
        MMState {
            state: self,
            ui,
            picker_ui,
            footer_ui,
            preview_ui,
            event_controller,
        }
    }

    fn reset(&mut self) {
        // nothing
    }

    pub(crate) fn events(&mut self) -> Event {
        self.reset();
        std::mem::take(&mut self.events)
    }
}

// ----------------------------------------------------------------------
pub struct MMState<'a, 'b: 'a, T: SSS, S: Selection> {
    // access through deref/mut
    pub(crate) state: &'a mut State,

    pub ui: &'a mut UI,
    pub picker_ui: &'a mut PickerUI<'b, T, S>,
    pub footer_ui: &'a mut DisplayUI,
    pub preview_ui: &'a mut Option<PreviewUI>,
    pub event_controller: &'a EventSender,
}

impl<'a, 'b: 'a, T: SSS, S: Selection> MMState<'a, 'b, T, S> {
    pub fn previewer_area(&self) -> Option<&Rect> {
        self.preview_ui.as_ref().map(|ui| &ui.area)
    }

    pub fn ui_area(&self) -> &Rect {
        &self.ui.area
    }

    pub fn current_item(&self) -> Option<S> {
        get_current(self.picker_ui).map(|s| s.1)
    }

    /// Same as current_item, but without applying the identifier.
    pub fn current_raw(&self) -> Option<&T> {
        self.picker_ui
            .worker
            .get_nth(self.picker_ui.results.index())
    }
    /// Runs f on selections if nonempty, otherwise, the current item
    pub fn map_selected_to_vec<U>(&self, mut f: impl FnMut(&S) -> U) -> Vec<U> {
        if !self.picker_ui.selector.is_empty() {
            self.picker_ui.selector.map_to_vec(f)
        } else {
            get_current(self.picker_ui)
                .iter()
                .map(|s| f(&s.1))
                .collect()
        }
    }

    pub fn injector(&self) -> WorkerInjector<T> {
        self.picker_ui.worker.injector()
    }

    pub fn widths(&self) -> &Vec<u16> {
        self.picker_ui.results.widths()
    }

    pub fn status(&self) -> &Status {
        // replace StatusType with the actual type
        &self.picker_ui.results.status
    }

    pub fn selections(&self) -> &Selector<T, S> {
        &self.picker_ui.selector
    }

    pub fn preview_visible(&self) -> bool {
        self.preview_ui.as_ref().is_some_and(|s| s.is_show())
    }

    pub fn get_content_and_index(&self) -> (String, u32) {
        (
            self.picker_ui.input.input.clone(),
            self.picker_ui.results.index(),
        )
    }

    pub fn make_env_vars(&self) -> EnvVars {
        env_vars! {
            "FZF_LINES" => self.ui_area().height.to_string(),
            "FZF_COLUMNS" => self.ui_area().width.to_string(),
            "FZF_TOTAL_COUNT" => self.status().item_count.to_string(),
            "FZF_MATCH_COUNT" => self.status().matched_count.to_string(),
            "FZF_SELECT_COUNT" => self.selections().len().to_string(),
            "FZF_POS" => get_current(self.picker_ui).map_or("".to_string(), |x| format!("{}", x.0)),
            "FZF_QUERY" => self.input.clone(),
        }
    }

    // -------- other
    pub fn stash_preview_visibility(&mut self, show: Option<bool>) {
        if let Some(p) = self.preview_ui {
            if let Some(s) = show {
                self.state.stashed_preview_visibility = Some(p.is_show());
                p.show(s);
            } else if let Some(s) = self.state.stashed_preview_visibility.take() {
                p.show(s);
            }
        }
    }
}

pub(crate) fn get_current<T: SSS, S: Selection>(picker_ui: &PickerUI<T, S>) -> Option<(u32, S)> {
    let current_raw = picker_ui.worker.get_nth(picker_ui.results.index());
    current_raw.map(picker_ui.selector.identifier)
}

// ----- BOILERPLATE -----------
impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("input", &self.input)
            .field("preview_payload", &self.preview_payload)
            .field("iterations", &self.iterations)
            .field("preview_show", &self.preview_visible)
            .field("layout", &self.layout)
            .field("events", &self.events)
            .finish_non_exhaustive()
    }
}

impl<'a, 'b: 'a, T: SSS, S: Selection> std::ops::Deref for MMState<'a, 'b, T, S> {
    type Target = State;

    fn deref(&self) -> &Self::Target {
        self.state
    }
}

impl<'a, 'b: 'a, T: SSS, S: Selection> std::ops::DerefMut for MMState<'a, 'b, T, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state
    }
}
