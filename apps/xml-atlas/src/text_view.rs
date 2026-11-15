//! The source view: virtualized, and editable with a cursor, a selection and input methods
//! (`docs/spec/text.md`, TEXT-16 to TEXT-21; `docs/decisions.md`, D19). Only the visible lines
//! are laid out each frame. Movement and edits come from `xml_atlas_engine::text`; this file
//! maps keys and the mouse to them and draws the result.

use std::ops::Range;

use egui::output::IMEOutput;
use egui::text::CCursor;
use egui::{
    Align2, Color32, Event, EventFilter, FontId, Galley, IMEPurpose, ImeEvent, Key, Painter, Rect,
    Sense, Stroke, Ui, pos2, vec2,
};
use xml_atlas_engine::problems::Problem;
use xml_atlas_engine::text::{self, Selection, TextEdit};
use xml_atlas_engine::{Document, FormatOptions};

use crate::app::{Note, Open};
use crate::highlight::{self, Palette, line_job};
use crate::problems_panel;
use crate::scroll::{self, BAR, RowScroll};

/// Lines up to this long are laid out whole. Of a longer one only the columns on screen are,
/// with [`MARGIN_COLUMNS`] on each side, so a minified file of one huge line stays as quick
/// as any other (TEXT-5).
const WHOLE_LINE_BYTES: usize = 4000;

/// Columns laid out beyond each side of the view on a long line.
const MARGIN_COLUMNS: usize = 64;

/// How far back from the view's left edge a long line's layout looks for a `<` to start
/// from, so that its colours start right.
const BACK_TO_TAG_BYTES: usize = 512;

/// A tab is shown as this many spaces (TEXT-4).
const TAB_WIDTH: usize = text::TAB_WIDTH;

/// Lines kept above or below the cursor when the view scrolls to it.
const CONTEXT_LINES: usize = 3;

/// Text an input method is composing (TEXT-19): shown at the cursor, underlined, and not in
/// the document until it is committed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    /// The part the input method is working on, in characters of `text`.
    pub active: Option<Range<usize>>,
}

/// Columns of the long lines on screen (TEXT-5), kept up to date with the document.
#[derive(Clone, Debug, Default)]
pub struct LongLines {
    /// The document revision and line count they are for.
    revision: u64,
    lines: usize,
    columns: Vec<(usize, text::LineColumns)>,
}

