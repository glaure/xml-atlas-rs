//! Scrolling by whole rows (`docs/decisions.md`, D16; `docs/spec/text.md`, TEXT-8).
//!
//! egui keeps a scroll offset as a 32-bit float of pixels. Seven million lines are about 119
//! million pixels, where a float steps by 8 px, so small wheel steps are lost and single
//! lines cannot be reached. Here the position is the top row as an integer plus the pixels
//! of it scrolled out of sight, and the scroll bars are drawn here too.

use egui::{Id, Rect, Sense, Ui, pos2, vec2};

/// Width of a scroll bar.
pub const BAR: f32 = 10.0;

/// A scroll bar's thumb is never shorter than this.
const MIN_THUMB: f32 = 24.0;

/// The vertical position: row `top` is the first one shown, `px` pixels of it above the view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowScroll {
    pub top: usize,
    pub px: f32,
}

impl RowScroll {
    /// The last row that can be at the top: the one that shows the last row at the bottom.
    pub fn max_top(rows: usize, visible: usize) -> usize {
        rows.saturating_sub(visible.max(1))
    }

    /// Moves the view down by `dy` pixels (up when negative), by whole rows and the rest.
    /// Every step counts, however far down the view is.
    pub fn by(&mut self, dy: f32, row_h: f32, max_top: usize) {
        let mut px = self.px + dy;
        let rows = (px / row_h).floor();
        px -= rows * row_h;
        let top = self.top as i64 + rows as i64;
        if top < 0 {
            *self = Self::default();
        } else if top as usize >= max_top {
            *self = Self {
                top: max_top,
                px: 0.0,
            };
        } else {
            *self = Self {
                top: top as usize,
                px,
            };
        }
    }

    /// Shows row `top` at the top, as far as the end allows.
    pub fn to(&mut self, top: usize, max_top: usize) {
        *self = Self {
            top: top.min(max_top),
            px: 0.0,
        };
    }

    /// How far down the view is, from 0 at the start to 1 at the end.
    fn fraction(&self, max_top: usize) -> f64 {
        if max_top == 0 {
            0.0
        } else {
            (self.top as f64 / max_top as f64).min(1.0)
        }
    }
}

/// Where the thumb of a bar of `track` pixels is, from its start, and how long it is, for
/// `shown` of `total` units scrolled `fraction` of the way.
fn thumb(track: f32, shown: f64, total: f64, fraction: f64) -> (f32, f32) {
    let len = if total <= shown {
        track
    } else {
        ((track as f64 * shown / total) as f32).clamp(MIN_THUMB.min(track), track)
    };
    ((track - len) * fraction as f32, len)
}

/// The fraction of the way a thumb of `len` pixels whose start is at `at` stands for.
fn fraction_at(track: f32, len: f32, at: f32) -> f64 {
    let room = track - len;
    if room <= 0.0 {
        0.0
    } else {
        (at / room).clamp(0.0, 1.0) as f64
    }
}

/// The vertical scroll bar in `rect`, for `rows` rows of which `visible` fit. Dragging the
/// thumb moves the view; a click beside it moves it by a page.
pub fn vertical_bar(ui: &Ui, rect: Rect, id: Id, rows: usize, visible: usize, s: &mut RowScroll) {
    let max_top = RowScroll::max_top(rows, visible);
    let (at, len) = thumb(
        rect.height(),
        visible as f64,
        rows as f64,
        s.fraction(max_top),
    );
    let thumb_rect = Rect::from_min_size(pos2(rect.left(), rect.top() + at), vec2(BAR, len));
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    if let Some(p) = resp.interact_pointer_pos() {
        let grab = id.with("grab");
        if resp.drag_started() {
            let offset = if thumb_rect.contains(p) {
                p.y - thumb_rect.top()
            } else {
                len / 2.0
            };
            ui.data_mut(|d| d.insert_temp(grab, offset));
        }
        if resp.dragged() {
            let offset = ui.data(|d| d.get_temp::<f32>(grab)).unwrap_or(len / 2.0);
            let f = fraction_at(rect.height(), len, p.y - rect.top() - offset);
            s.to((f * max_top as f64).round() as usize, max_top);
        } else if resp.clicked() && !thumb_rect.contains(p) {
            let page = visible.saturating_sub(1).max(1);
            let top = if p.y < thumb_rect.top() {
                s.top.saturating_sub(page)
            } else {
                s.top + page
            };
            s.to(top, max_top);
        }
    }
    paint_bar(ui, rect, thumb_rect, resp.hovered() || resp.dragged());
}

