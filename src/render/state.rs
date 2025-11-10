use bitflags::bitflags;
use crossterm::event::{self};
use log::{debug, error, warn};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Style, Stylize},
    text::Text,
    widgets::{Block, Borders, Paragraph, Row, Table, Wrap},
};
use rustc_hash::FxHashSet;
use std::{
    cell::{RefCell, RefMut},
    collections::HashSet,
    fmt::{Formatter, Write},
    ops::Deref,
    sync::Arc,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    PickerItem, Selection, SelectionSet,
    action::Action,
    config::{
        InputConfig, PreviewConfig, PreviewSetting, RenderConfig, ResultsConfig,
        TerminalLayoutSettings,
    },
    env_vars,
    message::Event,
    nucleo::{injector::WorkerInjector, worker::{Status, Worker}},
    spawn::{EnvVars, preview::PreviewerView},
    tui::Tui,
    ui::{PickerUI, PreviewUI, UI},
    utils::text::{
        clip_text_lines, fit_width, grapheme_index_to_byte_index, prefix_text, substitute_escaped,
    },
};

// --------------------------------------------------------------------
// todo: use bitflag for more efficient hashmap

pub struct State<S: Selection, C> {
    pub current: Option<(u32, S)>,
    pub input: String,
    pub col: Option<usize>,
    
    preview_payload: String,
    // pub execute_payload: Option<String>,
    // pub become_payload: Option<String>,
    pub context: Arc<C>,
    pub iterations: u32,
    pub preview_show: bool,
    pub layout: [Rect; 4],
    
    events: HashSet<Event>,
}

pub struct EphemeralState<'a, T: PickerItem, S: Selection, C> {
    state: &'a State<S, C>,
    
    pub picker_ui: &'a PickerUI<'a, T, S, C>,
    pub area: &'a Rect,
    pub previewer_area: Option<Rect>,
    pub effects: Effects,
}

impl<'a, T: PickerItem, S: Selection, C> EphemeralState<'a, T, S, C> {    
    pub fn new(
        state: &'a State<S, C>,
        picker_ui: &'a PickerUI<T, S, C>,
        area: &'a Rect,
        previewer_area: Option<Rect>,
    ) -> Self {
        Self {
            state,
            picker_ui,
            area,
            previewer_area,
            effects: Effects::empty(),
        }
    }
    
    pub fn current_raw(&self) -> Option<&T> {
        self.picker_ui.worker.get_nth(self.picker_ui.results.index())
    }

    pub fn injector(&self) -> WorkerInjector<T, C> {
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
            "FZF_LINES" => self.area.height.to_string(),
            "FZF_COLUMNS" => self.area.width.to_string(),
            "FZF_TOTAL_COUNT" => self.status().item_count.to_string(),
            "FZF_MATCH_COUNT" => self.status().matched_count.to_string(),
            "FZF_SELECT_COUNT" => self.selections().len().to_string(),
            "FZF_POS" => self.current.as_ref().map_or("".to_string(), |x| format!("{}", x.0)),
            "FZF_QUERY" => self.input.clone(),
        }
    }
}

impl<S: Selection, C> State<S, C> {
    pub fn new(context: Arc<C>) -> Self {
        Self {
            current: None,
            
            preview_payload: String::new(),
            preview_show: false,
            layout: [Rect::default(); 4],
            col: None,
            
            context,
            input: String::new(),
            iterations: 0,
            
            events: HashSet::new(),
        }
    }
    
    pub fn current(&mut self) -> Option<S> {
        self.current.take().map(|x| x.1)
    }
    
    pub fn preview_payload(&self) -> &String {
        &self.preview_payload
    }
    
    pub fn contains(&self, event: &Event) -> bool {
        self.events.contains(event)
    }
    
    pub fn insert(&mut self, event: Event) -> bool {
        self.events.insert(event)
    }
    
    fn reset(&mut self) {
        self.iterations += 1;
    }
    
    pub fn update_current(&mut self, new_current: Option<(u32, S)>) -> bool {
        let changed = self.current != new_current;
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
        let next = context;
        let changed = self.preview_payload != next;
        if changed {
            self.preview_payload = next.into();
            self.insert(Event::PreviewChange);
        }
        changed
    }
    
    pub fn update_layout(&mut self, context: [Rect; 4]) -> bool {
        let changed = self.layout != context;
        if changed {
            self.insert(Event::Resize);
            self.layout = context;
        }
        changed
    }
    
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
    
    
    
    pub fn update<'a, T: PickerItem>(&'a mut self, picker_ui: &'a PickerUI<T, S, C>){
        self.update_input(&picker_ui.input.input);
        self.col = picker_ui.results.col();
        
        let current_raw = picker_ui.worker.get_nth(picker_ui.results.index());
        self.update_current(current_raw.map(picker_ui.selections.identifier));
    }
    
    pub fn dispatcher<'a, T: PickerItem>(&'a self, ui: &'a UI, picker_ui: &'a PickerUI<T, S, C>, preview_ui: Option<&PreviewUI>) -> EphemeralState<'a, T, S, C> {
        EphemeralState::new(&self, 
            picker_ui,
            &ui.area,
            preview_ui.map(|p| p.area),
        )
    }
    
    pub fn events(
        &mut self,
    ) -> HashSet<Event> {
        let mut ret = std::mem::take(&mut self.events);
        self.reset();
        ret
    }
}

bitflags! {
    #[derive(Clone, Copy)]
    pub struct Effects: u32 {
        // const CREATE_WIDGET ?
    }
}



// ----- BOILERPLATE -----------
impl<S: Selection, C> std::fmt::Debug for State<S, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
        .field("input", &self.input)
        .field("preview_payload", &self.preview_payload)
        .field("iterations", &self.iterations)
        .field("preview_show", &self.preview_show)
        .field("layout", &self.layout)
        .field("events", &self.events)
        .finish_non_exhaustive()
    }
}

impl<'a, T: PickerItem, S: Selection, C> Deref for EphemeralState<'a, T, S, C> {
    type Target = State<S, C>;
    
    fn deref(&self) -> &Self::Target {
        self.state
    }
}

impl<'a, T: PickerItem, S: Selection, L> Clone for EphemeralState<'a, T, S, L> {
    fn clone(&self) -> Self {
        Self {
            state: self.state,
            area: self.area,
            picker_ui: self.picker_ui,
            previewer_area: self.previewer_area,
            effects: self.effects,
        }
    }
}