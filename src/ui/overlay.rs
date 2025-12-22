#![allow(unused)]
use ratatui::{Frame, layout::Rect};

use crate::{action::Action, config::OverlayConfig};

#[derive(Debug, Default)]
pub enum OverlayEffect {
    #[default]
    None,
    Stop,
}

#[derive(Default)]
pub struct OverlayUI {
    inner: Option<Box<dyn Overlay>>,
    config: OverlayConfig,
    cached_area: Rect,
}

impl OverlayUI {
    pub fn set(&mut self, overlay: Box<dyn Overlay>, ui_area: &Rect) {
        self.inner = Some(overlay);
        self.update_dimensions(ui_area);
    }
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_dimensions(&mut self, ui_area: &Rect) {
        if let Some(inner) = &self.inner {
            self.cached_area = inner
                .area(ui_area)
                .unwrap_or_else(|| self.default_area(ui_area));
        }
    }

    pub fn default_area(&self, ui_area: &Rect) -> Rect {
        let layout = &self.config.layout;
        // compute preferred size from percentage
        let mut w = layout.percentage[0].get_max(ui_area.width, 0);
        let mut h = layout.percentage[1].get_max(ui_area.height, 0);

        // clamp to min/max
        w = w.clamp(layout.min[0], layout.max[0]);
        h = h.clamp(layout.min[1], layout.max[1]);

        // center within ui_area
        let x = ui_area.x + (ui_area.width.saturating_sub(w)) / 2;
        let y = ui_area.y + (ui_area.height.saturating_sub(h)) / 2;

        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    pub fn draw(&self, frame: &mut Frame) {
        if let Some(overlay) = &self.inner {
            overlay.draw(frame, self.cached_area);
        }
    }
    pub fn is_active(&self) -> bool {
        self.inner.is_some()
    }
    pub fn disable(&mut self) {
        self.inner = None
    }

    /// Returns whether the overlay was active (handled the action)
    pub fn handle_input(&mut self, action: char) -> bool {
        if let Some(inner) = &self.inner {
            match inner.handle_input(action) {
                OverlayEffect::None => {}
                OverlayEffect::Stop => self.disable(),
            }
            true
        } else {
            false
        }
    }
}

pub trait Overlay {
    fn handle_input(&self, action: char) -> OverlayEffect;
    ///
    /// let widget = self.widget();
    /// frame.render_widget(widget, area)
    fn draw(&self, frame: &mut Frame, area: Rect);
    fn area(&self, ui_area: &Rect) -> Option<Rect> {
        None
    }
}

// ------------------------
// would be cool if associated types could be recovered from erased traits
// I think this can be done by wrapping overlay with a fn turning make_widget into draw
// type Widget: ratatui::widgets::Widget;
// fn make_widget(&self) -> Self::Widget {
//     todo!()
// }
// // OverlayUI
// pub fn draw(&self, frame: &mut Frame) {
//     if let Some(overlay) = &self.inner {
//         let widget = overlay.make_widget();
//         frame.render_widget(widget, overlay.area());
//     }
// }
