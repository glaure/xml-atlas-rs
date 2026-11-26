//! Tools > XPath… (`docs/spec/xpath.md`, XP-1, XP-3 to XP-5; D35).
//!
//! Ctrl+Shift+X opens a query bar under the find bar. Enter runs the expression on a thread
//! of its own; *Stop* ends it. Nodes are listed in a panel at the bottom, as the problems are:
//! a click selects one, F3 and Shift+F3 step through them while the bar is open. A number,
//! string or boolean is shown in the bar. Each tab keeps its own results ([`Open::xpath`]);
//! after an edit they are out of date and not shown until the query runs again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use egui::{Key, KeyboardShortcut, Modifiers, RichText, Ui};
use xml_atlas_engine::xml::Index;
use xml_atlas_engine::xpath::{Item, ItemKind, Value, XPath, XPathError};

use crate::app::Open;
use crate::keys;

pub const OPEN: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::X);
const NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F3);
const PREVIOUS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::F3);

/// Expressions kept in the bar's list.
const HISTORY: usize = 20;
/// Height of the results list before it scrolls, in rows.
const VISIBLE_ROWS: f32 = 8.0;
/// How much of a node's value a row shows.
const VALUE_CHARS: usize = 120;
/// Shown in the empty field; Tab takes it.
const EXAMPLE: &str = "//order[@status='open']";

/// The bar: shared by the tabs.
#[derive(Default)]
pub struct XPathBar {
    pub visible: bool,
    text: String,
    /// The expressions run, newest first.
    history: Vec<String>,
    focus_field: bool,
    /// The field's text as last checked, and what is wrong with it (XP-1).
    checked: Option<Checked>,
}

/// An expression checked as it is typed, against one document.
struct Checked {
    text: String,
    index: Weak<Index>,
    problem: Option<Problem>,
}

/// What is wrong with the expression in the field.
#[derive(Clone, Debug, PartialEq)]
struct Problem {
    message: String,
    /// The character where it goes wrong, from 1, when it is known.
    at: Option<usize>,
}

/// One tab's query and what it gave.
pub struct Results {
    expr: String,
    /// The index the query ran on. Edits replace the index or move it (copy on write), so
    /// while this still points at the document's index, the results' node ids are its own.
    index: Weak<Index>,
    state: State,
    /// The result selected last, in the list and in the document.
    current: Option<usize>,
    /// Scroll the list to this row on the next frame.
    scroll_to: Option<usize>,
}

enum State {
    Running {
        rx: Receiver<Result<Value, XPathError>>,
        stop: Arc<AtomicBool>,
        started: Instant,
    },
    Done {
        value: Value,
        took: Duration,
    },
    Failed(String),
}

impl Results {
    /// Whether the document is still the one the query ran on.
    fn fresh(&self, open: &Open) -> bool {
        self.fresh_for(&open.doc.index)
    }

    /// Whether `index` is the one the query ran on.
    fn fresh_for(&self, index: &Arc<Index>) -> bool {
        self.index.as_ptr() == Arc::as_ptr(index) && self.index.strong_count() > 0
    }

    fn nodes(&self) -> Option<&[Item]> {
        match &self.state {
            State::Done {
                value: Value::Nodes(n),
                ..
            } => Some(n),
            _ => None,
        }
    }

