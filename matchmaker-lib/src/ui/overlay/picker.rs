use std::{marker::PhantomData, sync::Arc};

use ratatui::widgets::Clear;

use crate::{
    action::{Action, ActionExt, NullActionExt},
    config::{
        CursorSetting, OverlayConfig, OverlayLayoutSettings, QueryConfig, ResultsConfig,
        RowConnectionStyle,
    },
    nucleo::{Column, Worker},
    ui::{
        utils::{default_area, dim_surroundings},
        Constraint, Direction, Frame, Layout, Overlay, OverlayEffect, QueryUI, Rect, ResultsUI,
        SizeHint,
    },
    Selector,
};

/// A self-contained fuzzy picker overlay.
///
/// A miniature of the main picker: it owns its own [`QueryUI`], [`ResultsUI`],
/// a private [`Worker`] (a separate `nucleo` instance from the main picker's,
/// running on its own matcher thread), and a [`Selector`]. The overlay is
/// generic over the picker's action type `A` only — the item type is fixed to
/// `(String, String)` (two columns) for now; generalize by making the struct
/// generic over `T`/`D` if a different item type is needed.
///
/// The worker is built lazily in [`Overlay::on_enable`] (items are injected
/// then) and dropped in [`Overlay::on_disable`], so the matcher thread only
/// lives while the overlay is active.
///
/// # Example
/// ```rust
/// use matchmaker::{
///     action::Action,
///     binds::{bindmap, key},
///     config::{OverlayConfig, QueryConfig, ResultsConfig},
///     PickOptions,
///     action::NullActionExt,
///     ui::PickerOverlay,
/// };
///
/// let opts: PickOptions<'_, String, (), NullActionExt> = PickOptions::new()
///     .overlay(PickerOverlay::new(
///         vec![("foo".into(), "bar".into())],
///         OverlayConfig::default(),
///         ResultsConfig::default(),
///         QueryConfig::default(),
///     ))
///     .binds(bindmap!(key!(ctrl-o) => Action::Overlay(0)));
/// ```
pub struct PickerOverlay<A: ActionExt = NullActionExt> {
    query: QueryUI,
    results: ResultsUI,
    /// Lazily built on enable; `None` while inactive.
    worker: Option<Worker<(String, String), ()>>,
    selector: Selector,
    matcher: nucleo::Matcher,
    /// Dummy two-column content, injected into the worker on enable.
    items: Vec<(String, String)>,
    /// Cached overlay area (set by [`Overlay::area`]).
    area: Rect,
    config: OverlayConfig,
    /// Set whenever the query text changed; drives `worker.find` on the next draw.
    query_dirty: bool,
    _a: PhantomData<A>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OverlayConfig;
    use ratatui::{backend::TestBackend, Terminal};

    const UI_AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };

    fn test_overlay() -> PickerOverlay<NullActionExt> {
        PickerOverlay::new(
            vec![
                ("foo".to_string(), "detail 1".to_string()),
                ("bar".to_string(), "detail 2".to_string()),
                ("baz".to_string(), "detail 3".to_string()),
            ],
            OverlayConfig::default(),
            ResultsConfig::default(),
            QueryConfig::default(),
        )
    }

    #[test]
    fn draws_without_panicking() {
        let mut overlay = test_overlay();
        overlay.on_enable(&UI_AREA);
        overlay.area(&UI_AREA, &OverlayConfig::default().layout);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| overlay.draw(frame))
            .expect("overlay draw");

        // Navigation moves the cursor within bounds and exposes the current item.
        overlay.handle_action(&Action::Down(1));
        assert_eq!(overlay.results.index(), 1);
        let (idx, item) = overlay.current_item().expect("current item");
        assert_eq!(idx, 1);
        assert_eq!(item.0, "bar");

        overlay.handle_action(&Action::Down(10));
        assert!(overlay.current_item().is_some());
        overlay.handle_action(&Action::Up(10));
        assert_eq!(overlay.current_item().unwrap().1 .0, "foo");
    }
}

impl<A: ActionExt> PickerOverlay<A> {
    pub fn new(
        items: impl IntoIterator<Item = (String, String)>,
        config: OverlayConfig,
        results_config: ResultsConfig,
        query_config: QueryConfig,
    ) -> Self {
        Self {
            query: QueryUI::new(query_config),
            results: ResultsUI::new(results_config),
            worker: None,
            selector: Selector::new(),
            matcher: nucleo::Matcher::new(nucleo::Config::DEFAULT),
            items: items.into_iter().collect(),
            area: Rect::default(),
            config,
            query_dirty: true,
            _a: PhantomData,
        }
    }

    /// The nucleo index and data of the item currently under the cursor, if any.
    ///
    /// This is how the current item is accessed, e.g. on `Accept`:
    /// `self.worker.get_nth_indexed(self.results.index())`.
    pub fn current_item(&self) -> Option<(u32, &(String, String))> {
        let worker = self.worker.as_ref()?;
        worker.get_nth_indexed(self.results.index())
    }

