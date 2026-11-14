//! Read-only source view. Only the visible lines are laid out each frame.

use egui::{Align2, FontId, Sense, Ui, pos2, vec2};

use crate::app::Open;
use crate::highlight::{Palette, line_job};

/// Lines longer than this are cut for display. Minified one-line files stay responsive;
/// wrapping and horizontal paging of huge lines are on the roadmap.
const MAX_LINE_BYTES: usize = 4000;

pub fn show(ui: &mut Ui, open: &mut Open) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let font = FontId::monospace(13.0);
    let row_h = ui.fonts_mut(|f| f.row_height(&font)).ceil() + 1.0;
    let digits = open.doc.line_count().to_string().len() as f32;
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
    let gutter = digits * char_w + 16.0;
    let palette = Palette::new(ui.visuals().dark_mode);
    let visuals = ui.visuals().clone();

    let focus_lines = open.focus.map(|id| open.doc.node_lines(id));
    let mut area = egui::ScrollArea::both().id_salt("text").auto_shrink(false);
    if let Some(line) = open.scroll_text_to.take() {
        area = area.vertical_scroll_offset(((line as f32) - 3.0).max(0.0) * row_h);
    }
    let mut clicked = None;
    area.show_rows(ui, row_h, open.doc.line_count(), |ui, range| {
        for n in range {
            let raw = open.doc.line(n);
            let raw = &raw[..raw.len().min(MAX_LINE_BYTES)];
            let line = String::from_utf8_lossy(raw).replace('\t', "    ");
            let galley = ui.painter().layout_job(line_job(&line, &font, &palette));
            let width = (gutter + galley.size().x + 8.0).max(ui.available_width());
            let (rect, resp) = ui.allocate_exact_size(vec2(width, row_h), Sense::click());
            let painter = ui.painter();
            if focus_lines.is_some_and(|(a, b)| a <= n && n <= b) {
                painter.rect_filled(rect, 0.0, visuals.selection.bg_fill.gamma_multiply(0.35));
            }
            painter.text(
                pos2(rect.left() + gutter - 8.0, rect.center().y),
                Align2::RIGHT_CENTER,
                (n + 1).to_string(),
                font.clone(),
                visuals.weak_text_color(),
            );
            painter.galley(
                pos2(rect.left() + gutter, rect.top()),
                galley,
                visuals.text_color(),
            );
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
