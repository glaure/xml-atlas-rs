//! The source view: virtualized, and editable with a cursor and a selection
//! (`docs/spec/text.md`, TEXT-16 to TEXT-21; `docs/decisions.md`, D19). Only the visible lines
//! are laid out each frame. Movement and edits come from `xml_atlas_engine::text`; this file
//! maps keys and the mouse to them and draws the result.

use std::ops::Range;

use egui::text::CCursor;
use egui::{
    Align2, Color32, Event, EventFilter, FontId, Galley, Key, Painter, Rect, Sense, Stroke, Ui,
    pos2, vec2,
};
use xml_atlas_engine::FormatOptions;
use xml_atlas_engine::problems::Problem;
use xml_atlas_engine::text::{self, Selection, TextEdit};

use crate::app::{Note, Open};
use crate::highlight::{Palette, line_job};
use crate::problems_panel;

/// Lines longer than this are cut for display. Minified one-line files stay responsive;
/// wrapping and horizontal paging of huge lines are on the roadmap. The cursor stops at the
/// cut (D19).
const MAX_LINE_BYTES: usize = 4000;

/// A tab is shown as this many spaces (TEXT-4).
const TAB_WIDTH: usize = text::TAB_WIDTH;

/// Lines kept above or below the cursor when the view scrolls to it.
const CONTEXT_LINES: usize = 3;

/// The keyboard focus of the text view of the document with view id `id`.
pub fn focus_id(id: egui::Id) -> egui::Id {
    id.with("text-edit")
}

