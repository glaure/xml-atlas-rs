//! Find bar above the text view (`docs/spec/text.md`, TEXT-12, TEXT-14).
//!
//! Ctrl+F opens it, Ctrl+H opens it with a Replace row, Escape closes it. Enter or F3 finds
//! the next match, Shift+Enter or Shift+F3 the previous one. A match selects the element that
//! contains it, so tree and grid follow, and is outlined in the text. Replace replaces the
//! outlined match and finds the next; Replace All replaces every match, in the whole document
//! or in the selected element, as one undo step.

use egui::{Key, KeyboardShortcut, Modifiers, RichText, Ui};
use xml_atlas_engine::find::{self, Direction, FindOptions, Finder};
use xml_atlas_engine::text::Selection;
use xml_atlas_engine::{Edit, EditError};

use crate::app::{Note, Open};

/// Matches counted at most, so a very common query does not stall typing.
const COUNT_LIMIT: usize = 1_000_000;

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
const OPEN_REPLACE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::H);
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
    /// The document `count` belongs to (address and length of its source, and its revision),
    /// so that it is recounted when another file is opened or the document changes.
    counted: Option<(usize, usize, u64)>,
    /// What the last search said: "not found", "continued from the top".
    note: String,
    focus_field: bool,
    /// The Replace row is shown (TEXT-14).
    pub replacing: bool,
    /// The replacement text.
    with: String,
    /// Replace All only inside the selected element.
    in_selection: bool,
    focus_with: bool,
}

struct Compiled {
    query: String,
    options: FindOptions,
    finder: Option<Finder>,
}

impl FindBar {
    pub fn options(&self) -> FindOptions {
        self.options
    }

    /// Sets match case and whole word, as remembered from the last session.
    pub fn set_options(&mut self, options: FindOptions) {
        self.options = options;
        self.compiled = None;
    }

    /// Shows the bar and puts the cursor in its field.
    pub fn open(&mut self) {
        self.visible = true;
        self.focus_field = true;
    }

    /// Shows the bar with the Replace row; the cursor goes to the replacement when there is
    /// already something to find.
    pub fn open_replace(&mut self) {
        self.visible = true;
        self.replacing = true;
        if self.query.is_empty() {
            self.focus_field = true;
        } else {
            self.focus_with = true;
        }
    }