/// The horizontal scroll bar in `rect`, for content `width` pixels wide of which `view`
/// show; `left` is how far it is scrolled.
pub fn horizontal_bar(ui: &Ui, rect: Rect, id: Id, width: f32, view: f32, left: &mut f32) {
    let room = (width - view).max(0.0);
    let fraction = if room > 0.0 {
        (*left / room) as f64
    } else {
        0.0
    };
    let (at, len) = thumb(rect.width(), view as f64, width as f64, fraction);
    let thumb_rect = Rect::from_min_size(pos2(rect.left() + at, rect.top()), vec2(len, BAR));
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    if let Some(p) = resp.interact_pointer_pos() {
        let grab = id.with("grab");
        if resp.drag_started() {
            let offset = if thumb_rect.contains(p) {
                p.x - thumb_rect.left()
            } else {
                len / 2.0
            };
            ui.data_mut(|d| d.insert_temp(grab, offset));
        }
        if resp.dragged() {
            let offset = ui.data(|d| d.get_temp::<f32>(grab)).unwrap_or(len / 2.0);
            *left = fraction_at(rect.width(), len, p.x - rect.left() - offset) as f32 * room;
        } else if resp.clicked() && !thumb_rect.contains(p) {
            let step = if p.x < thumb_rect.left() { -view } else { view };
            *left = (*left + step).clamp(0.0, room);
        }
    }
    paint_bar(ui, rect, thumb_rect, resp.hovered() || resp.dragged());
}

/// Draws a bar's track and thumb in the theme's colours.
fn paint_bar(ui: &Ui, track: Rect, thumb: Rect, active: bool) {
    let v = ui.visuals();
    let painter = ui.painter();
    painter.rect_filled(track, 0.0, v.extreme_bg_color);
    let fill = if active {
        v.widgets.active.bg_fill
    } else {
        v.widgets.inactive.bg_fill
    };
    painter.rect_filled(thumb.shrink(2.0), 3.0, fill);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 300 MiB fixture: seven million lines of 17 px.
    const ROWS: usize = 7_000_000;
    const ROW_H: f32 = 17.0;

    /// Far down, where egui lost one wheel notch in six, every notch moves by the same amount
    /// and back again (TEXT-8).
    #[test]
    fn every_notch_counts_far_down() {
        let max_top = RowScroll::max_top(ROWS, 30);
        let mut s = RowScroll {
            top: ROWS - 1000,
            px: 0.0,
        };
        let start = s;
        for _ in 0..50 {
            // A notch, as egui hands it out over several frames.
            for step in [13.0, 13.0, 13.0, 13.0, 13.0, 15.0] {
                s.by(step, ROW_H, max_top);
            }
        }
        // 50 notches of 80 px.
        let moved = (s.top - start.top) as f32 * ROW_H + s.px;
        assert!((moved - 4000.0).abs() < 0.01, "moved {moved}");
        for _ in 0..50 {
            s.by(-80.0, ROW_H, max_top);
        }
        assert_eq!(s.top, start.top);
        assert!(s.px.abs() < 0.01);
    }

    #[test]
    fn stops_at_both_ends() {
        let max_top = RowScroll::max_top(100, 30);
        assert_eq!(max_top, 70);
        let mut s = RowScroll::default();
        s.by(-50.0, ROW_H, max_top);
        assert_eq!(s, RowScroll::default());
        s.by(1e9, ROW_H, max_top);
        assert_eq!(s, RowScroll { top: 70, px: 0.0 });
        s.to(5, max_top);
        assert_eq!(s.top, 5);
        s.to(ROWS, max_top);
        assert_eq!(s.top, 70);
        // A file shorter than the view does not scroll.
        assert_eq!(RowScroll::max_top(10, 30), 0);
    }

    /// The thumb goes from the top to the bottom of its track, and dragging it to a place
    /// and back reads the same fraction; the last line can be reached by dragging.
    #[test]
    fn thumb_maps_both_ways() {
        let (at, len) = thumb(500.0, 30.0, ROWS as f64, 0.0);
        assert_eq!((at, len), (0.0, MIN_THUMB));
        let (at, _) = thumb(500.0, 30.0, ROWS as f64, 1.0);
        assert_eq!(at, 500.0 - MIN_THUMB);
        for f in [0.0, 0.25, 0.5, 1.0] {
            let (at, len) = thumb(500.0, 30.0, ROWS as f64, f);
            assert!((fraction_at(500.0, len, at) - f).abs() < 1e-6);
        }
        assert_eq!(fraction_at(500.0, MIN_THUMB, 9999.0), 1.0);
        // Everything fits: the thumb fills the track.
        assert_eq!(thumb(500.0, 30.0, 10.0, 0.0), (0.0, 500.0));
    }
}
