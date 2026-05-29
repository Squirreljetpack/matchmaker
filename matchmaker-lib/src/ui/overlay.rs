use crate::action::{Action, ActionExt};
use crate::config::OverlayLayoutSettings;
use crate::ui::{Frame, Rect};

use crate::config::OverlayConfig;

#[derive(Debug, Default)]
pub enum OverlayEffect {
    #[default]
    None,
    Disable,
}

pub trait Overlay {
    type A: ActionExt;
    fn on_enable(&mut self, area: &Rect) {
        let _ = area;
    }
    fn on_disable(&mut self) {}
    fn handle_input(&mut self, c: char) -> OverlayEffect;
    fn handle_action(&mut self, action: &Action<Self::A>) -> OverlayEffect {
        let _ = action;
        OverlayEffect::None
    }

    // methods are mutable for flexibility (i.e. render_stateful_widget)

    /// Draw the widget within the rect
    ///
    /// # Example
    /// ```rust
    //  pub fn draw(&self, frame: &mut Frame) {
    //      let widget = self.make_widget();
    //      frame.render_widget(Clear, self.area);
    //      frame.render_widget(widget, self.area);
    // }
    /// ```
    fn draw(&mut self, frame: &mut Frame);

    /// Called when layout area changes.
    /// Implementation should compute and cache its area.
    fn area(&mut self, ui_area: &Rect, layout: &OverlayLayoutSettings);
}

/// If Exact(0), the default computed dimension is used (see [`OverlayConfig`] and [`crate::ui::utils::default_area`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeHint {
    Min(u16),
    Max(u16),
    Exact(u16),
}

impl From<u16> for SizeHint {
    fn from(value: u16) -> Self {
        SizeHint::Exact(value)
    }
}

// -------- OVERLAY_UI -----------

pub struct OverlayUI<A: ActionExt> {
    overlays: Box<[Box<dyn Overlay<A = A>>]>,
    index: Option<usize>,
    config: OverlayConfig,
}

impl<A: ActionExt> OverlayUI<A> {
    pub fn new(overlays: Box<[Box<dyn Overlay<A = A>>]>, config: OverlayConfig) -> Self {
        Self {
            overlays,
            index: None,
            config,
        }
    }

    pub fn index(&self) -> Option<usize> {
        self.index
    }

    pub fn enable(&mut self, index: usize, ui_area: &Rect) {
        assert!(index < self.overlays.len());
        self.index = Some(index);
        let overlay = &mut self.overlays[index];
        overlay.on_enable(ui_area);
        overlay.area(ui_area, &self.config.layout);
    }

    pub fn disable(&mut self) {
        if let Some(x) = self.current_mut() {
            x.on_disable()
        }
        self.index = None
    }

    pub fn current(&self) -> Option<&dyn Overlay<A = A>> {
        self.index
            .and_then(|i| self.overlays.get(i))
            .map(|b| b.as_ref())
    }

    fn current_mut(&mut self) -> Option<&mut Box<dyn Overlay<A = A> + 'static>> {
        if let Some(i) = self.index {
            self.overlays.get_mut(i)
        } else {
            None
        }
    }

    pub fn update_dimensions(&mut self, ui_area: &Rect) {
        if let Some(i) = self.index {
            let overlay = &mut self.overlays[i];
            overlay.area(ui_area, &self.config.layout);
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        if let Some(x) = self.current_mut() {
            x.draw(frame);
        }
    }

    /// Returns whether the overlay was active (handled the action)
    pub fn handle_input(&mut self, action: char) -> bool {
        if let Some(x) = self.current_mut() {
            match x.handle_input(action) {
                OverlayEffect::None => {}
                OverlayEffect::Disable => self.disable(),
            }
            true
        } else {
            false
        }
    }

    pub fn handle_action(&mut self, action: &Action<A>) -> bool {
        if let Some(inner) = self.current_mut() {
            match inner.handle_action(action) {
                OverlayEffect::None => {}
                OverlayEffect::Disable => self.disable(),
            }
            true
        } else {
            false
        }
    }
}
