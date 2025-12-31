use cli_boilerplate_automation::{broc::EnvVars, env_vars};
use std::{
    collections::HashSet,
    ops::Deref,
};

use crate::{
    SSS, Selection, Selector, message::Event, nucleo::{Status, injector::WorkerInjector}, ui::{PickerUI, PreviewUI, Rect, UI}
};

// --------------------------------------------------------------------
// todo: use bitflag for more efficient hashmap

#[derive(Default)]
pub struct State<S: Selection> {
    pub current: Option<(u32, S)>,
    pub input: String,
    pub col: Option<usize>,

    pub(crate) preview_run: String,
    pub(crate) preview_set: Option<String>,
    pub iterations: u32,
    pub preview_show: bool,
    pub layout: [Rect; 4],

    pub(crate) matcher_running: bool,
    pub(crate) events: HashSet<Event>,
}

pub struct MMState<'a, T: SSS, S: Selection> {
    pub(crate) state: &'a State<S>,

    pub picker_ui: &'a PickerUI<'a, T, S>,
    pub ui: &'a UI,
    pub preview_ui: Option<&'a PreviewUI>,
}

impl<'a, T: SSS, S: Selection> MMState<'a, T, S> {
    pub fn previewer_area(&self) -> Option<&Rect> {
        self.preview_ui.map(|ui| &ui.area)
    }

    pub fn ui_area(&self) -> &Rect {
        &self.ui.area
    }

    pub fn current_raw(&self) -> Option<&T> {
        self.picker_ui.worker.get_nth(self.picker_ui.results.index())
    }
    /// Runs f on selections if nonempty, otherwise, the current item
    pub fn map_selected_to_vec<U>(&self, mut f: impl FnMut(&S) -> U) -> Vec<U> {
        if !self.picker_ui.selections.is_empty() {
            self.picker_ui.selections.map_to_vec(f)
        } else {
            self.current.iter().map(|s| f(&s.1)).collect()
        }
    }

    pub fn injector(&self) -> WorkerInjector<T> {
        self.picker_ui.worker.injector()
    }

    pub fn widths(&self) -> &Vec<u16> {
        self.picker_ui.results.widths()
    }

    pub fn status(&self) -> &Status { // replace StatusType with the actual type
        &self.picker_ui.results.status
    }

    pub fn selections(&self) -> &Selector<T, S> {
        &self.picker_ui.selections
    }

    pub fn make_env_vars(&self) -> EnvVars {
        env_vars! {
            "FZF_LINES" => self.ui_area().height.to_string(),
            "FZF_COLUMNS" => self.ui_area().width.to_string(),
            "FZF_TOTAL_COUNT" => self.status().item_count.to_string(),
            "FZF_MATCH_COUNT" => self.status().matched_count.to_string(),
            "FZF_SELECT_COUNT" => self.selections().len().to_string(),
            "FZF_POS" => self.current.as_ref().map_or("".to_string(), |x| format!("{}", x.0)),
            "FZF_QUERY" => self.input.clone(),
        }
    }

    // pub fn dispatch<E>(&mut self, handler: &DynamicMethod<T, S, E>, event: &E) {
    //     (handler)(self, event);
    // }
}

impl<S: Selection> State<S> {
    pub fn new() -> Self {
        // this is the same as default
        Self {
            current: None,

            preview_run: String::new(),
            preview_set: None,
            preview_show: false,
            layout: [Rect::default(); 4],
            col: None,

            input: String::new(),
            iterations: 0,
            matcher_running: true,

            events: HashSet::new(),
        }
    }
    // ------ properties -----------
    pub fn take_current(&mut self) -> Option<S> {
        self.current.take().map(|x| x.1)
    }

    pub fn preview_payload(&self) -> &String {
        &self.preview_run
    }

    pub fn contains(&self, event: &Event) -> bool {
        self.events.contains(event)
    }

    pub fn insert(&mut self, event: Event) -> bool {
        self.events.insert(event)
    }

    pub fn preview_set_payload(&self) -> &Option<String> {
        &self.preview_set
    }

    // ------- updates --------------
    pub fn update_current(&mut self, new_current: Option<(u32, S)>) -> bool {
        let changed = self.current.as_ref().map(|x| x.0) != new_current.as_ref().map(|x| x.0);
        if changed {
            self.current = new_current;
            self.insert(Event::CursorChange);
        }
        changed
    }

    pub fn update_input(&mut self, new_input: &str) -> bool {
        let changed = self.input != new_input;
        if changed {
            self.input = new_input.to_string();
            self.insert(Event::QueryChange);
        }
        changed
    }

    pub fn update_preview(&mut self, context: &str) -> bool {
        let changed = self.preview_run != context;
        if changed {
            self.preview_run = context.into();
            self.insert(Event::PreviewChange);
        }
        changed
    }

    pub fn update_preview_set(&mut self, context: String) -> bool {
        let next = Some(context);
        let changed = self.preview_set != next;
        if changed {
            self.preview_set = next;
            self.insert(Event::PreviewSet);
        }
        changed
    }

    pub fn update_preview_unset(&mut self) {
        self.preview_set = None;
        self.insert(Event::PreviewSet);
    }

    pub fn update_layout(&mut self, context: [Rect; 4]) -> bool {
        let changed = self.layout != context;
        if changed {
            self.insert(Event::Resize);
            self.layout = context;
        }
        changed
    }

    /// Emit PreviewChange event on visibility change
    pub fn update_preview_ui(&mut self, preview_ui: &PreviewUI) -> bool {
        let next = preview_ui.is_show();
        let changed = self.preview_show != next;
        self.preview_show = next;
        // todo: cache to make up for this
        if changed && next {
            self.insert(Event::PreviewChange);
        };
        changed
    }

    pub fn update<'a, T: SSS>(&'a mut self, picker_ui: &'a PickerUI<T, S>){
        if self.iterations == 0 {
            self.insert(Event::Start);
        }
        self.iterations += 1;

        self.update_input(&picker_ui.input.input);
        self.col = picker_ui.results.col();

        if self.matcher_running != picker_ui.results.status.running {
            if !picker_ui.results.status.running && picker_ui.results.status.item_count != 0 {
                self.insert(Event::Synced);
            }
            self.matcher_running = picker_ui.results.status.running;
        };

        let current_raw = picker_ui.worker.get_nth(picker_ui.results.index());
        self.update_current(current_raw.map(picker_ui.selections.identifier));
    }



    // ---------- flush -----------
    pub fn dispatcher<'a, T: SSS>(&'a self, ui: &'a UI, picker_ui: &'a PickerUI<T, S>, preview_ui: Option<&'a PreviewUI>) -> MMState<'a, T, S> {
        MMState {
            state: self,
            picker_ui,
            ui,
            preview_ui,
        }
    }

    fn reset(&mut self) {
        // nothing
    }

    pub fn events(
        &mut self,
    ) -> HashSet<Event> {
        self.reset();
        // this rules out persistent preview_set, todo: impl effects to trigger this instead
        std::mem::take(&mut self.events) // maybe copy is faster dunno
    }
}

// ----- BOILERPLATE -----------
impl<S: Selection> std::fmt::Debug for State<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
        .field("input", &self.input)
        .field("preview_payload", &self.preview_run)
        .field("iterations", &self.iterations)
        .field("preview_show", &self.preview_show)
        .field("layout", &self.layout)
        .field("events", &self.events)
        .finish_non_exhaustive()
    }
}

impl<'a, T: SSS, S: Selection> Deref for MMState<'a, T, S> {
    type Target = State<S>;

    fn deref(&self) -> &Self::Target {
        self.state
    }
}