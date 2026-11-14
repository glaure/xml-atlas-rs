//! Find bar above the text view (`docs/spec/text.md`, TEXT-12).
//!
//! Ctrl+F opens it, Escape closes it. Enter or F3 finds the next match, Shift+Enter or
//! Shift+F3 the previous one. A match selects the element that contains it, so tree and grid
//! follow, and is outlined in the text.

use egui::{Key, KeyboardShortcut, Modifiers, RichText, Ui};
use xml_atlas_engine::find::{Direction, FindOptions, Finder};

use crate::app::Open;

/// Matches counted at most, so a very common query does not stall typing.
const COUNT_LIMIT: usize = 1_000_000;

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
const NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F3);
const PREVIOUS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::F3);

#[derive(Default)]
pub struct FindBar {
    pub visible: bool,
    query: String,
    options: FindOptions,
    /// The compiled query, and the query and options it was built from.
    compiled: Option<Compiled>,
    /// Number of matches in the document, shown after the buttons.
    count: String,
    /// The document `count` belongs to (address and length of its source), so that it is
    /// recounted when another file is opened.
    counted: Option<(usize, usize)>,
    /// What the last search said: "not found", "continued from the top".
    note: String,
    focus_field: bool,
}

struct Compiled {
    query: String,
    options: FindOptions,
    finder: Option<Finder>,
}

impl FindBar {
    /// Shows the bar and puts the cursor in its field.
    pub fn open(&mut self) {
        self.visible = true;
        self.focus_field = true;
    }

    /// Global shortcuts; call once per frame before the panels are drawn.
    pub fn shortcuts(&mut self, ctx: &egui::Context, open: Option<&mut Open>) {
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
            self.open();
        }
        let Some(open) = open else { return };
        // Shift+F3 first: the plain F3 shortcut would also match it.
        if ctx.input_mut(|i| i.consume_shortcut(&PREVIOUS)) {
            self.visible = true;
            self.find(open, Direction::Backward);
        } else if ctx.input_mut(|i| i.consume_shortcut(&NEXT)) {
            self.visible = true;
            self.find(open, Direction::Forward);
        }
    }

    pub fn show(&mut self, ui: &mut Ui, open: &mut Open) {
        if self.counted != Some(doc_key(open)) {
            self.recount(open);
        }
        let mut direction = None;
        ui.horizontal(|ui| {
            ui.label("Find");
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .desired_width(260.0)
                    .hint_text("text in the document"),
            );
            if std::mem::take(&mut self.focus_field) {
                field.request_focus();
            }
            if field.changed() {
                self.recount(open);
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let back = ui.input(|i| i.modifiers.shift);
                direction = Some(if back {
                    Direction::Backward
                } else {
                    Direction::Forward
                });
                field.request_focus();
            }
            // The field gives up focus on Escape before we see the key, so check both.
            if (field.has_focus() || field.lost_focus()) && ui.input(|i| i.key_pressed(Key::Escape))
            {
                self.visible = false;
            }
            let mut changed = ui
                .checkbox(&mut self.options.match_case, "Match case")
                .changed();
            changed |= ui
                .checkbox(&mut self.options.whole_word, "Whole word")
                .changed();
            if changed {
                self.recount(open);
            }
            if ui
                .button("⬆")
                .on_hover_text("Previous (Shift+F3)")
                .clicked()
            {
                direction = Some(Direction::Backward);
            }
            if ui.button("⬇").on_hover_text("Next (F3)").clicked() {
                direction = Some(Direction::Forward);
            }
            let status = [self.count.as_str(), self.note.as_str()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("  ·  ");
            ui.label(RichText::new(status).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕").on_hover_text("Close (Escape)").clicked() {
                    self.visible = false;
                }
            });
        });
        if let Some(d) = direction {
            self.find(open, d);
        }
    }

    /// Finds the next match in `direction` from the current one, or from the selection.
    fn find(&mut self, open: &mut Open, direction: Direction) {
        let Some(finder) = self.finder() else {
            self.note = if self.query.is_empty() {
                String::new()
            } else {
                "Cannot search for this text".into()
            };
            return;
        };
        let from = match (&open.mark, direction) {
            (Some(m), Direction::Forward) => m.start as usize + 1,
            (Some(m), Direction::Backward) => m.start as usize,
            (None, _) => open
                .selected
                .map_or(0, |id| open.doc.index.node(id).start as usize),
        };
        let Some(found) = finder.next(&open.doc.src, from, direction) else {
            self.note = "Not found".into();
            return;
        };
        let range = found.range.start as u64..found.range.end as u64;
        if let Some(id) = open.element_at(range.start) {
            open.select(id);
        }
        open.scroll_text_to = Some(open.doc.line_of(range.start));
        open.mark = Some(range);
        self.note = match (found.wrapped, direction) {
            (true, Direction::Forward) => "Continued from the top".into(),
            (true, Direction::Backward) => "Continued from the bottom".into(),
            (false, _) => String::new(),
        };
    }

    /// The finder for the current query and options, rebuilt when they changed.
    fn finder(&mut self) -> Option<&Finder> {
        let stale = self
            .compiled
            .as_ref()
            .is_none_or(|c| c.query != self.query || c.options != self.options);
        if stale {
            self.compiled = Some(Compiled {
                query: self.query.clone(),
                options: self.options,
                finder: Finder::new(&self.query, self.options).ok(),
            });
        }
        self.compiled.as_ref()?.finder.as_ref()
    }

    /// Counts the matches of the current query for the status line.
    fn recount(&mut self, open: &Open) {
        self.counted = Some(doc_key(open));
        self.note.clear();
        let src = &open.doc.src;
        let Some(finder) = self.finder() else {
            self.count.clear();
            return;
        };
        let mut count = 0;
        let mut from = 0;
        while count < COUNT_LIMIT
            && let Some(found) = finder.next(src, from, Direction::Forward)
            && !found.wrapped
        {
            count += 1;
            from = found.range.end.max(found.range.start + 1);
        }
        self.count = match count {
            0 => "No matches".into(),
            1 => "1 match".into(),
            n if n >= COUNT_LIMIT => format!("{COUNT_LIMIT}+ matches"),
            n => format!("{n} matches"),
        };
    }
}

fn doc_key(open: &Open) -> (usize, usize) {
    (open.doc.src.as_ptr() as usize, open.doc.src.len())
}