    fn stop(&self) {
        if let State::Running { stop, .. } = &self.state {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

impl XPathBar {
    /// Shows the bar and puts the cursor in its field.
    pub fn open(&mut self) {
        self.visible = true;
        self.focus_field = true;
    }

    /// Hides the bar and its results, and stops a query still running.
    pub fn close(&mut self, open: Option<&mut Open>) {
        self.visible = false;
        if let Some(r) = open.and_then(|o| o.xpath.as_ref()) {
            r.stop();
        }
    }

    /// Ctrl+Shift+X, and F3 and Shift+F3 while the bar shows results; call before the find
    /// bar's shortcuts, which take F3 otherwise.
    pub fn shortcuts(&mut self, ctx: &egui::Context, open: Option<&mut Open>) {
        let Some(open) = open else {
            return;
        };
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
            self.open();
        }
        if !self.visible || !has_fresh_nodes(open) {
            return;
        }
        // Shift+F3 first: the plain F3 shortcut would also match it.
        if ctx.input_mut(|i| i.consume_shortcut(&PREVIOUS)) {
            step(open, false);
        } else if ctx.input_mut(|i| i.consume_shortcut(&NEXT)) {
            step(open, true);
        }
    }

    /// The bar, under the find bar.
    pub fn show(&mut self, ui: &mut Ui, open: &mut Open) {
        poll(open);
        let mut run = false;
        let mut close = false;
        let mut valid = false;
        let mut message = None;
        ui.horizontal(|ui| {
            ui.label("XPath");
            let empty = self.text.is_empty();
            let output = egui::TextEdit::singleline(&mut self.text)
                .desired_width(360.0)
                .font(egui::TextStyle::Monospace)
                .hint_text(EXAMPLE)
                // While empty, Tab stays in the field to take the example.
                .lock_focus(empty)
                .show(ui);
            let field = output.response.response.clone();
            if std::mem::take(&mut self.focus_field) {
                field.request_focus();
            }
            if empty
                && field.has_focus()
                && ui.input(|i| i.key_pressed(Key::Tab) && i.modifiers.is_none())
            {
                self.take_example(ui.ctx(), field.id);
            }
            if empty {
                field.clone().on_hover_text("Tab takes the example");
            }
            let problem = self.check(open);
            valid = problem.is_none() && !self.text.trim().is_empty();
            if let Some(p) = &problem {
                show_problem(ui, &output, p);
            } else if valid {
                ui.label(RichText::new("valid").weak())
                    .on_hover_text("A valid XPath 1.0 expression");
            }
            message = problem.map(|p| p.message);
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                // A wrong expression is not run: its message is already shown.
                run = valid;
                field.request_focus();
            }
            // The field gives up focus on Escape before we see the key, so check both.
            if (field.has_focus() || field.lost_focus()) && ui.input(|i| i.key_pressed(Key::Escape))
            {
                close = true;
            }
            ui.add_enabled_ui(!self.history.is_empty(), |ui| {
                ui.menu_button("▾", |ui| {
                    for h in &self.history {
                        if ui.button(RichText::new(h).monospace()).clicked() {
                            self.text = h.clone();
                            self.focus_field = true;
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("Expressions run before");
            });
            let running = matches!(
                open.xpath.as_ref().map(|r| &r.state),
                Some(State::Running { .. })
            );
            if running {
                if ui.button("Stop").clicked()
                    && let Some(r) = &open.xpath
                {
                    r.stop();
                }
            } else if ui
                .add_enabled(valid, egui::Button::new("Run"))
                .on_hover_text("Enter")
                .on_disabled_hover_text("Write a valid XPath 1.0 expression first")
                .clicked()
            {
                run = true;
            }
            if has_fresh_nodes(open) {
                if ui
                    .button("⬆")
                    .on_hover_text(format!("Previous ({})", keys::text(ui.ctx(), &PREVIOUS)))
                    .clicked()
                {
                    step(open, false);
                }
                if ui.button("⬇").on_hover_text("Next (F3)").clicked() {
                    step(open, true);
                }
            }
            status(ui, open);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕").on_hover_text("Close (Escape)").clicked() {
                    close = true;
                }
            });
        });
        if let Some(m) = message {
            ui.horizontal(|ui| {
                ui.label(RichText::new("XPath").color(egui::Color32::TRANSPARENT));
                ui.colored_label(ui.visuals().error_fg_color, m);
            });
        }
        if run {
            self.run(ui.ctx(), open);
        }
        if close {
            self.close(Some(open));
        }
    }

    /// Puts the example into the empty field, the cursor after it.
    fn take_example(&mut self, ctx: &egui::Context, field: egui::Id) {
        self.text = EXAMPLE.to_string();
        if let Some(mut state) = egui::TextEdit::load_state(ctx, field) {
            let end = egui::text::CCursor::new(self.text.chars().count());
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(end)));
            state.store(ctx, field);
        }
    }

