mod picker;

pub use picker::*;

use crate::action::{Action, ActionExt};
use crate::aliases::SSS;
use crate::config::OverlayLayoutSettings;
use crate::render::MMState;
use crate::ui::utils::AdaptivePercentage;
use crate::ui::{Frame, Rect};

use crate::config::OverlayConfig;

#[derive(Debug, Default)]
pub enum OverlayEffect {
    #[default]
    None,
    Disable,
}

/// Overlays receive the picker state (`MMState`) at their entry points so
/// they can read live selections, the cursor item, and the query without
/// snapshots.
pub trait Overlay<Act: ActionExt, T: SSS, D: 'static> {
    fn on_enable(&mut self, area: &Rect, state: &mut MMState<'_, '_, T, D>) {
        let _ = (area, state);
    }
    fn on_disable(&mut self) {}
    fn handle_input(&mut self, c: char, state: &mut MMState<'_, '_, T, D>) -> OverlayEffect;
    fn handle_action(
        &mut self,
        action: &Action<Act>,
        state: &mut MMState<'_, '_, T, D>,
    ) -> OverlayEffect {
        let _ = (action, state);
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

/// A size constraint for one axis of an overlay: the base size is interpolated
/// from `adaptive_percentage` against the axis size, or falls back to the
/// layout percentage when empty, then is clamped to `[min, max]` where `0`
/// means no clamp (see [`crate::ui::utils::default_area`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeHint {
    /// Percentage points keyed by axis size, in ascending order.
    pub adaptive_percentage: &'static AdaptivePercentage,
    /// Lower clamp; `0` = no clamp.
    pub min: u16,
    /// Upper clamp; `0` = no clamp.
    pub max: u16,
}

impl From<u16> for SizeHint {
    /// Exact size: both clamps equal to `value`.
    fn from(value: u16) -> Self {
        Self {
            adaptive_percentage: &[],
            min: value,
            max: value,
        }
    }
}

impl From<[u16; 2]> for SizeHint {
    /// `[min, max]` clamps over the computed base size.
    fn from([min, max]: [u16; 2]) -> Self {
        Self {
            adaptive_percentage: &[],
            min,
            max,
        }
    }
}

// -------- OVERLAY_UI -----------

pub struct OverlayUI<Act: ActionExt, T: SSS, D: 'static> {
    overlays: Box<[Box<dyn Overlay<Act, T, D>>]>,
    index: Option<usize>,
    config: OverlayConfig,
}

impl<Act: ActionExt, T: SSS, D: 'static> OverlayUI<Act, T, D> {
    pub fn new(overlays: Box<[Box<dyn Overlay<Act, T, D>>]>, config: OverlayConfig) -> Self {
        Self {
            overlays,
            index: None,
            config,
        }
    }

    pub fn index(&self) -> Option<usize> {
        self.index
    }

    pub fn enable(&mut self, index: usize, ui_area: &Rect, state: &mut MMState<'_, '_, T, D>) {
        assert!(index < self.overlays.len());
        self.index = Some(index);
        let overlay = &mut self.overlays[index];
        overlay.on_enable(ui_area, state);
        overlay.area(ui_area, &self.config.layout);
    }

    pub fn disable(&mut self) {
        if let Some(x) = self.current_mut() {
            x.on_disable()
        }
        self.index = None
    }

    pub fn current(&self) -> Option<&dyn Overlay<Act, T, D>> {
        self.index
            .and_then(|i| self.overlays.get(i))
            .map(|b| b.as_ref())
    }

    fn current_mut(&mut self) -> Option<&mut Box<dyn Overlay<Act, T, D> + 'static>> {
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
    pub fn handle_input(&mut self, action: char, state: &mut MMState<'_, '_, T, D>) -> bool {
        if let Some(x) = self.current_mut() {
            match x.handle_input(action, state) {
                OverlayEffect::None => {}
                OverlayEffect::Disable => self.disable(),
            }
            true
        } else {
            false
        }
    }

    pub fn handle_action(
        &mut self,
        action: &Action<Act>,
        state: &mut MMState<'_, '_, T, D>,
    ) -> bool {
        if let Some(inner) = self.current_mut() {
            match inner.handle_action(action, state) {
                OverlayEffect::None => {}
                OverlayEffect::Disable => self.disable(),
            }
            true
        } else {
            false
        }
    }
}
