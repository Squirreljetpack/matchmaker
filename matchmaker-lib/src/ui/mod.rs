mod display;
mod input;
mod overlay;
mod preview;
mod results;
mod status;
pub mod utils;
use cba::_trace;
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
    SSS, Selector,
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
    pub fn new<'a, T: SSS, D: 'static, W: std::io::Write>(
        mut config: RenderConfig,
        matcher: &'a mut nucleo::Matcher,
        worker: Worker<T, D>,
        selector: Selector,
        view: Option<Preview>,
        tui: &mut Tui<W>,
    ) -> (Self, PickerUI<'a, T, D>, DisplayUI, Option<PreviewUI>) {
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

        let picker = PickerUI::new(
            config.results,
            config.status,
            config.query,
            config.header,
            matcher,
            worker,
            selector,
        );

        let preview = if let Some(view) = view {
            Some(PreviewUI::new(view, config.preview, ui_area))
        } else {
            None
        };

        let footer = DisplayUI::new(config.footer);

        (ui, picker, footer, preview)
    }

    /// Construct a picker UI without a terminal backend.
    ///
    /// Intended for non-interactive use, e.g. formatting templates without
    /// starting the interface. Only the pieces needed offline are built and
    /// returned: the display UI is not used (callers pass a
    /// `DisplayUI::default()` and `None` preview to the dispatcher), there is
    /// no preview, and no reverse detection happens.
    pub fn new_offline<'a, T: SSS, D: 'static>(
        config: RenderConfig,
        matcher: &'a mut nucleo::Matcher,
        worker: Worker<T, D>,
    ) -> (Self, PickerUI<'a, T, D>) {
        assert!(!worker.columns.is_empty());

        let ui = Self {
            layout: None,
            area: Rect::default(),
            config: config.ui,
        };

        let picker = PickerUI::new(
            config.results,
            config.status,
            config.query,
            config.header,
            matcher,
            worker,
            Selector::new(),
        );

        (ui, picker)
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

pub struct PickerUI<'a, T: SSS, D> {
    pub results: ResultsUI,
    pub status: StatusUI,
    pub query: QueryUI,
    pub header: DisplayUI,
    pub matcher: &'a mut nucleo::Matcher,
    pub selector: Selector,
    pub worker: Worker<T, D>,
    pub filtering: bool,
}

impl<'a, T: SSS, D: 'static> PickerUI<'a, T, D> {
    /// The nucleo item index and a reference to the data of the item currently
    /// under the cursor, if any.
    pub fn current_indexed(&self) -> Option<(u32, &T)> {
        self.worker.get_nth_indexed(self.results.index())
    }

    pub fn new(
        results_config: ResultsConfig,
        status_config: StatusConfig,
        input_config: QueryConfig,
        header_config: DisplayConfig,
        matcher: &'a mut nucleo::Matcher,
        mut worker: Worker<T, D>,
        selector: Selector,
    ) -> Self {
        let mut results = ResultsUI::new(results_config);
        results.init(&mut worker);

        Self {
            results,
            status: StatusUI::new(status_config),
            query: QueryUI::new(input_config),
            header: DisplayUI::new(header_config),
            matcher,
            selector,
            worker,
            filtering: true,
        }
    }

    /// Prefer [`crate::render::MMState::worker_restart`]
    pub fn restart(&mut self) {
        self.worker.restart(false);
        self.results.invalidate_widths();
        self.selector.clear();
    }

    pub(crate) fn active_column_index_raw(&self) -> usize {
        let cursor_byte = self.query.byte_index(self.query.cursor() as usize);

        self.worker.query.active_column_index(cursor_byte)
    }

    /// Get the active column index by checking the query
    ///
    /// We defaulting to empty column when non-filtering so that we can render for f:ist a certain way
    pub fn active_column_index(&self) -> usize {
        if !self.filtering {
            self.worker
                .query
                .empty_column_index()
                .unwrap_or(self.worker.query.primary_column_index())
        } else {
            self.active_column_index_raw()
        }
    }

    pub fn set_filtering(&mut self, s: Option<bool>) {
        if let Some(s) = s {
            self.filtering = s
        } else {
            self.filtering = !self.filtering
        }
        _trace!(self.filtering);
    }

    pub fn layout(&self, area: Rect) -> [Rect; 4] {
        let PickerUI {
            query,
            header,
            status,
            ..
        } = self;

        let mut constraints = [
            Constraint::Length(query.height()), // input
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

impl<'a, T: SSS, D: 'static> PickerUI<'a, T, D> {
    pub fn update(&mut self) {
        if self.filtering {
            self.worker.find(&self.query.input());
        }

        let active_column = self.active_column_index();
        self.results.update_active_column(active_column);
    }

    // creation from UI ensures Some
    pub fn reverse(&self) -> bool {
        self.results.reverse()
    }
}
