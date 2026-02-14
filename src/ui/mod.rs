mod display;
mod input;
mod overlay;
mod preview;
mod results;
pub use display::DisplayUI;
pub use input::InputUI;
pub use overlay::*;
pub use preview::PreviewUI;
pub use results::ResultsUI;

pub use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Table,
}; // reexport for convenience

use crate::{
    SSS, Selection, Selector,
    config::{
        DisplayConfig, InputConfig, PreviewLayoutSetting, RenderConfig, ResultsConfig,
        TerminalLayoutSettings, UiConfig,
    },
    nucleo::Worker,
    preview::Preview,
    render::Click,
    tui::Tui,
};
// UI
pub struct UI {
    pub layout: Option<TerminalLayoutSettings>,
    pub area: Rect, // unused
    pub config: UiConfig,
}

// requires columns > 1
impl UI {
    pub fn new<'a, T: SSS, S: Selection, W: std::io::Write>(
        mut config: RenderConfig,
        matcher: &'a mut nucleo::Matcher,
        worker: Worker<T>,
        selection_set: Selector<T, S>,
        view: Option<Preview>,
        tui: &mut Tui<W>,
    ) -> (Self, PickerUI<'a, T, S>, DisplayUI, Option<PreviewUI>) {
        assert!(!worker.columns.is_empty());

        if config.results.reverse.is_default() {
            config.results.reverse = (
                tui.is_fullscreen() && tui.area.y < tui.area.height / 2
                // reverse if fullscreen + cursor is in lower half of the screen
            )
            .into()
        }

        let ui = Self {
            layout: tui.config.layout.clone(),
            area: tui.area,
            config: config.ui,
        };

        let picker = PickerUI::new(
            config.results,
            config.input,
            config.header,
            matcher,
            worker,
            selection_set,
        );

        let preview = if let Some(view) = view {
            Some(PreviewUI::new(view, config.preview))
        } else {
            None
        };

        let footer = DisplayUI::new(config.footer);

        (ui, picker, footer, preview)
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

pub struct PickerUI<'a, T: SSS, S: Selection> {
    pub results: ResultsUI,
    pub input: InputUI,
    pub header: DisplayUI,
    pub matcher: &'a mut nucleo::Matcher,
    pub selector: Selector<T, S>,
    pub worker: Worker<T>,
}

impl<'a, T: SSS, S: Selection> PickerUI<'a, T, S> {
    pub fn new(
        results_config: ResultsConfig,
        input_config: InputConfig,
        header_config: DisplayConfig,
        matcher: &'a mut nucleo::Matcher,
        worker: Worker<T>,
        selections: Selector<T, S>,
    ) -> Self {
        Self {
            results: ResultsUI::new(results_config),
            input: InputUI::new(input_config),
            header: DisplayUI::new(header_config),
            matcher,
            selector: selections,
            worker,
        }
    }

    pub fn layout(&self, area: Rect) -> [Rect; 4] {
        let PickerUI {
            input,
            header,
            results,
            ..
        } = self;

        let mut constraints = [
            Constraint::Length(1 + input.config.border.height()), // input
            Constraint::Length(results.config.status_show as u16), // status
            Constraint::Length(header.height()),
            Constraint::Fill(1), // results
        ];

        if self.reverse() {
            constraints.reverse();
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        std::array::from_fn(|i| {
            chunks[if self.reverse() {
                chunks.len() - i - 1
            } else {
                i
            }]
        })
    }
}

impl<'a, T: SSS, O: Selection> PickerUI<'a, T, O> {
    pub fn make_table(&mut self, click: &mut Click) -> (Table<'_>, u16) {
        let table =
            self.results
                .make_table(&mut self.worker, &mut self.selector, self.matcher, click);
        let width = self.results.table_width();
        (table, width)
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

        let p = self.percentage.inner();

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
