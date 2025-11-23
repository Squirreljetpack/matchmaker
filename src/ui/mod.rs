use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Table,
};

use crate::{
    MMItem, Selection, SelectionSet, config::{
        DisplayConfig, InputConfig, PreviewLayoutSetting, RenderConfig, ResultsConfig, TerminalLayoutSettings, UiConfig
    }, nucleo::Worker, proc::Preview, tui::Tui
};

mod input;
mod preview;
mod results;
mod display;
pub use display::DisplayUI;
pub use input::InputUI;
pub use preview::PreviewUI;
pub use results::ResultsUI;

// UI

pub struct UI {
    pub layout: Option<TerminalLayoutSettings>,
    pub area: Rect, // unused
    pub config: UiConfig
}

impl UI {
    pub fn new<'a, T: MMItem, S: Selection, C, W: std::io::Write>(
        mut config: RenderConfig,
        matcher: &'a mut nucleo::Matcher,
        worker: Worker<T, C>,
        selection_set: SelectionSet<T, S>,
        view: Option<Preview>,
        tui: &mut Tui<W>,
    ) -> (Self, PickerUI<'a, T, S, C>, Option<PreviewUI>) {
        if config.results.reverse.is_none() {
            config.results.reverse = Some(
                tui.is_fullscreen() && tui.area.y < tui.area.height / 2
            );
        }

        let ui = Self {
            layout: tui.layout().clone(),
            area: tui.area,
            config: config.ui
        };

        let picker = PickerUI::new(config.results, config.input, config.header, config.footer, matcher, worker, selection_set);

        let preview = if let Some(view) = view {
            Some(PreviewUI::new(view, config.preview))
        } else {
            None
        };

        (ui, picker, preview)
    }

    pub fn update_dimensions(&mut self, area: Rect) {
        self.area = area;
    }

    pub fn make_ui(&self) -> ratatui::widgets::Block<'_> {
        self.config.border.as_block()
    }

    pub fn inner_area(&self, area: &Rect) -> Rect {
        Rect {
            x: area.x + self.config.border.left(),
            y: area.y + self.config.border.top(),
            width: area.width.saturating_sub(self.config.border.width()),
            height: area.height.saturating_sub(self.config.border.height()),
        }
    }
}

pub struct PickerUI<'a, T: MMItem, S: Selection, C> {
    pub results: ResultsUI,
    pub input: InputUI,
    pub header: DisplayUI,
    pub footer: DisplayUI,
    pub matcher: &'a mut nucleo::Matcher,
    pub selections: SelectionSet<T, S>,
    pub worker: Worker<T, C>,
}

impl<'a, T: MMItem, S: Selection, C> PickerUI<'a, T, S, C> {
    pub fn new(
        results_config: ResultsConfig,
        input_config: InputConfig,
        header_config: DisplayConfig,
        footer_config: DisplayConfig,
        matcher: &'a mut nucleo::Matcher,
        worker: Worker<T, C>,
        selections: SelectionSet<T, S>,
    ) -> Self {
        Self {
            results: ResultsUI::new(results_config),
            input: InputUI::new(input_config),
            header: DisplayUI::new(header_config),
            footer: DisplayUI::new(footer_config),
            matcher,
            selections,
            worker,
        }
    }

    pub fn layout(&self, area: Rect) -> [Rect; 5] {
        let PickerUI { input, header, footer, .. } = self;

        let mut constraints = [
        Constraint::Length(1 + input.config.border.height()), // input
        Constraint::Length(1), // status
        Constraint::Length(header.height()),
        Constraint::Fill(1), // results
        Constraint::Length(footer.height()),
        ];

        if self.reverse() {
            constraints.reverse();
        }

        let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

        if self.reverse() {
            [chunks[4], chunks[3], chunks[2], chunks[1], chunks[0]]
        } else {
            [chunks[0], chunks[1], chunks[2], chunks[3], chunks[4]]
        }
    }
}

impl<'a, T: MMItem, O: Selection, C> PickerUI<'a, T, O, C> {
    pub fn make_table(&mut self) -> Table<'_> {
        self.results
        .make_table(&mut self.worker, &mut self.selections, self.matcher)
    }

    pub fn update(&mut self) {
        self.worker.find(&self.input.input);
    }

    // creation from UI ensures Some
    pub fn reverse(&self) -> bool {
        self.results.reverse()
    }
}

impl PreviewLayoutSetting {
    pub fn split(&self, area: Rect) -> [Rect; 2] {
        use crate::config::Side;
        use ratatui::layout::{Constraint, Direction, Layout};

        let direction = match self.side {
            Side::Left | Side::Right => Direction::Horizontal,
            Side::Top | Side::Bottom => Direction::Vertical,
        };

        let side_first = matches!(self.side, Side::Left | Side::Top);

        let total = if matches!(direction, Direction::Horizontal) {
            area.width
        } else {
            area.height
        };

        let p = self.percentage.get();

        let mut side_size = if p != 0 { total * p / 100 } else { 0 };

        let min = if self.min < 0 {
            total.saturating_sub((-self.min) as u16)
        } else {
            self.min as u16
        };

        let max = if self.max < 0 {
            total.saturating_sub((-self.max) as u16)
        } else {
            self.max as u16
        };

        side_size = side_size.clamp(min, max);

        let side_constraint = Constraint::Length(side_size);

        let constraints = if side_first {
            [side_constraint, Constraint::Min(0)]
        } else {
            [Constraint::Min(0), side_constraint]
        };

        let chunks = Layout::default()
        .direction(direction)
        .constraints(constraints)
        .split(area);

        if side_first {
            [chunks[0], chunks[1]]
        } else {
            [chunks[1], chunks[0]]
        }
    }
}
