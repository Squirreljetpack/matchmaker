use ratatui::style::{Color, Style};
use ratatui::widgets::Block;

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
    //  pub fn draw(&self, frame: &mut Frame, area: Rect) {
    //      let widget = self.make_widget();
    //      frame.render_widget(Clear, area);
    //      frame.render_widget(widget, area);
    // }
    /// ```
    fn draw(&mut self, frame: &mut Frame, area: Rect);

    /// Called when layout area changes.
    /// The output of this is processed and cached into the area which the draw method is called with.
    ///
    /// # Notes
    /// Return None or Err([0, 0]) to draw in the default area (see [`OverlayConfig`] and [`default_area`])
    fn area(&mut self, ui_area: &Rect) -> Result<Rect, [u16; 2]> {
        let _ = ui_area;
        Err([0, 0])
    }
}

// -------- OVERLAY_UI -----------

pub struct OverlayUI<A: ActionExt> {
    overlays: Box<[Box<dyn Overlay<A = A>>]>,
    index: Option<usize>,
    config: OverlayConfig,
    cached_area: Rect,
}

impl<A: ActionExt> OverlayUI<A> {
    pub fn new(overlays: Box<[Box<dyn Overlay<A = A>>]>, config: OverlayConfig) -> Self {
        Self {
            overlays,
            index: None,
            config,
            cached_area: Default::default(),
        }
    }

    pub fn index(&self) -> Option<usize> {
        self.index
    }

    pub fn enable(&mut self, index: usize, ui_area: &Rect) {
        assert!(index < self.overlays.len());
        self.index = Some(index);
        self.current_mut().unwrap().on_enable(ui_area);
        self.update_dimensions(ui_area);
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

    // ---------
    pub fn update_dimensions(&mut self, ui_area: &Rect) {
        if let Some(x) = self.current_mut() {
            self.cached_area = match x.area(ui_area) {
                Ok(x) => x,
                // centered
                Err(pref) => default_area(pref, &self.config.layout, ui_area),
            };
            log::debug!("Overlay area: {}", self.cached_area);
        }
    }

    // -----------

    pub fn draw(&mut self, frame: &mut Frame) {
        // Draw the overlay on top
        let area = self.cached_area;
        let outer_dim = self.config.outer_dim;

        if let Some(x) = self.current_mut() {
            if outer_dim {
                Self::dim_surroundings(frame, area)
            };
            x.draw(frame, area);
        }
    }

    // todo: bottom is missing + looks bad
    fn dim_surroundings(frame: &mut Frame, inner: Rect) {
        let full_area = frame.area();
        let dim_style = Style::default().bg(Color::Black).fg(Color::DarkGray);

        // Top
        if inner.y > 0 {
            let top = Rect {
                x: 0,
                y: 0,
                width: full_area.width,
                height: inner.y,
            };
            frame.render_widget(Block::default().style(dim_style), top);
        }

        // Bottom
        if inner.y + inner.height < full_area.height {
            let bottom = Rect {
                x: 0,
                y: inner.y + inner.height,
                width: full_area.width,
                height: full_area.height - (inner.y + inner.height),
            };
            frame.render_widget(Block::default().style(dim_style), bottom);
        }

        // Left
        if inner.x > 0 {
            let left = Rect {
                x: 0,
                y: inner.y,
                width: inner.x,
                height: inner.height,
            };
            frame.render_widget(Block::default().style(dim_style), left);
        }

        // Right
        if inner.x + inner.width < full_area.width {
            let right = Rect {
                x: inner.x + inner.width,
                y: inner.y,
                width: full_area.width - (inner.x + inner.width),
                height: inner.height,
            };
            frame.render_widget(Block::default().style(dim_style), right);
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

pub fn default_area(
    [mut w, mut h]: [u16; 2],
    layout: &OverlayLayoutSettings,
    ui_area: &Rect,
) -> Rect {
    // compute preferred size from percentage then clamp to min/max
    if w == 0 {
        w = layout.percentage[0].compute_with_clamp(ui_area.width, 0);
        w = w.clamp(layout.min[0], layout.max[0]);
    }
    if h == 0 {
        h = layout.percentage[1].compute_with_clamp(ui_area.height, 0);
        h = h.clamp(layout.min[1], layout.max[1]);
    }

    // center within ui_area
    let x = ui_area.x + (ui_area.width.saturating_sub(w)) / 2;
    let y = ui_area.y + (ui_area.height.saturating_sub(h + 8)) / 2; // bump up 4 lines nearer to top

    Rect {
        x,
        y,
        width: w,
        height: h,
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