    /// Global shortcuts; call once per frame before the panels are drawn.
    pub fn shortcuts(&mut self, ctx: &egui::Context, open: Option<&mut Open>) {
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
            self.open();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN_REPLACE)) {
            self.open_replace();
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
                let toggle = if self.replacing {
                    "Find only"
                } else {
                    "Replace…"
                };
                if ui.button(toggle).on_hover_text("Ctrl+H").clicked() {
                    self.replacing = !self.replacing;
                    self.focus_with = self.replacing;
                }
            });
        });
        let mut action = None;
        if self.replacing {
            ui.horizontal(|ui| {
                ui.label("Replace with");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.with)
                        .desired_width(260.0)
                        .hint_text("inserted as typed"),
                );
                if std::mem::take(&mut self.focus_with) {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    action = Some(false);
                    field.request_focus();
                }
                if (field.has_focus() || field.lost_focus())
                    && ui.input(|i| i.key_pressed(Key::Escape))
                {
                    self.visible = false;
                }
                if ui
                    .button("Replace")
                    .on_hover_text("Replace the outlined match and find the next (Enter)")
                    .clicked()
                {
                    action = Some(false);
                }
                if ui.button("Replace All").clicked() {
                    action = Some(true);
                }
                ui.add_enabled(
                    scope(open).is_some(),
                    egui::Checkbox::new(&mut self.in_selection, "In selection"),
                )
                .on_hover_text(
                    "Replace All only inside the text selected in the text view or, when none \
                     is, the element selected in tree or grid",
                );
            });
        }
        if let Some(d) = direction {
            self.find(open, d);
        }
        match action {
            Some(true) => self.replace_all(open),
            Some(false) => self.replace(open),
            None => {}
        }
    }

    /// Replaces the outlined match, when it is one, and finds the next. Otherwise only finds
    /// the next, so the first press shows what the second will replace.
    fn replace(&mut self, open: &mut Open) {
        let with = self.with.clone();
        let edit = match (self.finder(), &open.mark) {
            (Some(finder), Some(m)) => {
                let range = m.start as usize..m.end as usize;
                find::replace(finder, &open.doc.src, range.clone(), &with)
                    .map(|e| (e, range.start + with.len()))
            }
            _ => None,
        };
        let Some((edit, after)) = edit else {
            self.find(open, Direction::Forward);
            return;
        };
        apply(open, edit, "Replaced 1 match".into());
        self.find_from(open, after, Direction::Forward);
    }

    /// Replaces every match, in the selected element or the whole document, as one step.
    fn replace_all(&mut self, open: &mut Open) {
        let within = scope(open)
            .filter(|_| self.in_selection)
            .unwrap_or(0..open.doc.src.len());
        let with = self.with.clone();
        let Some(finder) = self.finder() else {
            return;
        };
        match find::replace_all(finder, &open.doc.src, within.clone(), &with) {
            Some((edit, count)) => {
                let done = match count {
                    1 => "Replaced 1 match".to_string(),
                    n => format!("Replaced {n} matches"),
                };
                let before = open.doc.src.len();
                apply(open, edit, done);
                self.note.clear();
                // A text selection still covers what it covered, now replaced.
                if open.text_sel.is_some_and(|s| s.range() == within) {
                    let end = (within.end + open.doc.src.len()).saturating_sub(before);
                    open.text_sel = Some(Selection {
                        anchor: within.start,
                        head: end,
                    });
                }
            }
            None => self.note = "Not found".into(),
        }
    }

    /// Finds the next match in `direction` from the current one, or from the selection.
    fn find(&mut self, open: &mut Open, direction: Direction) {
        let from = match (&open.mark, direction) {
            (Some(m), Direction::Forward) => m.start as usize + 1,
            (Some(m), Direction::Backward) => m.start as usize,
            (None, _) => open
                .selected
                .map_or(0, |id| open.doc.index.node(id).start as usize),
        };
        self.find_from(open, from, direction);
    }

    /// Finds the next match in `direction` from byte `from`.
    fn find_from(&mut self, open: &mut Open, from: usize, direction: Direction) {
        let Some(finder) = self.finder() else {
            self.note = if self.query.is_empty() {
                String::new()
            } else {
                "Cannot search for this text".into()
            };
            return;
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

fn doc_key(open: &Open) -> (usize, usize, u64) {
    (
        open.doc.src.as_ptr() as usize,
        open.doc.src.len(),
        open.doc.revision(),
    )
}

/// Applies a replacement as one undo step and says what it did. Replacing is plain text, so
/// it can break the markup: then the status bar says so, and Ctrl+Z undoes it.
fn apply(open: &mut Open, edit: Edit, done: String) {
    let was_broken = open.doc.index.error.is_some();
    open.note = Some(match open.edit(|doc| doc.apply(edit).map(|()| true)) {
        Ok(_) => match (&open.doc.index.error, was_broken) {
            (Some(e), false) => Note::refused(format!(
                "{done}; the document is no longer well-formed ({e}). Ctrl+Z undoes it"
            )),
            _ => Note::info(done),
        },
        Err(EditError::Empty) => Note::info(format!("{done}: already as given, nothing changed")),
        Err(e) => Note::refused(format!("Cannot replace: {e}")),
    });
}

/// What *In selection* limits Replace All to (TEXT-14): the text selected in the text view,
/// or when none is, the element selected in tree or grid, its tags included.
fn scope(open: &Open) -> Option<std::ops::Range<usize>> {
    match open.text_sel.filter(|s| !s.is_empty()) {
        Some(s) => Some(s.range()),
        None => open.selected.map(|id| open.doc.index.node(id).span()),
    }
}