/// A preedit shown in a line: before byte `at` of the line as drawn, `chars` columns wide.
/// What lies at or after `at` is drawn that much further right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shift {
    at: usize,
    chars: usize,
}

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
    // The text, a vertical scroll bar right of it and, when a line is wider than the view, a
    // horizontal one below. The view scrolls by whole lines (D16).
    let area_rect = ui.available_rect_before_wrap();
    ui.allocate_rect(area_rect, Sense::hover());
    let wide = open.text_width > (area_rect.width() - BAR) as f64;
    let text_rect = Rect::from_min_max(
        area_rect.min,
        pos2(
            area_rect.right() - BAR,
            area_rect.bottom() - if wide { BAR } else { 0.0 },
        ),
    );
    let resp = ui.interact(text_rect, id, Sense::click_and_drag());
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
    } else {
        // Losing the keyboard ends the composition; the input method is told by `ime` below.
        open.text_preedit = None;
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

    let total = open.doc.line_count();
    let visible = ((text_rect.height() / row_h).floor() as usize).max(1);
    let max_top = RowScroll::max_top(total, visible);
    let cursor_line = open.text_sel.map(|s| open.doc.line_of(s.head as u64));
    // The document may have become shorter than where the view was.
    if open.text_scroll.top > max_top {
        open.text_scroll.to(max_top, max_top);
    }
    let jumped = open.scroll_text_to.is_some();
    if let Some(line) = open.scroll_text_to.take() {
        open.text_scroll
            .to(line.saturating_sub(CONTEXT_LINES), max_top);
    } else if open.text_reveal
        && let Some(line) = cursor_line
    {
        // Only when the cursor's line is off screen; the line itself scrolls sideways below.
        let top = open.text_scroll.top;
        if line < top + 1 || line + 2 > top + visible {
            let to = if line < top + 1 {
                line.saturating_sub(CONTEXT_LINES)
            } else {
                (line + CONTEXT_LINES + 1).saturating_sub(visible)
            };
            open.text_scroll.to(to, max_top);
        }
    }
    // The mouse wheel, by whole lines and the pixels left over, however far down (TEXT-8);
    // Shift turns it sideways.
    if ui.rect_contains_pointer(area_rect) {
        let delta = ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta));
        open.text_scroll.by(-delta.y, row_h, max_top);
        open.text_left -= delta.x as f64;
    }
    scroll::vertical_bar(
        ui,
        Rect::from_min_max(
            pos2(text_rect.right(), area_rect.top()),
            pos2(area_rect.right(), text_rect.bottom()),
        ),
        id.with("v-bar"),
        total,
        visible,
        &mut open.text_scroll,
    );
    let text_view_w = text_rect.width() - gutter;
    if wide {
        scroll::horizontal_bar(
            ui,
            Rect::from_min_max(
                pos2(text_rect.left() + gutter, text_rect.bottom()),
                pos2(text_rect.right(), area_rect.bottom()),
            ),
            id.with("h-bar"),
            open.text_width - gutter as f64,
            text_view_w as f64,
            &mut open.text_left,
        );
    }
    let room = (open.text_width - (gutter + text_view_w) as f64).max(0.0);
    open.text_left = open.text_left.clamp(0.0, room);

    let reveal = std::mem::take(&mut open.text_reveal);
    let sel = open.text_sel;
    let preedit = open.text_preedit.clone();
    let mut caret_rect = None;
    // The byte under the pointer, and the first non-blank byte of its line.
    let mut under: Option<(usize, usize)> = None;
    let pointer_pos = ui
        .input(|i| i.pointer.interact_pos())
        .filter(|p| text_rect.contains(*p) || resp.dragged());
    let RowScroll { top, px } = open.text_scroll;
    let range = top..(top + visible + 2).min(total);
    open.text_rows = top..(top + visible).min(total);
    // Line numbers stay; the text scrolls sideways under its own clip.
    let numbers = ui.painter_at(Rect::from_min_max(
        text_rect.min,
        pos2(text_rect.left() + gutter, text_rect.bottom()),
    ));
    let text_clip = Rect::from_min_max(
        pos2(text_rect.left() + gutter - 4.0, text_rect.top()),
        text_rect.max,
    );
    let painter = ui.painter_at(text_clip);
    let left = open.text_left;
    let mut widest: f64 = 0.0;
    let mut caret_x = None;
    let view_columns = (text_view_w / char_w).ceil() as usize;
    {
        let doc = &open.doc;
        let long_cache = &mut open.text_long;
        long_cache.follow(doc);
        // The state the next line starts in, when the line above was laid out whole (TEXT-3);
        // else it comes from the index.
        let mut carry = None;
        for (i, n) in range.enumerate() {
            let line_start = (doc.line_starts[n] as usize).max(text::text_start(&doc.src));
            let len = doc.line(n).len() - (line_start - doc.line_starts[n] as usize);
            let full = &doc.src[line_start..line_start + len];
            // What of the line is laid out: all of a short one; of a long one the columns on
            // screen and a margin, from a `<` so that the colours start right (TEXT-5).
            let mut long = None;
            let (from, to, from_col) = if len <= WHOLE_LINE_BYTES {
                (0, len, 0)
            } else {
                let cols = long_cache.get(n, full);
                let want = ((left / char_w as f64) as usize).saturating_sub(MARGIN_COLUMNS);
                let (mut from, _) = cols.at_column(full, want);
                let back = from.saturating_sub(BACK_TO_TAG_BYTES);
                if let Some(i) = full[back..from].iter().rposition(|&b| b == b'<') {
                    from = back + i;
                }
                let from_col = cols.column(full, from);
                let span = want - from_col.min(want) + view_columns + 2 * MARGIN_COLUMNS;
                let to = text::LineColumns::advance(full, from, span);
                long = Some(cols);
                (from, to, from_col)
            };
            let start = line_start + from;
            let raw = &full[from..to];
            let head_here = sel
                .map(|s| s.head)
                .filter(|&h| cursor_line == Some(n) && h >= start && h - start <= raw.len());
            // The preedit, on the cursor's line where the cursor is laid out.
            let pre = match (&preedit, head_here) {
                (Some(p), Some(h)) => Some((p, h - start)),
                _ => None,
            };
            let shift = pre.map(|(p, at)| Shift {
                at,
                chars: p.text.chars().count(),
            });
            let line = match pre {
                Some((p, at)) => [
                    text::decode(&raw[..at]),
                    p.text.replace('\t', " "),
                    text::decode(&raw[at..]),
                ]
                .concat(),
                None => text::decode(raw),
            };
            let line = line.replace('\t', &" ".repeat(TAB_WIDTH));
            let state = carry.unwrap_or_else(|| highlight::state_at(&doc.index, &doc.src, start));
            let (job, end) = line_job(&line, &font, &palette, state);
            carry = (long.is_none() && pre.is_none()).then_some(end);
            let galley = painter.layout_job(job);
            let line_w = match long {
                Some(cols) => cols.total() as f64 * char_w as f64,
                None => galley.size().x as f64,
            };
            widest = widest.max(gutter as f64 + line_w + 8.0);
            let y = text_rect.top() - px + i as f32 * row_h;
            let rect =
                Rect::from_min_size(pos2(text_rect.left(), y), vec2(text_rect.width(), row_h));
            // Where the laid-out part starts, from the view's left edge: small even when the
            // line is far wider than a float counts exactly.
            let from_x = from_col as f64 * char_w as f64;
            let origin = pos2(text_rect.left() + gutter - (left - from_x) as f32, y);
            let line_bytes = start as u64..(start + raw.len()) as u64;
            let whole_line = line_start as u64..(line_start + len) as u64;
            if let Some(r) = &selected {
                paint_span(
                    &painter,
                    &galley,
                    origin,
                    rect,
                    raw,
                    shift,
                    &line_bytes,
                    r,
                    selected_fill,
                    None,
                );
            }
            if let Some(r) = &mark {
                let stroke = Some(mark_stroke);
                paint_span(
                    &painter,
                    &galley,
                    origin,
                    rect,
                    raw,
                    shift,
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
                        origin.x + col_x(&galley, shown_column(raw, at.min(raw.len()), shift))
                    };
                    let mut right = x(b);
                    if b > whole_line.end {
                        right += char_w * 0.6;
                    }
                    let r = Rect::from_x_y_ranges(x(a)..=right, rect.y_range());
                    painter.rect_filled(r, 0.0, text_fill);
                }
            }
            numbers.text(
                pos2(rect.left() + gutter - 8.0, rect.center().y),
                Align2::RIGHT_CENTER,
                (n + 1).to_string(),
                font.clone(),
                visuals.weak_text_color(),
            );
            let here: Vec<&Problem> = open
                .problems
                .iter()
                .filter(|p| whole_line.start <= p.offset && p.offset <= whole_line.end)
                .collect();
            for p in &here {
                let color = problems_panel::color(&visuals, p.severity);
                numbers.text(
                    pos2(rect.left() + 2.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    problems_panel::symbol(p.severity),
                    font.clone(),
                    color,
                );
                let at = (p.offset.saturating_sub(line_bytes.start) as usize).min(raw.len());
                let x = origin.x + col_x(&galley, shown_column(raw, at, shift));
                let end = galley.size().x.max(x - origin.x + 8.0) + origin.x;
                squiggle(&painter, x, end.max(x + 8.0), rect.bottom() - 2.0, color);
            }
            // The cursor, where it can be drawn: not past the cut of a long line.
            if let Some(head) = head_here {
                // After the preedit, if there is one.
                let x = origin.x + col_x(&galley, shown_column(raw, head - start, shift));
                let caret = Rect::from_x_y_ranges(x..=x, rect.y_range());
                caret_rect = Some(caret);
                caret_x = Some(from_x + (x - origin.x) as f64);
                if let Some((p, at)) = pre {
                    let color = visuals.text_color();
                    underline_preedit(&painter, &galley, origin, rect, column(raw, at), p, color);
                }
                if focused {
                    painter
                        .line_segment([caret.center_top(), caret.center_bottom()], cursor_stroke);
                }
            }
            painter.galley(origin, galley.clone(), visuals.text_color());
            if !here.is_empty() {
                let text: Vec<&str> = here.iter().map(|p| p.message.as_str()).collect();
                ui.interact(
                    rect.intersect(text_rect),
                    id.with(("problem", n)),
                    Sense::hover(),
                )
                .on_hover_text(text.join("\n"));
            }
            if let Some(pos) = pointer_pos.filter(|p| rect.y_range().contains(p.y)) {
                let col = unshift(raw, galley.cursor_from_pos(pos - origin).index.0, shift);
                let indent = full.iter().take_while(|b| b.is_ascii_whitespace()).count();
                under = Some((start + byte_at(raw, col), line_start + indent));
            }
            // The cursor on a long line outside what is laid out: where it is, to bring it
            // into view.
            if let (Some(cols), Some(s)) = (long, sel)
                && cursor_line == Some(n)
                && head_here.is_none()
            {
                caret_x = Some(cols.column(full, s.head - line_start) as f64 * char_w as f64);
            }
        }
    }
    open.text_width = widest;
    // Sideways, the cursor stays in view with a few characters around it, and a match or
    // item the view jumped to (Find, a click in grid or tree) is shown, however far along a
    // long line it is.
    let margin = char_w as f64 * 4.0;
    let view_w = text_view_w as f64;
    let mut to = left;
    if let Some(x) = caret_x.filter(|_| reveal) {
        if x - margin < left {
            to = (x - margin).max(0.0);
        } else if x + margin > left + view_w {
            to = x + margin - view_w;
        }
    }
    if jumped && let Some(m) = &open.mark {
        // A jump shows the whole match, its start a third into the view when it was not.
        let doc = &open.doc;
        let at = m.start as usize;
        let n = doc.line_of(m.start);
        let line_start = text::line_start(doc, at);
        let len = doc.line(n).len() - (line_start - doc.line_starts[n] as usize);
        let end = (m.end as usize).min(line_start + len);
        let (a, b) = if len > WHOLE_LINE_BYTES {
            let full = &doc.src[line_start..line_start + len];
            let cols = open.text_long.get(n, full);
            (
                cols.column(full, at - line_start),
                cols.column(full, end - line_start),
            )
        } else {
            (text::column(doc, at), text::column(doc, end))
        };
        let (x, x_end) = (a as f64 * char_w as f64, b as f64 * char_w as f64);
        if x < left || x_end > left + view_w {
            to = (x - view_w / 3.0).max(0.0);
        }
    }
    if to != left {
        open.text_left = to;
        ui.ctx().request_repaint();
    }
    let shift = ui.input(|i| i.modifiers.shift);
    let mut interrupt = false;
    if let Some((at, line_first)) = under {
        let pressed = resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.any_pressed());
        if pressed || resp.double_clicked() || resp.triple_clicked() {
            // A click ends a composition without committing it.
            interrupt = open.text_preedit.take().is_some();
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
    if ui.memory(|m| m.has_focus(id)) {
        // While the text view has the keyboard, input methods are on, and their windows open
        // next to the cursor (TEXT-19).
        let to_global = ui
            .ctx()
            .layer_transform_to_global(ui.layer_id())
            .unwrap_or_default();
        let cursor = caret_rect.unwrap_or(Rect::from_min_size(area_rect.min, vec2(0.0, row_h)));
        ui.output_mut(|o| {
            o.ime = Some(IMEOutput {
                purpose: IMEPurpose::Normal,
                rect: to_global * area_rect,
                cursor_rect: to_global * cursor,
                should_interrupt_composition: interrupt,
            })
        });
    }
}

