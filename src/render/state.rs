use cli_boilerplate_automation::{broc::EnvVars, env_vars};
use ratatui::{
    layout::Rect, text::Text}
    ;
    use std::{
        collections::HashSet,
        ops::Deref,
    };

    use crate::{
        MMItem, Selection, SelectionSet, message::Event, nucleo::{Status, injector::WorkerInjector}, render::DynamicMethod, ui::{Overlay, OverlayUI, PickerUI, PreviewUI, UI}
    };

    // --------------------------------------------------------------------
    // todo: use bitflag for more efficient hashmap

    #[derive(Default)]
    pub struct State<S: Selection> {
        pub current: Option<(u32, S)>,
        pub input: String,
        pub col: Option<usize>,

        preview_run: String,
        preview_set: Option<String>,
        pub iterations: u32,
        pub preview_show: bool,
        pub layout: [Rect; 4],

        events: HashSet<Event>,
    }

    pub struct MMState<'a, T: MMItem, S: Selection> {
        state: &'a State<S>,

        pub picker_ui: &'a PickerUI<'a, T, S>,
        pub ui: &'a UI,
        pub preview_ui: Option<&'a PreviewUI>,

        /// Exposes flags which affect the render loop
        pub effects: Effects,
    }

    // mutate this to mutate
    #[derive(Default)]
    pub struct Effects {
        pub clear_preview_set: bool,
        pub overlay_widget: Option<Box<dyn Overlay>>,
        pub header: Option<Text<'static>>,
        pub footer: Option<Text<'static>>,
        pub input: Option<(String, u16)>
    }

    impl Effects {
        pub fn clear_input(&mut self) {
            self.input = Some(Default::default())
        }
    }

    impl<'a, T: MMItem, S: Selection> MMState<'a, T, S> {
        pub(crate) fn new(
            state: &'a State<S>,
            picker_ui: &'a PickerUI<T, S>,
            ui: &'a UI,
            preview_ui: Option<&'a PreviewUI>,
        ) -> Self {
            Self {
                state,
                picker_ui,
                ui,
                preview_ui,
                effects: Effects::default(),
            }
        }

        // ------- Getters --------
        pub fn previewer_area(&self) -> Option<&Rect> {
            self.preview_ui.map(|ui| &ui.area)
        }

        pub fn ui_area(&self) -> &Rect {
            &self.ui.area
        }

        pub fn current_raw(&self) -> Option<&T> {
            self.picker_ui.worker.get_nth(self.picker_ui.results.index())
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

        pub fn selections(&self) -> &SelectionSet<T, S> {
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

        pub fn dispatch<E>(&mut self, handler: &DynamicMethod<T, S, E>, event: &E) {
            (handler)(self, event);
        }
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

                events: HashSet::new(),
            }
        }

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

        fn reset(&mut self) {
            self.iterations += 1;
        }

        pub fn update<'a, T: MMItem>(&'a mut self, picker_ui: &'a PickerUI<T, S>){
            self.update_input(&picker_ui.input.input);
            self.col = picker_ui.results.col();

            let current_raw = picker_ui.worker.get_nth(picker_ui.results.index());
            self.update_current(current_raw.map(picker_ui.selections.identifier));
        }

        pub fn dispatcher<'a, T: MMItem>(&'a self, ui: &'a UI, picker_ui: &'a PickerUI<T, S>, preview_ui: Option<&'a PreviewUI>) -> MMState<'a, T, S> {
            MMState::new(self,
                picker_ui,
                ui,
                preview_ui,
            )
        }

        #[allow(unused)]
        // Using effects avoids lifetime issues at the cost of additional allocation
        pub fn apply_effects<T: MMItem>(&mut self, effects: Effects, ui: &mut UI, picker_ui: &mut PickerUI<T, S>, preview_ui: Option<&mut PreviewUI>, overlay_ui: &mut OverlayUI) {
            if effects.clear_preview_set {
                self.preview_set = None
            }
            if let Some(text) = effects.footer {
                picker_ui.footer.text = text
            }
            if let Some(text) = effects.header {
                picker_ui.header.text = text
            }
            if let Some(overlay) = effects.overlay_widget {
                overlay_ui.set(overlay, &ui.area);
            }
            if let Some((input, cursor)) = effects.input {
                picker_ui.input.set(input, cursor);
            }
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

    impl<'a, T: MMItem, S: Selection> Deref for MMState<'a, T, S> {
        type Target = State<S>;

        fn deref(&self) -> &Self::Target {
            self.state
        }
    }

    // impl<'a, T: MMItem, S: Selection> Clone for EphemeralState<'a, T, S> {
    //     fn clone(&self) -> Self {
    //         Self {
    //             state: self.state,
    //             ui_area: self.ui_area,
    //             picker_ui: self.picker_ui,
    //             previewer_area: self.previewer_area,
    //             effects: self.effects,
    //         }
    //     }
    // }