mod display;
mod input;
mod overlay;
mod preview;
mod results;
mod status;
pub mod utils;
pub use display::*;
pub use input::*;
pub use overlay::*;
pub use preview::*;

pub use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Table,
};
pub use results::*;
pub use status::*; // reexport for convenience

use crate::{
    SSS, Selection, Selector,
    config::{
        DisplayConfig, QueryConfig, RenderConfig, ResultsConfig, StatusConfig,
        TerminalLayoutSettings, UiConfig,
    },
    nucleo::Worker,
    preview::Preview,
    tui::Tui,
};
// UI
pub struct UI {
    pub layout: Option<TerminalLayoutSettings>,
    area: Rect, // unused
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
        hidden_columns: Vec<bool>,
    ) -> (Self, PickerUI<'a, T, S>, DisplayUI, Option<PreviewUI>) {
        assert!(!worker.columns.is_empty());

        if config.results.reverse.is_none() {
            config.results.reverse = (
                tui.is_fullscreen() && tui.area.y < tui.area.height / 2
                // reverse if fullscreen + cursor is in lower half of the screen
            )
            .into()
        }

        let ui_area = [
            tui.area.width.saturating_sub(config.ui.border.width()),
            tui.area.height.saturating_sub(config.ui.border.height()),
        ];

        let area = Rect {
            x: tui.area.x + config.ui.border.left(),
            y: tui.area.y + config.ui.border.top(),
            width: ui_area[0],
            height: ui_area[1],
        };

        let ui = Self {
            layout: tui.config.layout.clone(),
            area,
            config: config.ui,
        };

        let mut picker = PickerUI::new(
            config.results,
            config.status,
            config.query,
            config.header,
            matcher,
            worker,
            selection_set,
        );
        picker.results.set_hidden_columns(hidden_columns);

        let preview = if let Some(view) = view {
            Some(PreviewUI::new(view, config.preview, ui_area))
        } else {
            None
        };

        let footer = DisplayUI::new(config.footer);

        (ui, picker, footer, preview)
    }

    pub fn update_dimensions(&mut self, area: Rect) {
        let border = &self.config.border;

        self.area = Rect {
            x: area.x + border.left(),
            y: area.y + border.top(),
            width: area.width.saturating_sub(border.width()),
            height: area.height.saturating_sub(border.height()),
        };
    }

    pub fn make_ui(&self) -> ratatui::widgets::Block<'_> {
        self.config.border.as_block()
    }

    pub fn area(&self) -> Rect {
        self.area
    }

    pub fn compute_area(&self, area: &Rect) -> Rect {
        Rect {
            x: area.x + self.config.border.left(),
            y: area.y + self.config.border.top(),
            width: area.width.saturating_sub(self.config.border.width()),
            height: area.height.saturating_sub(self.config.border.height()),
        }
    }

    pub fn full_area(&self) -> Rect {
        Rect {
            x: self.area.x - self.config.border.left(),
            y: self.area.y - self.config.border.top(),
            width: self.area.width + self.config.border.width(),
            height: self.area.height + self.config.border.height(),
        }
    }
}

pub struct PickerUI<'a, T: SSS, S: Selection> {
    pub results: ResultsUI,
    pub status: StatusUI,
    pub query: QueryUI,
    pub header: DisplayUI,
    pub matcher: &'a mut nucleo::Matcher,
    pub selector: Selector<T, S>,
    pub worker: Worker<T>,
}

impl<'a, T: SSS, S: Selection> PickerUI<'a, T, S> {
    pub fn new(
        results_config: ResultsConfig,
        status_config: StatusConfig,
        input_config: QueryConfig,
        header_config: DisplayConfig,
        matcher: &'a mut nucleo::Matcher,
        worker: Worker<T>,
        selector: Selector<T, S>,
    ) -> Self {
        Self {
            results: ResultsUI::new(results_config, worker.columns.len()),
            status: StatusUI::new(status_config),
            query: QueryUI::new(input_config),
            header: DisplayUI::new(header_config),
            matcher,
            selector,
            worker,
        }
    }

    pub(crate) fn restart(&mut self) {
        self.worker.restart(false);
        self.results.set_dirty();
    }

    pub fn active_column_index(&self) -> usize {
        let cursor_byte = self.query.byte_index(self.query.cursor() as usize);

        self.worker
            .query
            .current_column(cursor_byte)
            .and_then(|name| self.worker.columns.iter().position(|c| &c.name == name))
            .unwrap_or(self.worker.query.primary_column_index())
    }

    pub fn layout(&self, area: Rect) -> [Rect; 4] {
        let PickerUI {
            query,
            header,
            status,
            ..
        } = self;

        let mut constraints = [
            Constraint::Length(1 + query.config.border.height()), // input
            Constraint::Length(status.status_config.show as u16), // status
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
    pub fn update(&mut self) {
        self.worker.find(&self.query.input);
    }
    pub fn update_status(&mut self) {
        self.results.status = Worker::new_snapshot(&mut self.worker.nucleo).1;
    }

    // creation from UI ensures Some
    pub fn reverse(&self) -> bool {
        self.results.reverse()
    }
}