impl LongLines {
    /// Brings the columns up to the document: after a change that stayed inside one line,
    /// only that line's columns around it are counted again; after anything else, all are
    /// counted anew when next shown.
    fn follow(&mut self, doc: &Document) {
        if self.revision == doc.revision() && self.lines == doc.line_count() {
            return;
        }
        let change = doc.last_change().filter(|c| {
            !self.columns.is_empty()
                && self.revision + 1 == doc.revision()
                && self.lines == doc.line_count()
                && !doc.src[c.at..c.at + c.inserted].contains(&b'\n')
        });
        match change {
            Some(c) => {
                let n = doc.line_of(c.at as u64);
                let start = text::line_start(doc, c.at);
                let len = doc.line(n).len() - (start - doc.line_starts[n] as usize);
                let line = &doc.src[start..start + len];
                self.columns.retain_mut(|(l, cols)| {
                    *l != n || cols.edit(line, c.at - start, c.removed, c.inserted)
                });
            }
            None => self.columns.clear(),
        }
        self.revision = doc.revision();
        self.lines = doc.line_count();
    }

    /// The columns of long line `n`, whose bytes are `line`; counted when first shown.
    fn get(&mut self, n: usize, line: &[u8]) -> &text::LineColumns {
        let i = match self.columns.iter().position(|(l, _)| *l == n) {
            Some(i) => i,
            None => {
                if self.columns.len() >= 64 {
                    self.columns.clear();
                }
                self.columns.push((n, text::LineColumns::new(line)));
                self.columns.len() - 1
            }
        };
        &self.columns[i].1
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

/// The keys of TEXT-16 to TEXT-19, while the text view has the keyboard.
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
            // Input methods, dead keys and compose (TEXT-19).
            Event::Ime(ime) => {
                if let Some(t) = ime_event(&mut open.text_preedit, ime) {
                    apply(open, text::insert(&open.doc, sel, &t));
                }
            }
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
    let t = std::time::Instant::now();
    let result = open.type_edit(change.edit);
    log::debug!(
        "typed: {:?}, index {:?}{}",
        t.elapsed(),
        open.doc.timings.index,
        if open.doc.stale().is_some() {
            ", stale"
        } else {
            ""
        }
    );
    match result {
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

/// Follows an input method (TEXT-19): a preedit replaces the one before, an empty one ends
/// the composition. Returns committed text, which goes into the document as typing. Empty
/// commits and lone line breaks are left out: platforms send them to end a composition, and
/// Enter comes as a key.
fn ime_event(preedit: &mut Option<Preedit>, event: ImeEvent) -> Option<String> {
    log::debug!("input method: {event:?}");
    match event {
        ImeEvent::Preedit {
            text,
            active_range_chars,
        } => {
            *preedit = (!text.is_empty()).then_some(Preedit {
                text,
                active: active_range_chars,
            });
            None
        }
        ImeEvent::Commit(text) => {
            *preedit = None;
            (!text.is_empty() && text != "\n" && text != "\r").then_some(text)
        }
        _ => None,
    }
}

/// Underlines the preedit, which starts at display column `col`; the part the input method
/// works on gets a heavier line.
fn underline_preedit(
    painter: &Painter,
    galley: &Galley,
    origin: egui::Pos2,
    row: Rect,
    col: usize,
    pre: &Preedit,
    color: Color32,
) {
    let y = row.bottom() - 2.0;
    let x = |c: usize| origin.x + col_x(galley, col + c);
    let all = 0..pre.text.chars().count();
    painter.line_segment(
        [pos2(x(all.start), y), pos2(x(all.end), y)],
        Stroke::new(1.0, color),
    );
    if let Some(a) = pre
        .active
        .clone()
        .filter(|a| !a.is_empty() && a.end <= all.end)
    {
        painter.line_segment(
            [pos2(x(a.start), y), pos2(x(a.end), y)],
            Stroke::new(2.0, color),
        );
    }
}

/// Display column of byte `at` in `raw` as drawn: after a preedit when it lies at or past it.
fn shown_column(raw: &[u8], at: usize, shift: Option<Shift>) -> usize {
    let col = column(raw, at);
    match shift {
        Some(s) if at >= s.at => col + s.chars,
        _ => col,
    }
}

/// The display column in `raw` that drawn column `col` stands for: inside a preedit is where
/// it is inserted.
fn unshift(raw: &[u8], col: usize, shift: Option<Shift>) -> usize {
    let Some(s) = shift else { return col };
    let start = column(raw, s.at);
    if col >= start + s.chars {
        col - s.chars
    } else {
        col.min(start)
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
    shift: Option<Shift>,
    line: &Range<u64>,
    span: &Range<u64>,
    fill: Color32,
    stroke: Option<Stroke>,
) {
    let Some((a, b)) = span_columns(raw, shift, line, span) else {
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
fn span_columns(
    raw: &[u8],
    shift: Option<Shift>,
    line: &Range<u64>,
    span: &Range<u64>,
) -> Option<(usize, usize)> {
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
    (start < end).then(|| {
        (
            shown_column(raw, start, shift),
            shown_column(raw, end, shift),
        )
    })
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
        span_columns(
            raw,
            None,
            &(line_start..line_start + raw.len() as u64),
            &span,
        )
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

    fn pre(text: &str, active: Option<Range<usize>>) -> ImeEvent {
        ImeEvent::Preedit {
            text: text.into(),
            active_range_chars: active,
        }
    }

    /// Choosing a candidate, as egui-winit records it on macOS, Wayland with fcitx5, and
    /// Windows: each commits `测试` once and leaves no preedit behind.
    #[test]
    fn input_method_commits_once() {
        let commit = || ImeEvent::Commit("测试".into());
        let platforms = [
            vec![pre("ce", Some(0..2)), pre("", None), commit()],
            vec![
                pre("ce", Some(0..2)),
                pre("", None),
                commit(),
                pre("", Some(0..0)),
                pre("", None),
            ],
            vec![
                pre("ce", Some(0..2)),
                pre("测试", Some(0..2)),
                pre("", None),
                commit(),
            ],
        ];
        for events in platforms {
            let mut preedit = None;
            let mut committed = Vec::new();
            for e in events {
                committed.extend(ime_event(&mut preedit, e));
            }
            assert_eq!(committed, ["测试"]);
            assert_eq!(preedit, None);
        }
        // While composing, the preedit is kept; Backspace on its last letter clears it.
        let mut preedit = None;
        assert_eq!(ime_event(&mut preedit, pre("c", Some(0..1))), None);
        assert_eq!(preedit.as_ref().map(|p| p.text.as_str()), Some("c"));
        ime_event(&mut preedit, pre("", Some(0..0)));
        assert_eq!(preedit, None);
        // Ending with an empty commit or a line break adds nothing; a dead key's `é` does.
        assert_eq!(
            ime_event(&mut preedit, ImeEvent::Commit(String::new())),
            None
        );
        assert_eq!(ime_event(&mut preedit, ImeEvent::Commit("\n".into())), None);
        assert_eq!(
            ime_event(&mut preedit, ImeEvent::Commit("é".into())).as_deref(),
            Some("é")
        );
    }

    /// A preedit `日本` (two columns) shown before `ße` in `\tGrüße`.
    #[test]
    fn a_preedit_moves_what_follows_it() {
        let raw = "\tGrüße".as_bytes();
        let at = "\tGrü".len();
        let shift = Some(Shift { at, chars: 2 });
        // Before it nothing moves; at and after it, two columns.
        assert_eq!(shown_column(raw, 1, shift), 4);
        assert_eq!(
            shown_column(raw, at, shift),
            7 + 2,
            "the cursor goes after it"
        );
        assert_eq!(shown_column(raw, raw.len(), shift), 9 + 2);
        // A highlight over the whole line stretches over it.
        let line = 0..raw.len() as u64;
        assert_eq!(span_columns(raw, shift, &line, &(0..100)), Some((4, 11)));
        // A click inside it lands where it is inserted; after it, on the text.
        assert_eq!(unshift(raw, 5, shift), 5);
        assert_eq!(unshift(raw, 8, shift), 7);
        assert_eq!(unshift(raw, 10, shift), 8);
        assert_eq!(byte_at(raw, unshift(raw, 10, shift)), at + 2);
        assert_eq!(unshift(raw, 8, None), 8);
    }
}