pub fn show(ui: &mut Ui, open: &mut Open, format: &FormatOptions) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let font = FontId::monospace(13.0);
    let row_h = ui.fonts_mut(|f| f.row_height(&font)).ceil() + 1.0;
    let digits = open.doc.line_count().to_string().len() as f32;
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
    // Room for a problem marker left of the line numbers, when there are problems.
    let marker_w = if open.problems.is_empty() {
        0.0
    } else {
        char_w * 2.0
    };
    let gutter = digits * char_w + 16.0 + marker_w;
    let palette = Palette::new(ui.visuals().dark_mode);
    let visuals = ui.visuals().clone();

    let id = focus_id(open.id);
    // The whole text is one widget with the keyboard focus id; the lines only paint. egui
    // keeps the focus only on an id a widget uses, and surrenders it on a click elsewhere.
    let area_rect = ui.available_rect_before_wrap();
    let resp = ui.interact(area_rect, id, Sense::click_and_drag());
    if std::mem::take(&mut open.text_focus) {
        // After `interact`: egui drops the focus there when the frame has a click elsewhere,
        // such as the one on the Edit menu that asked for it.
        resp.request_focus();
    }
    let focused = ui.memory(|m| m.has_focus(id));
    if focused {
        // Tab and the arrow keys stay here instead of moving the focus.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            )
        });
        keys(ui, open, format);
    }
    // Another command may have changed the text under the cursor.
    if let Some(sel) = &mut open.text_sel {
        sel.anchor = text::snap(&open.doc.src, sel.anchor);
        sel.head = text::snap(&open.doc.src, sel.head);
    }

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
    let text_fill = visuals.selection.bg_fill;
    let cursor_stroke = Stroke::new(2.0, visuals.text_cursor.stroke.color);

    let mut area = egui::ScrollArea::both().id_salt("text").auto_shrink(false);
    let cursor_line = open.text_sel.map(|s| open.doc.line_of(s.head as u64));
    if let Some(line) = open.scroll_text_to.take() {
        area = area.vertical_scroll_offset(line.saturating_sub(CONTEXT_LINES) as f32 * row_h);
    } else if open.text_reveal
        && let Some(line) = cursor_line
    {
        // Only when the cursor's line is off screen; the line itself scrolls sideways below.
        let shown = &open.text_rows;
        if line < shown.start + 1 || line + 2 > shown.end {
            let top = if line < shown.start + 1 {
                line.saturating_sub(CONTEXT_LINES)
            } else {
                (line + CONTEXT_LINES + 1).saturating_sub(shown.len().max(1))
            };
            area = area.vertical_scroll_offset(top as f32 * row_h);
        }
    }
    let reveal = std::mem::take(&mut open.text_reveal);
    let sel = open.text_sel;
    // The byte under the pointer, and the first non-blank byte of its line.
    let mut under: Option<(usize, usize)> = None;
    let pointer_pos = ui.input(|i| i.pointer.interact_pos());
    area.show_rows(ui, row_h, open.doc.line_count(), |ui, range| {
        open.text_rows = range.clone();
        let doc = &open.doc;
        for n in range {
            let start = (doc.line_starts[n] as usize).max(text::text_start(&doc.src));
            let len = doc.line(n).len() - (start - doc.line_starts[n] as usize);
            let raw = &doc.src[start..start + len.min(MAX_LINE_BYTES)];
            let line = text::decode(raw).replace('\t', &" ".repeat(TAB_WIDTH));
            let galley = ui.painter().layout_job(line_job(&line, &font, &palette));
            let width = (gutter + galley.size().x + 8.0).max(ui.available_width());
            let (rect, row) = ui.allocate_exact_size(vec2(width, row_h), Sense::hover());
            let painter = ui.painter();
            let origin = pos2(rect.left() + gutter, rect.top());
            let line_bytes = start as u64..(start + len) as u64;
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
            // The text selection: exact on every line, and a little past the end of a line
            // whose line break it takes.
            if let Some(s) = sel.filter(|s| !s.is_empty()) {
                let r = s.range();
                let (a, b) = (r.start as u64, r.end as u64);
                if a <= line_bytes.end && b >= line_bytes.start {
                    let x = |at: u64| {
                        let at = (at.clamp(line_bytes.start, line_bytes.end) - line_bytes.start)
                            as usize;
                        origin.x + col_x(&galley, column(raw, at.min(raw.len())))
                    };
                    let mut right = x(b);
                    if b > line_bytes.end {
                        right += char_w * 0.6;
                    }
                    let r = Rect::from_x_y_ranges(x(a)..=right, rect.y_range());
                    painter.rect_filled(r, 0.0, text_fill);
                }
            }
            painter.text(
                pos2(rect.left() + gutter - 8.0, rect.center().y),
                Align2::RIGHT_CENTER,
                (n + 1).to_string(),
                font.clone(),
                visuals.weak_text_color(),
            );
            let here: Vec<&Problem> = open
                .problems
                .iter()
                .filter(|p| line_bytes.start <= p.offset && p.offset <= line_bytes.end)
                .collect();
            for p in &here {
                let color = problems_panel::color(&visuals, p.severity);
                painter.text(
                    pos2(rect.left() + 2.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    problems_panel::symbol(p.severity),
                    font.clone(),
                    color,
                );
                let at = ((p.offset - line_bytes.start) as usize).min(raw.len());
                let x = origin.x + col_x(&galley, column(raw, at));
                let end = galley.size().x.max(x - origin.x + 8.0) + origin.x;
                squiggle(painter, x, end.max(x + 8.0), rect.bottom() - 2.0, color);
            }
            // The cursor, where it can be drawn: not past the cut of a long line.
            if let Some(s) = sel
                && cursor_line == Some(n)
                && s.head - start <= raw.len()
            {
                let x = origin.x + col_x(&galley, column(raw, s.head - start));
                let caret = Rect::from_x_y_ranges(x..=x, rect.y_range());
                if focused {
                    painter
                        .line_segment([caret.center_top(), caret.center_bottom()], cursor_stroke);
                }
                if reveal {
                    ui.scroll_to_rect(caret.expand2(vec2(char_w * 4.0, 0.0)), None);
                }
            }
            painter.galley(origin, galley.clone(), visuals.text_color());
            if !here.is_empty() {
                let text: Vec<&str> = here.iter().map(|p| p.message.as_str()).collect();
                row.on_hover_text(text.join("\n"));
            }
            if let Some(pos) = pointer_pos.filter(|p| rect.y_range().contains(p.y)) {
                let col = galley.cursor_from_pos(pos - origin).index.0;
                let indent = raw.iter().take_while(|b| b.is_ascii_whitespace()).count();
                under = Some((start + byte_at(raw, col), start + indent));
            }
        }
    });
    let shift = ui.input(|i| i.modifiers.shift);
    if let Some((at, line_first)) = under {
        let pressed = resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.any_pressed());
        if pressed || resp.double_clicked() || resp.triple_clicked() {
            mouse(ui, open, &resp, at);
        } else if resp.dragged()
            && let Some(s) = open.text_sel
        {
            // Dragging extends the selection to the byte under the pointer.
            open.text_sel = Some(s.moved(at, true));
        }
        if resp.clicked()
            && !shift
            && let Some(element) = open.element_at(line_first as u64)
        {
            // A click still selects the element of its line (TEXT-7, TREE-12).
            open.select(element);
            // The click came from the text view; don't jump it.
            open.scroll_text_to = None;
        }
    }
}