    /// What is wrong with the expression in the field, checked again when the text or the
    /// document changed: parsing a line is a few microseconds, so it is done as it is typed.
    fn check(&mut self, open: &Open) -> Option<Problem> {
        let index = &open.doc.index;
        let same = self
            .checked
            .as_ref()
            .is_some_and(|c| c.text == self.text && c.index.as_ptr() == Arc::as_ptr(index));
        if !same {
            let problem = if self.text.trim().is_empty() {
                None
            } else {
                match XPath::parse(&self.text) {
                    Err(XPathError::Syntax { message, at }) => Some(Problem {
                        message: format!("{message} at {at}"),
                        at: Some(at),
                    }),
                    Err(e) => Some(Problem {
                        message: e.to_string(),
                        at: None,
                    }),
                    Ok(x) => x.check(&open.doc.src, index).err().map(|e| Problem {
                        message: e.to_string(),
                        at: None,
                    }),
                }
            };
            self.checked = Some(Checked {
                text: self.text.clone(),
                index: Arc::downgrade(index),
                problem,
            });
        }
        self.checked.as_ref().and_then(|c| c.problem.clone())
    }

    /// Runs the expression in the field on the tab's document, on a thread of its own.
    fn run(&mut self, ctx: &egui::Context, open: &mut Open) {
        let text = self.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.history.retain(|h| *h != text);
        self.history.insert(0, text.clone());
        self.history.truncate(HISTORY);
        if let Some(r) = &open.xpath {
            r.stop();
        }
        // Typing may have left the index behind the text (D19): read it again first.
        open.settle();
        let index = Arc::clone(&open.doc.index);
        let weak = Arc::downgrade(&index);
        let state = match XPath::parse(&text) {
            Err(e) => State::Failed(e.to_string()),
            Ok(query) => {
                let src = Arc::clone(&open.doc.src);
                let stop = Arc::new(AtomicBool::new(false));
                let (tx, rx) = channel();
                let ctx = ctx.clone();
                let flag = Arc::clone(&stop);
                let spawned = std::thread::Builder::new()
                    .name("xpath".into())
                    .spawn(move || {
                        let answer = query.evaluate(&src, &index, &flag);
                        // The tab may be gone; then nobody waits for the answer.
                        let _ = tx.send(answer);
                        ctx.request_repaint();
                    });
                match spawned {
                    Ok(_) => State::Running {
                        rx,
                        stop,
                        started: Instant::now(),
                    },
                    Err(e) => State::Failed(format!("cannot start the query: {e}")),
                }
            }
        };
        open.xpath = Some(Results {
            expr: text,
            index: weak,
            state,
            current: None,
            scroll_to: None,
        });
    }
}

/// Frames the field in the error colour and underlines the character where the expression
/// goes wrong; at the end, a short line after the last character.
fn show_problem(ui: &Ui, output: &egui::text_edit::TextEditOutput, problem: &Problem) {
    let color = ui.visuals().error_fg_color;
    let rect = output.response.response.rect;
    ui.painter().rect_stroke(
        rect,
        ui.visuals().widgets.inactive.corner_radius,
        egui::Stroke::new(1.5, color),
        egui::StrokeKind::Outside,
    );
    let Some(at) = problem.at else {
        return;
    };
    let galley = &output.galley;
    let chars = galley.text().chars().count();
    let i = at.saturating_sub(1).min(chars);
    let pos = |c: usize| galley.pos_from_cursor(egui::text::CCursor::new(c));
    let start = pos(i);
    let end_x = if i < chars {
        pos(i + 1).min.x
    } else {
        start.min.x + galley.size().y * 0.5
    };
    let offset = output.galley_pos.to_vec2();
    let y = start.max.y + offset.y + 1.0;
    let painter = ui
        .painter()
        .with_clip_rect(output.text_clip_rect.expand(2.0));
    painter.line_segment(
        [
            egui::pos2(start.min.x + offset.x, y),
            egui::pos2(end_x.max(start.min.x + 4.0) + offset.x, y),
        ],
        egui::Stroke::new(2.0, color),
    );
}

/// Whether `open` has results to show the panel for.
pub fn has_results(open: &Open) -> bool {
    open.xpath.as_ref().is_some_and(|r| r.nodes().is_some())
}

