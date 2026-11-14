//! Read-only source view. Only the visible lines are laid out each frame.

use std::ops::Range;

use egui::text::CCursor;
use egui::{Align2, Color32, FontId, Galley, Painter, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::app::Open;
use crate::highlight::{Palette, line_job};

/// Lines longer than this are cut for display. Minified one-line files stay responsive;
/// wrapping and horizontal paging of huge lines are on the roadmap.
const MAX_LINE_BYTES: usize = 4000;

/// A tab is shown as this many spaces (TEXT-4).
const TAB_WIDTH: usize = 4;

pub fn show(ui: &mut Ui, open: &mut Open) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let font = FontId::monospace(13.0);
    let row_h = ui.fonts_mut(|f| f.row_height(&font)).ceil() + 1.0;
    let digits = open.doc.line_count().to_string().len() as f32;
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
    let gutter = digits * char_w + 16.0;
    let palette = Palette::new(ui.visuals().dark_mode);
    let visuals = ui.visuals().clone();

    // The selected element from its `<` to its last `>` (TEXT-6), and inside it what was
    // clicked in the grid or tree (TEXT-11).
    let selected = open.selected.map(|id| {
        let n = open.doc.index.node(id);
        n.start..n.end
    });
    let mark = open.mark.clone();
    let selected_fill = visuals.selection.bg_fill.gamma_multiply(0.35);
    let mark_fill = visuals.selection.bg_fill.gamma_multiply(0.7);
    let mark_stroke = Stroke::new(1.0, visuals.selection.stroke.color.gamma_multiply(0.8));

    let mut area = egui::ScrollArea::both().id_salt("text").auto_shrink(false);
    if let Some(line) = open.scroll_text_to.take() {
        area = area.vertical_scroll_offset(((line as f32) - 3.0).max(0.0) * row_h);
    }
    let mut clicked = None;
    area.show_rows(ui, row_h, open.doc.line_count(), |ui, range| {
        for n in range {
            let full = open.doc.line(n);
            let raw = &full[..full.len().min(MAX_LINE_BYTES)];
            let line = String::from_utf8_lossy(raw).replace('\t', &" ".repeat(TAB_WIDTH));
            let galley = ui.painter().layout_job(line_job(&line, &font, &palette));
            let width = (gutter + galley.size().x + 8.0).max(ui.available_width());
            let (rect, resp) = ui.allocate_exact_size(vec2(width, row_h), Sense::click());
            let painter = ui.painter();
            let origin = pos2(rect.left() + gutter, rect.top());
            let line_bytes = open.doc.line_starts[n]..open.doc.line_starts[n] + full.len() as u64;
            if let Some(r) = &selected {
                paint_span(
                    painter,
                    &galley,
                    origin,
                    rect,
                    raw,
                    &line_bytes,
                    r,
                    selected_fill,
                    None,
                );
            }
            if let Some(r) = &mark {
                let stroke = Some(mark_stroke);
                paint_span(
                    painter,
                    &galley,
                    origin,
                    rect,
                    raw,
                    &line_bytes,
                    r,
                    mark_fill,
                    stroke,
                );
            }
            painter.text(
                pos2(rect.left() + gutter - 8.0, rect.center().y),
                Align2::RIGHT_CENTER,
                (n + 1).to_string(),
                font.clone(),
                visuals.weak_text_color(),
            );
            painter.galley(origin, galley, visuals.text_color());
            if resp.clicked() {
                let indent = raw.iter().take_while(|b| b.is_ascii_whitespace()).count();
                clicked = Some(open.doc.line_starts[n] + indent as u64);
            }
        }
    });
    if let Some(offset) = clicked
        && let Some(id) = open.element_at(offset)
    {
        open.select(id);
        // The click came from the text view; don't jump it.
        open.scroll_text_to = None;
    }
}

/// Fills the part of `span` that lies on this line.
#[allow(clippy::too_many_arguments)]
fn paint_span(
    painter: &Painter,
    galley: &Galley,
    origin: egui::Pos2,
    row: Rect,
    raw: &[u8],
    line: &Range<u64>,
    span: &Range<u64>,
    fill: Color32,
    stroke: Option<Stroke>,
) {
    let Some((a, b)) = span_columns(raw, line, span) else {
        return;
    };
    let x = |col| origin.x + galley.pos_from_cursor(CCursor::new(col)).left();
    let r = Rect::from_x_y_ranges(x(a) - 1.0..=x(b) + 1.0, row.y_range());
    painter.rect_filled(r, 2.0, fill);
    if let Some(stroke) = stroke {
        painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
    }
}

/// Display columns `start..end` of `span` on one line, or `None` when it does not touch the
/// line. `raw` is the line's bytes as shown (without the line ending, possibly cut), `line`
/// its byte range in the file. Where the span carries on from the previous line it starts at
/// the first non-blank character; where it carries on to the next it ends at the last one.
fn span_columns(raw: &[u8], line: &Range<u64>, span: &Range<u64>) -> Option<(usize, usize)> {
    if span.end <= line.start || span.start > line.end {
        return None;
    }
    let first = raw
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(raw.len());
    let last = raw
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(0, |i| i + 1);
    let start = if span.start <= line.start {
        first
    } else {
        ((span.start - line.start) as usize).min(raw.len())
    };
    let end = if span.end >= line.end {
        last
    } else {
        ((span.end - line.start) as usize).min(raw.len())
    };
    (start < end).then(|| (column(raw, start), column(raw, end)))
}

/// Display column of byte `at` in `raw`: characters before it, with a tab counting as
/// [`TAB_WIDTH`].
fn column(raw: &[u8], at: usize) -> usize {
    let before = &raw[..at];
    let tabs = before.iter().filter(|&&b| b == b'\t').count();
    String::from_utf8_lossy(before).chars().count() + tabs * (TAB_WIDTH - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(line: &str, line_start: u64, span: Range<u64>) -> Option<(usize, usize)> {
        let raw = line.as_bytes();
        span_columns(raw, &(line_start..line_start + raw.len() as u64), &span)
    }

    #[test]
    fn span_inside_one_line() {
        // `<b>` in `  <a><b/></a>` starts at byte 5.
        assert_eq!(cols("  <a><b/></a>", 100, 105..109), Some((5, 9)));
    }

    #[test]
    fn span_over_several_lines_uses_the_text_of_each() {
        // A span that began on an earlier line starts at the first non-blank character and,
        // when it carries on, ends at the last one.
        assert_eq!(cols("    <x/>   ", 10, 0..1000), Some((4, 8)));
        assert_eq!(cols("    <x/>", 10, 0..18), Some((4, 8)));
        assert_eq!(cols("    <x/>", 10, 14..1000), Some((4, 8)));
    }

    #[test]
    fn span_off_the_line() {
        assert_eq!(cols("<a/>", 10, 0..10), None);
        assert_eq!(cols("<a/>", 10, 15..20), None);
        // An empty or blank line inside a span draws nothing.
        assert_eq!(cols("   ", 10, 0..100), None);
    }

    #[test]
    fn columns_count_characters_and_expand_tabs() {
        // `ü` is two bytes but one column; a tab is four columns.
        let line = "\t<n a=\"ü\">x</n>";
        let at = line.find("x").unwrap() as u64;
        assert_eq!(cols(line, 0, at..at + 1), Some((4 + 9, 4 + 10)));
    }
}
