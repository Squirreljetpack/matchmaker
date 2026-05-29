use ratatui::style::{Color, Style};
use ratatui::widgets::Block;

use crate::config::OverlayLayoutSettings;
use crate::ui::{Frame, Rect, SizeHint};
use crate::utils::Percentage;

/// Dim the surroundings of the given area.
pub fn dim_surroundings(frame: &mut Frame, inner: Rect) {
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

pub fn default_area(size: [SizeHint; 2], layout: &OverlayLayoutSettings, ui_area: &Rect) -> Rect {
    let computed_w =
        layout.percentage[0].compute_clamped(ui_area.width, layout.min[0], layout.max[0]);

    let computed_h =
        layout.percentage[1].compute_clamped(ui_area.height, layout.min[1], layout.max[1]);

    let mut w = match size[0] {
        SizeHint::Exact(v) => v,
        SizeHint::Min(v) => v.max(computed_w),
        SizeHint::Max(v) => v.min(computed_w),
    }
    .min(ui_area.width);

    let mut h = match size[1] {
        SizeHint::Exact(v) => v,
        SizeHint::Min(v) => v.max(computed_h),
        SizeHint::Max(v) => v.min(computed_h),
    }
    .min(ui_area.height);

    if w == 0 && !matches!(size[0], SizeHint::Max(_)) {
        w = computed_w;
    }
    if h == 0 && !matches!(size[1], SizeHint::Max(_)) {
        h = computed_h;
    }

    let available_h = ui_area.height.saturating_sub(h);
    let offset = if layout.y_offset < Percentage::new(50) {
        let o = layout
            .y_offset
            .compute_clamped(available_h.saturating_sub(h), 0, 0);

        (available_h / 2).saturating_sub(o)
    } else {
        available_h / 2
            + layout
                .y_offset
                .saturating_sub(50)
                .compute_clamped(available_h, 0, 0)
    };

    let x = ui_area.x + (ui_area.width.saturating_sub(w)) / 2;
    let y = ui_area.y + offset;

    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

/// Helper to resize a Rect while preserving its center.
pub fn update_area(area: &mut Rect, w: Option<u16>, h: Option<u16>) {
    let center_x = area.x + area.width / 2;
    let center_y = area.y + area.height / 2;

    if let Some(new_w) = w {
        area.width = new_w;
    }
    if let Some(new_h) = h {
        area.height = new_h;
    }

    // preserve the original center
    area.x = center_x.saturating_sub(area.width / 2);
    area.y = center_y.saturating_sub(area.height / 2);
}