/// Runs `expr` as Enter in the bar does and waits for the answer, for other modules' tests.
#[cfg(test)]
pub fn run_for_test(open: &mut Open, expr: &str) {
    let mut bar = XPathBar {
        text: expr.into(),
        ..Default::default()
    };
    bar.run(&egui::Context::default(), open);
    let t = Instant::now();
    while matches!(
        open.xpath.as_ref().map(|r| &r.state),
        Some(State::Running { .. })
    ) {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "the query did not end"
        );
        std::thread::sleep(Duration::from_millis(1));
        poll(open);
    }
}

/// The nodes of the results shown for `open`, while the document is still the one the query
/// ran on: for the diagram's marks (D36).
pub fn result_items(open: &Open) -> Option<&[Item]> {
    let r = open.xpath.as_ref().filter(|r| r.fresh(open))?;
    r.nodes()
}

/// The results panel at the bottom: one row per node, its line, path and value.
pub fn show_results(ui: &mut Ui, open: &mut Open) {
    let Some(r) = &open.xpath else {
        return;
    };
    let Some(nodes) = r.nodes() else {
        return;
    };
    let count = nodes.len();
    ui.horizontal(|ui| {
        ui.strong("XPath results");
        ui.label(RichText::new(format!("{}  ·  {}", count_text(count), r.expr)).weak());
    });
    if !r.fresh(open) {
        ui.label(
            RichText::new(
                "The document has changed since the query ran. Press Enter in the XPath bar \
                 to run it again.",
            )
            .weak(),
        );
        return;
    }
    let row_h = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.spacing().interact_size.y);
    let current = r.current;
    let scroll_to = open.xpath.as_mut().and_then(|r| r.scroll_to.take());
    let mut area = egui::ScrollArea::vertical()
        .id_salt("xpath-results")
        .max_height((row_h + ui.spacing().item_spacing.y) * VISIBLE_ROWS)
        .auto_shrink([false, true]);
    if let Some(i) = scroll_to {
        // Keep the row a few rows down from the top, so the rows before it show too.
        let top = (i.saturating_sub(2)) as f32 * (row_h + ui.spacing().item_spacing.y);
        area = area.vertical_scroll_offset(top);
    }
    let mut go = None;
    let doc = &open.doc;
    let nodes = match &open.xpath {
        Some(r) => r.nodes().unwrap_or_default(),
        None => &[],
    };
    // Only the rows on screen are laid out: a query can give millions of nodes.
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let body = egui::TextStyle::Body.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    // The line numbers' column: as wide as the largest line number.
    let line_w = ui
        .painter()
        .layout_no_wrap(doc.line_count().to_string(), mono.clone(), weak)
        .size()
        .x;
    area.show_rows(ui, row_h, count, |ui, rows| {
        for i in rows {
            let item = nodes[i];
            let span = item.span(&doc.src, &doc.index);
            let line = doc.line_of(span.start as u64) + 1;
            let path = item.path(&doc.src, &doc.index);
            let value = item.value(&doc.src, &doc.index, VALUE_CHARS);
            // The whole row is one link: it shows where it can be clicked in either theme.
            let (rect, row) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), row_h),
                egui::Sense::click(),
            );
            let row = row.on_hover_cursor(egui::CursorIcon::PointingHand);
            let v = ui.visuals();
            let selected = current == Some(i);
            let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
            if selected {
                painter.rect_filled(rect, 2.0, v.selection.bg_fill);
            } else if row.hovered() {
                painter.rect_filled(rect, 2.0, v.widgets.hovered.weak_bg_fill);
            }
            let y = rect.center().y;
            let mut x = rect.left() + 6.0;
            painter.text(
                egui::pos2(x + line_w, y),
                egui::Align2::RIGHT_CENTER,
                line.to_string(),
                mono.clone(),
                weak,
            );
            x += line_w + 12.0;
            let path_color = if selected {
                v.selection.stroke.color
            } else {
                v.hyperlink_color
            };
            let drawn = painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_CENTER,
                path,
                body.clone(),
                path_color,
            );
            if row.hovered() && !selected {
                // Underlined on hover, as a link is.
                painter.line_segment(
                    [
                        egui::pos2(drawn.left(), drawn.bottom()),
                        egui::pos2(drawn.right(), drawn.bottom()),
                    ],
                    egui::Stroke::new(1.0, path_color),
                );
            }
            if !value.is_empty() {
                painter.text(
                    egui::pos2(drawn.right() + 16.0, y),
                    egui::Align2::LEFT_CENTER,
                    value,
                    body.clone(),
                    weak,
                );
            }
            if row.clicked() {
                go = Some(i);
            }
        }
    });
    if let Some(i) = go {
        go_to(open, i, false);
    }
}

