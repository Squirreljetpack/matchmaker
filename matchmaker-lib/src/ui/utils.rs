use ratatui::style::{Color, Style};
use ratatui::widgets::Block;

use crate::config::OverlayLayoutSettings;
use crate::ui::{Frame, Rect, SizeHint};
use crate::utils::Percentage;

/// Adaptive sizing points: `(axis size, percentage)` pairs in ascending axis
/// size order.
pub type AdaptivePercentage = [(u16, Percentage)];

/// Linearly interpolates the percentage at `size` from `points`.
///
/// Clamps to the first point's percentage below the first size and to the
/// last point's percentage past the last size. Returns `0%` for empty input.
///
/// # Example
/// `[(80, 100%), (120, 80%)]` yields `90%` at size 100 and `80%` at size 125.
pub fn compute_adaptive_percentage(points: &AdaptivePercentage, size: u16) -> Percentage {
    let Some(&(first_size, first_pct)) = points.first() else {
        return Percentage::new(0);
    };
    if size <= first_size {
        return first_pct;
    }
    let Some(&(last_size, last_pct)) = points.last() else {
        return first_pct;
    };
    if size >= last_size {
        return last_pct;
    }

    let (lo_size, lo_pct, hi_size, hi_pct) = points
        .windows(2)
        .map(|w| (w[0].0, w[0].1, w[1].0, w[1].1))
        .find(|(lo, _, hi, _)| *lo <= size && size <= *hi)
        .unwrap_or((first_size, first_pct, last_size, last_pct));

    let span = hi_size - lo_size;
    if span == 0 {
        return hi_pct;
    }
    let t = u32::from(size - lo_size);
    let span = u32::from(span);
    let lo = u32::from(lo_pct.inner());
    let hi = u32::from(hi_pct.inner());

    Percentage::new(((lo * (span - t) + hi * t) / span) as u16)
}

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
    let computed_w = if size[0].adaptive_percentage.is_empty() {
        layout.percentage[0].compute_clamped(ui_area.width, layout.min[0], layout.max[0])
    } else {
        compute_adaptive_percentage(size[0].adaptive_percentage, ui_area.width).compute_clamped(
            ui_area.width,
            0,
            0,
        )
    };

    let computed_h =
        if size[1].adaptive_percentage.is_empty() {
            layout.percentage[1].compute_clamped(ui_area.height, layout.min[1], layout.max[1])
        } else {
            compute_adaptive_percentage(size[1].adaptive_percentage, ui_area.height)
                .compute_clamped(ui_area.height, 0, 0)
        };

    let mut w = computed_w;
    if size[0].max != 0 {
        w = w.min(size[0].max);
    }
    if size[0].min != 0 {
        w = w.max(size[0].min);
    }

    let mut h = computed_h;
    if size[1].max != 0 {
        h = h.min(size[1].max);
    }
    if size[1].min != 0 {
        h = h.max(size[1].min);
    }

    w = w.min(ui_area.width);
    h = h.min(ui_area.height);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_percentage_interpolates_and_clamps() {
        let points: &AdaptivePercentage = &[(80, Percentage::new(100)), (120, Percentage::new(80))];

        assert_eq!(compute_adaptive_percentage(points, 100).inner(), 90);
        assert_eq!(compute_adaptive_percentage(&points, 80).inner(), 100);
        assert_eq!(compute_adaptive_percentage(&points, 60).inner(), 100);
        assert_eq!(compute_adaptive_percentage(&points, 120).inner(), 80);
        assert_eq!(compute_adaptive_percentage(&points, 125).inner(), 80);
        assert_eq!(compute_adaptive_percentage(&[], 100).inner(), 0);
    }

    #[test]
    fn single_point_is_constant() {
        let points: &AdaptivePercentage = &[(40, Percentage::new(50))];
        assert_eq!(compute_adaptive_percentage(points, 20).inner(), 50);
        assert_eq!(compute_adaptive_percentage(points, 100).inner(), 50);
    }

    #[test]
    fn size_hint_froms_keep_old_behavior() {
        let exact: SizeHint = 12.into();
        assert_eq!(exact.min, 12);
        assert_eq!(exact.max, 12);

        let min_max: SizeHint = [8, 30].into();
        assert_eq!(min_max.min, 8);
        assert_eq!(min_max.max, 30);
    }
}
