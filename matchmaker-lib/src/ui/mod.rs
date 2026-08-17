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

pub const RESULTS_MIN_W: u16 = 5;
pub const RESULTS_MIN_H: u16 = 1;

use crate::{
    SSS, Selector,
    config::{
        BorderSetting, DisplayConfig, QueryConfig, RenderConfig, ResultsConfig, StatusConfig,
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
    /// Full picker pane rect including its border, set by update_layout_and_state.
    picker_area: Rect,
    pub config: UiConfig,
}

// requires columns > 1
impl UI {
    pub fn new<T: SSS, D: 'static, W: std::io::Write>(
        mut config: RenderConfig,
        worker: Worker<T, D>,
        selector: Selector,
        view: Option<Preview>,
        tui: &mut Tui<W>,
    ) -> (Self, PickerUI<T, D>, DisplayUI, Option<PreviewUI>) {
        assert!(!worker.columns.is_empty());

        if config.results.reverse.is_none() {
            config.results.reverse = (
                tui.is_fullscreen() && tui.area.y < tui.area.height / 2
                // reverse if fullscreen + cursor is in lower half of the screen
            )
            .into()
        }

        let ui_area = [
            tui.area
                .width
                .saturating_sub(config.ui.outer_border.width()),
            tui.area
                .height
                .saturating_sub(config.ui.outer_border.height()),
        ];

        let area = Rect {
            x: tui.area.x + config.ui.outer_border.left(),
            y: tui.area.y + config.ui.outer_border.top(),
            width: ui_area[0],
            height: ui_area[1],
        };

        let ui = Self {
            layout: tui.config.layout.clone(),
            area,
            picker_area: area,
            config: config.ui,
        };

        let picker = PickerUI::new(
            config.results,
            config.status,
            config.query,
            config.header,
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
    pub fn new_offline<T: SSS, D: 'static>(
        config: RenderConfig,
        worker: Worker<T, D>,
    ) -> (Self, PickerUI<T, D>) {
        assert!(!worker.columns.is_empty());

        let ui = Self {
            layout: None,
            area: Rect::default(),
            picker_area: Rect::default(),
            config: config.ui,
        };

        let picker = PickerUI::new(
            config.results,
            config.status,
            config.query,
            config.header,
            worker,
            Selector::new(),
        );

        (ui, picker)
    }

    pub fn update_dimensions(&mut self, area: Rect) {
        self.area = self.outer_border().inner_of(area);
    }

    pub fn make_ui(&self) -> ratatui::widgets::Block<'_> {
        self.config.outer_border.as_block()
    }

    pub fn outer_border(&self) -> &BorderSetting {
        &self.config.outer_border
    }

    pub fn border(&self) -> &BorderSetting {
        &self.config.border
    }

    pub fn picker_area(&self) -> Rect {
        self.picker_area
    }

    pub fn update_picker_area(&mut self, area: Rect) {
        self.picker_area = area;
    }

    pub fn area(&self) -> Rect {
        self.area
    }

    pub fn full_area(&self) -> Rect {
        let outer_border = &self.config.outer_border;
        Rect {
            x: self.area.x - outer_border.left(),
            y: self.area.y - outer_border.top(),
            width: self.area.width + outer_border.width(),
            height: self.area.height + outer_border.height(),
        }
    }
}

pub struct PickerUI<T: SSS, D> {
    pub results: ResultsUI,
    pub status: StatusUI,
    pub query: QueryUI,
    pub header: DisplayUI,
    pub selector: Selector,
    pub worker: Worker<T, D>,
    pub filtering: bool,
}

impl<T: SSS, D: 'static> PickerUI<T, D> {
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
            Constraint::Length(query.height()),                   // input
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

impl<T: SSS, D: 'static> PickerUI<T, D> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, widgets::Borders};

    fn test_ui(config: RenderConfig) -> (UI, PickerUI<&'static str, ()>) {
        let worker = Worker::<&'static str, ()>::new_single_column();
        UI::new_offline(config, worker)
    }

    #[test]
    fn full_area_roundtrips_outer_area() {
        let mut config = RenderConfig::default();
        config.ui.outer_border = BorderSetting {
            sides: Some(Borders::ALL),
            ..Default::default()
        };
        config.ui.border = BorderSetting {
            sides: Some(Borders::ALL),
            ..Default::default()
        };
        let (mut ui, _picker) = test_ui(config);

        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        ui.update_dimensions(area);
        assert_eq!(ui.full_area(), area);
    }

    #[test]
    fn make_ui_renders_outer_border() {
        let mut config = RenderConfig::default();
        config.ui.outer_border = BorderSetting {
            sides: Some(Borders::ALL),
            ..Default::default()
        };
        let (ui, _picker) = test_ui(config);

        let block = ui.make_ui();
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(block, frame.area()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), "┌");
        assert_eq!(buffer[(9, 3)].symbol(), "┘");
    }

    #[test]
    fn picker_layout_stacks_sections() {
        let (_ui, picker) = test_ui(RenderConfig::default());

        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let [input, status, header, results] = picker.layout(area);

        assert_eq!(
            input,
            Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 1
            }
        );
        assert_eq!(
            status,
            Rect {
                x: 0,
                y: 1,
                width: 80,
                height: 1
            }
        );
        assert_eq!(
            header,
            Rect {
                x: 0,
                y: 2,
                width: 80,
                height: 0
            }
        );
        assert_eq!(
            results,
            Rect {
                x: 0,
                y: 2,
                width: 80,
                height: 22
            }
        );
    }

    #[test]
    fn picker_layout_reversed() {
        let mut config = RenderConfig::default();
        config.results.reverse = Some(true);
        let (_ui, picker) = test_ui(config);

        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let [input, status, header, results] = picker.layout(area);

        // reversed: results at the top, input at the bottom
        assert_eq!(
            results,
            Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 22
            }
        );
        assert_eq!(
            header,
            Rect {
                x: 0,
                y: 22,
                width: 80,
                height: 0
            }
        );
        assert_eq!(
            status,
            Rect {
                x: 0,
                y: 22,
                width: 80,
                height: 1
            }
        );
        assert_eq!(
            input,
            Rect {
                x: 0,
                y: 23,
                width: 80,
                height: 1
            }
        );
    }
}