/// Takes the answer of a query when it has come.
fn poll(open: &mut Open) {
    let Some(r) = &mut open.xpath else {
        return;
    };
    let State::Running { rx, started, .. } = &r.state else {
        return;
    };
    let took = started.elapsed();
    r.state = match rx.try_recv() {
        Err(TryRecvError::Empty) => return,
        Ok(Ok(value)) => State::Done { value, took },
        Ok(Err(e)) => State::Failed(e.to_string()),
        Err(TryRecvError::Disconnected) => State::Failed("the query stopped".into()),
    };
    if r.nodes().is_some_and(|n| !n.is_empty()) {
        go_to(open, 0, true);
    }
}

/// What the query gave, after the buttons.
fn status(ui: &mut Ui, open: &Open) {
    let Some(r) = &open.xpath else {
        return;
    };
    match &r.state {
        State::Running { started, .. } => {
            ui.spinner();
            ui.label(
                RichText::new(format!("running, {:.1} s", started.elapsed().as_secs_f64())).weak(),
            );
            // The time counts up while it runs.
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        State::Failed(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        State::Done { value, took } => {
            let ms = format!("{:.0} ms", took.as_secs_f64() * 1000.0);
            match value {
                Value::Nodes(n) => {
                    let at = r
                        .current
                        .filter(|_| r.fresh(open))
                        .map(|i| format!("{} of ", i + 1))
                        .unwrap_or_default();
                    ui.label(RichText::new(format!("{at}{}  ·  {ms}", count_text(n.len()))).weak());
                }
                other => {
                    let kind = match other {
                        Value::Number(_) => "a number",
                        Value::String(_) => "a string",
                        _ => "a boolean",
                    };
                    ui.label(RichText::new(other.to_text().unwrap_or_default()).strong());
                    ui.label(RichText::new(format!("{kind}  ·  {ms}")).weak());
                }
            }
            if !r.fresh(open) {
                ui.label(RichText::new("out of date: Enter runs it again").weak());
            }
        }
    }
}

fn count_text(n: usize) -> String {
    match n {
        0 => "no nodes".into(),
        1 => "1 node".into(),
        n => format!("{n} nodes"),
    }
}

fn has_fresh_nodes(open: &Open) -> bool {
    open.xpath
        .as_ref()
        .is_some_and(|r| r.fresh(open) && r.nodes().is_some_and(|n| !n.is_empty()))
}

/// F3 and Shift+F3: the next or previous result, going on from the other end.
fn step(open: &mut Open, forward: bool) {
    let Some(r) = &open.xpath else {
        return;
    };
    let len = r.nodes().map_or(0, <[Item]>::len);
    if len == 0 {
        return;
    }
    let i = match (r.current, forward) {
        (None, true) => 0,
        (None, false) => len - 1,
        (Some(i), true) => (i + 1) % len,
        (Some(i), false) => (i + len - 1) % len,
    };
    go_to(open, i, true);
}

/// Selects result `i` in tree, grid and text, and marks an attribute, text or other node in
/// the text as a grid cell is (TEXT-11). `scroll` also scrolls the list to it.
fn go_to(open: &mut Open, i: usize, scroll: bool) {
    let Some(r) = &mut open.xpath else {
        return;
    };
    let Some(&item) = r.nodes().and_then(|n| n.get(i)) else {
        return;
    };
    if !r.fresh_for(&open.doc.index) {
        return;
    }
    r.current = Some(i);
    if scroll {
        r.scroll_to = Some(i);
    }
    let element = item.element(&open.doc.index);
    open.select(element);
    match item {
        Item::Attribute(e, k) => open.mark_attribute(e, k),
        Item::Folded(e) => open.mark_text(e),
        Item::Node(_)
            if item.kind(&open.doc.index) != ItemKind::Element
                && item.kind(&open.doc.index) != ItemKind::Root =>
        {
            let span = item.span(&open.doc.src, &open.doc.index);
            open.mark = Some(span.start as u64..span.end as u64);
            open.scroll_text_to = Some(span.start as u64);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use xml_atlas_engine::{Document, Edit};

    use super::*;

    fn opened(src: &str) -> Open {
        Open::new(
            Document::from_bytes(src.as_bytes().to_vec()),
            egui::Id::unique("xpath"),
        )
    }

    /// Runs `expr` as Enter does and waits for its thread.
    fn run(bar: &mut XPathBar, open: &mut Open, expr: &str) {
        bar.text = expr.into();
        bar.run(&egui::Context::default(), open);
        let t = Instant::now();
        while matches!(
            open.xpath.as_ref().map(|r| &r.state),
            Some(State::Running { .. })
        ) {
            assert!(
                t.elapsed() < Duration::from_secs(10),
                "the query did not end"
            );
            std::thread::sleep(Duration::from_millis(1));
            poll(open);
        }
    }

    #[test]
    fn a_query_selects_its_results_and_f3_steps_through_them() {
        let mut open = opened("<r>\r\n<a n=\"1\">Größe</a>\r\n<b/>\r\n<a n=\"2\"/>\r\n</r>");
        let mut bar = XPathBar::default();
        run(&mut bar, &mut open, "//a/@n");
        assert_eq!(open.xpath.as_ref().unwrap().nodes().unwrap().len(), 2);
        // The first result is selected at once: its element, with the attribute marked.
        let first_a = open.doc.index.root_element().unwrap() + 1;
        assert_eq!(open.selected, Some(first_a));
        let mark = open.mark.clone().unwrap();
        assert_eq!(
            &open.doc.src[mark.start as usize..mark.end as usize],
            b"n=\"1\""
        );
        step(&mut open, true);
        assert_eq!(open.xpath.as_ref().unwrap().current, Some(1));
        let mark = open.mark.clone().unwrap();
        assert_eq!(
            &open.doc.src[mark.start as usize..mark.end as usize],
            b"n=\"2\""
        );
        // It goes on from the other end, both ways.
        step(&mut open, true);
        assert_eq!(open.xpath.as_ref().unwrap().current, Some(0));
        step(&mut open, false);
        assert_eq!(open.xpath.as_ref().unwrap().current, Some(1));
        // Drawing the list, with a row selected, changes no selection.
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| show_results(ui, &mut open));
        out.textures_delta.clear();
        assert_eq!(open.xpath.as_ref().unwrap().current, Some(1));
        // Text is marked as a grid cell is.
        run(&mut bar, &mut open, "//a/text()");
        let mark = open.mark.clone().unwrap();
        assert_eq!(
            &open.doc.src[mark.start as usize..mark.end as usize],
            "Größe".as_bytes()
        );
        assert_eq!(bar.history, ["//a/text()", "//a/@n"]);
    }

    /// XP-1: the field says as it is typed whether the expression is valid, and where not.
    #[test]
    fn the_expression_is_checked_as_it_is_typed() {
        let mut open = opened(r#"<r xmlns:p="urn:p"><p:a/></r>"#);
        let mut bar = XPathBar::default();
        let check = |bar: &mut XPathBar, open: &Open, t: &str| {
            bar.text = t.into();
            bar.check(open)
        };
        assert_eq!(check(&mut bar, &open, "  "), None);
        assert_eq!(check(&mut bar, &open, "//p:a[@x = 'größe']"), None);
        assert_eq!(
            check(&mut bar, &open, "//p:a["),
            Some(Problem {
                message: "expected an expression, found the end at 7".into(),
                at: Some(7),
            })
        );
        assert_eq!(
            check(&mut bar, &open, "//q:a"),
            Some(Problem {
                message: "the prefix `q` is not declared on the root element".into(),
                at: None,
            })
        );
        // Drawing the bar with the error at the end, in the middle and after non-ASCII text.
        let ctx = egui::Context::default();
        for t in ["//p:a[", "//p:a[)]", "//größe[)]", "//q:a"] {
            bar.text = t.into();
            let mut out = ctx.run_ui(Default::default(), |ui| bar.show(ui, &mut open));
            out.textures_delta.clear();
            assert!(open.xpath.is_none(), "{t} ran");
        }
    }

    /// Tab in the empty field takes the example; with text in it, Tab leaves the text alone.
    #[test]
    fn tab_takes_the_example_in_an_empty_field() {
        let mut open = opened("<orders><order status='open'/></orders>");
        let mut bar = XPathBar {
            focus_field: true,
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut frame = |bar: &mut XPathBar, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| bar.show(ui, &mut open));
            out.textures_delta.clear();
        };
        let tab = || egui::Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        // The field asks for the focus, then has it.
        frame(&mut bar, vec![]);
        frame(&mut bar, vec![]);
        frame(&mut bar, vec![tab()]);
        assert_eq!(bar.text, EXAMPLE);
        frame(&mut bar, vec![tab()]);
        frame(&mut bar, vec![tab()]);
        assert_eq!(bar.text, EXAMPLE);
    }

    #[test]
    fn values_and_errors_are_shown_in_the_bar() {
        let mut open = opened("<r><a/><a/></r>");
        let mut bar = XPathBar::default();
        run(&mut bar, &mut open, "count(//a)");
        let r = open.xpath.as_ref().unwrap();
        assert!(matches!(&r.state, State::Done { value: Value::Number(n), .. } if *n == 2.0));
        assert!(!has_results(&open));
        run(&mut bar, &mut open, "//a[");
        let r = open.xpath.as_ref().unwrap();
        assert!(
            matches!(&r.state, State::Failed(e) if e == "expected an expression, found the end at 5"),
        );
        // Drawing the bar with each state changes nothing in the document.
        let ctx = egui::Context::default();
        let before = open.doc.src.clone();
        let mut out = ctx.run_ui(Default::default(), |ui| bar.show(ui, &mut open));
        out.textures_delta.clear();
        assert_eq!(open.doc.src, before);
    }

    /// After an edit the node ids belong to another index: the results are out of date and
    /// select nothing, until the query runs again.
    #[test]
    fn an_edit_makes_the_results_out_of_date() {
        let mut open = opened("<r><a/><b/><a/></r>");
        let mut bar = XPathBar::default();
        run(&mut bar, &mut open, "//a");
        assert!(has_fresh_nodes(&open));
        open.type_edit(Edit::new("Typing").replace(3..3, "<x/>"))
            .unwrap();
        assert!(!has_fresh_nodes(&open));
        open.selected = None;
        step(&mut open, true);
        assert_eq!(open.selected, None);
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| show_results(ui, &mut open));
        out.textures_delta.clear();
        run(&mut bar, &mut open, "//a");
        assert!(has_fresh_nodes(&open));
        assert_eq!(open.xpath.as_ref().unwrap().nodes().unwrap().len(), 2);
    }

    #[test]
    fn a_running_query_stops() {
        let mut src = String::from("<r>");
        for i in 0..200_000 {
            src.push_str(&format!("<a n=\"{i}\"/>"));
        }
        src.push_str("</r>");
        let mut open = opened(&src);
        let mut bar = XPathBar {
            // Each `a` compares with the last one, found again for each: slow on purpose.
            text: "count(//a[@n = //a[last()]/@n])".into(),
            ..Default::default()
        };
        bar.run(&egui::Context::default(), &mut open);
        open.xpath.as_ref().unwrap().stop();
        let t = Instant::now();
        loop {
            poll(&mut open);
            match &open.xpath.as_ref().unwrap().state {
                State::Running { .. } => {}
                State::Failed(e) => {
                    assert_eq!(e, "the query was stopped");
                    break;
                }
                State::Done { .. } => panic!("the query ran to its end"),
            }
            assert!(
                t.elapsed() < Duration::from_secs(10),
                "the query did not stop"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