/// A click puts the cursor, Shift+click extends the selection, a double-click selects a word
/// and a triple-click the line (TEXT-16, TEXT-17).
fn mouse(ui: &Ui, open: &mut Open, resp: &egui::Response, at: usize) {
    let shift = ui.input(|i| i.modifiers.shift);
    let range = |r: Range<usize>| Selection {
        anchor: r.start,
        head: r.end,
    };
    let sel = if resp.triple_clicked() {
        range(text::line_at(&open.doc, at))
    } else if resp.double_clicked() {
        range(text::word_at(&open.doc.src, at))
    } else {
        match open.text_sel {
            Some(s) if shift => s.moved(at, true),
            _ => Selection::cursor(at),
        }
    };
    open.text_sel = Some(sel);
    open.text_goal = None;
    open.doc.history.seal();
    resp.request_focus();
}

/// The keys of TEXT-16 to TEXT-18, while the text view has the keyboard.
fn keys(ui: &Ui, open: &mut Open, format: &FormatOptions) {
    let events = ui.input(|i| i.events.clone());
    let page = open.text_rows.len().saturating_sub(1).max(1) as isize;
    let options = open.doc.format_options(*format).options;
    for event in events {
        let sel = open
            .text_sel
            .unwrap_or_else(|| Selection::cursor(text::text_start(&open.doc.src)));
        let doc = &open.doc;
        let src = &doc.src;
        match event {
            Event::Text(t) if !t.is_empty() => apply(open, text::insert(doc, sel, &t)),
            // Ctrl+C, Ctrl+X, Ctrl+V and the Edit menu (TEXT-18).
            Event::Copy if !sel.is_empty() => ui.ctx().copy_text(text::selected_text(doc, sel)),
            Event::Cut => {
                if let Some((copied, change)) = text::cut(doc, sel) {
                    ui.ctx().copy_text(copied);
                    apply(open, Some(change));
                }
            }
            Event::Paste(t) => apply(open, text::paste(doc, sel, &t)),
            Event::Key {
                key: Key::A,
                pressed: true,
                modifiers,
                ..
            } if modifiers.command_only() => {
                open.text_goal = None;
                move_to(open, text::select_all(doc));
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let word = modifiers.command || modifiers.alt;
                let shift = modifiers.shift;
                let goal = || {
                    open.text_goal
                        .unwrap_or_else(|| text::column(doc, sel.head))
                };
                let to = match key {
                    Key::ArrowLeft if !shift && !word && !sel.is_empty() => Some(sel.range().start),
                    Key::ArrowRight if !shift && !word && !sel.is_empty() => Some(sel.range().end),
                    Key::ArrowLeft if word => Some(text::prev_word(src, sel.head)),
                    Key::ArrowRight if word => Some(text::next_word(src, sel.head)),
                    Key::ArrowLeft => Some(text::prev_char(src, sel.head)),
                    Key::ArrowRight => Some(text::next_char(src, sel.head)),
                    Key::Home if modifiers.command => Some(text::text_start(src)),
                    Key::End if modifiers.command => Some(src.len()),
                    Key::Home => Some(text::home(doc, sel.head)),
                    Key::End => Some(text::line_end(doc, sel.head)),
                    _ => None,
                };
                let vertical = match key {
                    Key::ArrowUp if !modifiers.alt => Some(-1),
                    Key::ArrowDown if !modifiers.alt => Some(1),
                    Key::PageUp => Some(-page),
                    Key::PageDown => Some(page),
                    _ => None,
                };
                if let Some(at) = to {
                    open.text_goal = None;
                    move_to(open, sel.moved(at, shift));
                } else if let Some(lines) = vertical {
                    let col = goal();
                    let at = text::vertical(doc, sel.head, lines, col);
                    open.text_goal = Some(col);
                    move_to(open, sel.moved(at, shift));
                } else {
                    let change = match key {
                        Key::Backspace => text::backspace(doc, sel, modifiers.command),
                        Key::Delete => text::delete(doc, sel, modifiers.command),
                        Key::Enter => text::newline(doc, sel),
                        Key::Tab if !modifiers.any() => text::tab(doc, sel, &options),
                        _ => continue,
                    };
                    apply(open, change);
                }
            }
            _ => {}
        }
    }
}