    fn build_worker(&mut self) {
        debug_assert!(self.worker.is_none());
        let raw_preprocessor = Arc::new(|_: &(String, String)| Some(()));
        let text_preprocessor = Arc::new(|_: &(String, String)| ());
        let columns = [
            Column::new("name", |item: &(String, String), _: &()| {
                item.0.clone().into()
            })
            .with_raw(|item: &(String, String), _: &()| item.0.clone().into()),
            Column::new("description", |item: &(String, String), _: &()| {
                item.1.clone().into()
            })
            .with_raw(|item: &(String, String), _: &()| item.1.clone().into()),
        ];

        let mut worker = Worker::new(columns, 0, raw_preprocessor, text_preprocessor);
        worker.append(self.items.iter().cloned());
        self.results.init(&mut worker);
        self.worker = Some(worker);
    }
}

impl<A: ActionExt> Overlay for PickerOverlay<A> {
    type A = A;

    fn on_enable(&mut self, _area: &Rect) {
        if self.worker.is_none() {
            self.build_worker();
        }
        self.query_dirty = true;
    }

    fn on_disable(&mut self) {
        // Stops the matcher thread; the worker is rebuilt on the next enable.
        self.worker = None;
    }

    fn handle_input(&mut self, c: char) -> OverlayEffect {
        self.query.push_char(c);
        self.query_dirty = true;
        OverlayEffect::None
    }

    fn handle_action(&mut self, action: &Action<Self::A>) -> OverlayEffect {
        match action {
            Action::Up(n) => {
                for _ in 0..*n {
                    self.results.cursor_prev();
                }
            }
            Action::Down(n) => {
                for _ in 0..*n {
                    self.results.cursor_next();
                }
            }
            Action::Accept => {
                // Access the current item: the index into the matched snapshot
                // plus the item data. This is the placeholder accept path.
                let Some((idx, item)) = self.current_item() else {
                    return OverlayEffect::Disable;
                };
                todo!("accept {idx}: {item:?}");
            }
            Action::Quit(_) => return OverlayEffect::Disable,

            // Edit actions, mirrored from the main dispatch (render/mod.rs)
            Action::SetQuery(context) => self.query.set(context.clone(), u16::MAX),
            Action::InsertQuery(context) => self.query.insert_str(context),
            Action::ForwardChar => self.query.forward_char(),
            Action::BackwardChar => self.query.backward_char(),
            Action::ForwardWord => self.query.forward_word(),
            Action::BackwardWord => self.query.backward_word(),
            Action::DeleteChar => self.query.delete(),
            Action::DeleteWord => self.query.delete_word(),
            Action::DeleteLineStart => self.query.delete_line_start(),
            Action::DeleteLineEnd => self.query.delete_line_end(),
            Action::ClearQuery => self.query.clear(),
            _ => return OverlayEffect::None,
        }
        if matches!(
            action,
            Action::SetQuery(_)
                | Action::InsertQuery(_)
                | Action::DeleteChar
                | Action::DeleteWord
                | Action::DeleteLineStart
                | Action::DeleteLineEnd
                | Action::ClearQuery
        ) {
            self.query_dirty = true;
        }
        OverlayEffect::None
    }

    fn draw(&mut self, frame: &mut Frame) {
        let Some(worker) = self.worker.as_mut() else {
            return;
        };

        // Same update pipeline as the main picker: find -> active column -> table.
        if self.query_dirty {
            worker.find(&self.query.input());
            self.query_dirty = false;
        }
        let cursor_byte = self.query.byte_index(self.query.cursor() as usize);
        self.results
            .update_active_column(worker.query.active_column_index(cursor_byte));
        self.results
            .update_table(worker, &self.selector, &mut self.matcher);

        if self.config.outer_dim {
            dim_surroundings(frame, self.area);
        }

        frame.render_widget(Clear, self.area);
        frame.render_widget(self.config.border.as_block(), self.area);

        let inner = self.config.border.inner_of(self.area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1 + self.query.config.border.height()),
                Constraint::Fill(1),
            ])
            .split(inner);
        let input = chunks[0];
        let results = chunks[1];

        // Query input (mirrors render::render_input)
        let p = self.query.cursor_offset(&input);
        if let CursorSetting::Default = self.query.config.cursor {
            frame.set_cursor_position(p);
        }
        frame.render_widget(self.query.make_input(), input);

        // Results (mirrors render::render_results)
        let (table, width) = self.results.get_table();
        let mut results_area = results;
        if matches!(
            self.results.config.row_connection,
            RowConnectionStyle::Capped
        ) {
            results_area.width = results_area.width.min(width);
        }
        frame.render_widget(table, results_area);
    }

    fn area(&mut self, ui_area: &Rect, layout: &OverlayLayoutSettings) {
        self.area = default_area([SizeHint::Exact(0), SizeHint::Exact(0)], layout, ui_area);

        let inner = self.config.border.inner_of(self.area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1 + self.query.config.border.height()),
                Constraint::Fill(1),
            ])
            .split(inner);
        let input = chunks[0];
        let results = chunks[1];
        self.query.update_width(input.width);
        self.results.update_dimensions(results);
    }
}