/// The cursor moved by a key: a new undo step starts with the next keystroke (TEXT-20), and
/// the view follows the cursor. Tree and grid stay (TREE-12).
fn move_to(open: &mut Open, sel: Selection) {
    open.text_sel = Some(sel);
    open.text_reveal = true;
    open.doc.history.seal();
}

/// Applies an edit a key made, and puts the cursor after it.
fn apply(open: &mut Open, change: Option<TextEdit>) {
    let Some(change) = change else { return };
    open.text_goal = None;
    let label = change.edit.label().to_string();
    match open.edit(|doc| doc.apply(change.edit).map(|()| true)) {
        Ok(_) => {
            open.text_sel = Some(change.selection);
            open.text_reveal = true;
            // Typing moves neither the text to the selected element nor the tree (TREE-12).
            open.scroll_text_to = None;
            open.tree.keep_scroll();
        }
        Err(e) => open.note = Some(Note::refused(format!("Cannot {label}: {e}"))),
    }
}

/// The x offset of display column `col` in `galley`.
fn col_x(galley: &Galley, col: usize) -> f32 {
    galley.pos_from_cursor(CCursor::new(col)).left()
}

/// The byte in `raw` at display column `col`: past the middle of a tab counts as after it.
fn byte_at(raw: &[u8], col: usize) -> usize {
    let mut at = 0;
    let mut here = 0;
    while at < raw.len() {
        let next = text::next_char(raw, at).min(raw.len());
        let w = if raw[at] == b'\t' { TAB_WIDTH } else { 1 };
        if here + w > col {
            if col - here > w / 2 {
                at = next;
            }
            break;
        }
        here += w;
        at = next;
    }
    at
}

/// A wavy line under a problem, from `x0` to `x1` at height `y`.
fn squiggle(painter: &Painter, x0: f32, x1: f32, y: f32, color: Color32) {
    const STEP: f32 = 3.0;
    let mut points = Vec::new();
    let mut x = x0;
    let mut up = false;
    while x <= x1 {
        points.push(pos2(x, if up { y - 2.0 } else { y }));
        up = !up;
        x += STEP;
    }
    painter.add(egui::Shape::line(points, Stroke::new(1.0, color)));
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
    let x = |col| origin.x + col_x(galley, col);
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
    text::decode(before).chars().count() + tabs * (TAB_WIDTH - 1)
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

    #[test]
    fn a_click_finds_the_byte_under_it() {
        // `ü` is two bytes but one column; a tab four columns, the nearer side of it wins.
        let raw = "\tGrüße".as_bytes();
        assert_eq!(byte_at(raw, 0), 0);
        assert_eq!(byte_at(raw, 1), 0);
        assert_eq!(byte_at(raw, 3), 1);
        assert_eq!(byte_at(raw, 4), 1);
        assert_eq!(byte_at(raw, 6), 3, "before ü");
        assert_eq!(byte_at(raw, 7), 5, "after ü");
        assert_eq!(byte_at(raw, 99), raw.len());
        // A byte that is not UTF-8 is one column, as the view draws it.
        assert_eq!(byte_at(b"a\xE4b", 2), 2);
        assert_eq!(column(b"a\xE4b", 2), 2);
    }
}
