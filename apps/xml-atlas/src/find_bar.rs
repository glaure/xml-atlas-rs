//! Find bar above the text view (`docs/spec/text.md`, TEXT-12, TEXT-14, TEXT-26).
//!
//! Ctrl+F opens it, Ctrl+H opens it with a Replace row, Escape closes it. Ctrl+Shift+F, or
//! Find in element… in the tree, keeps Find to the selected element until the bar closes. Enter or F3 finds
//! the next match, Shift+Enter or Shift+F3 the previous one. A match selects the element that
//! contains it, so tree and grid follow, and is outlined in the text. Replace replaces the
//! outlined match and finds the next; Replace All replaces every match, in the whole document
//! or in the selected element, as one undo step.

use crate::lang::Lang;
use std::ops::Range;

use egui::{Key, KeyboardShortcut, Modifiers, RichText, Ui};
use xml_atlas_engine::find::{self, Direction, FindOptions, Finder};
use xml_atlas_engine::text::Selection;
use xml_atlas_engine::{Edit, EditError};

use crate::app::{Note, Open};
use crate::keys;

/// Matches counted at most, so a very common query does not stall typing.
const COUNT_LIMIT: usize = 1_000_000;

pub const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
pub const OPEN_WITHIN: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::F);
pub const OPEN_REPLACE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::H);
pub const NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F3);
pub const PREVIOUS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::F3);

#[derive(Default)]
pub struct FindBar {
    pub visible: bool,
    query: String,
    options: FindOptions,
    /// The compiled query, and the query and options it was built from.
    compiled: Option<Compiled>,
    /// Where each match in the document starts, in order, up to [`COUNT_LIMIT`]; shown after
    /// the buttons as the count and the position of the current match.
    starts: Vec<usize>,
    /// The document and scope `starts` belong to, so that they are counted again when another
    /// file is opened, the document changes or Find keeps to another element.
    counted: Option<DocKey>,
    /// The path of the element Find keeps to, shown in the bar.
    within_label: String,
    /// Where the match the bar last went to starts, while it is outlined (`Open::mark`).
    current: Option<u64>,
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

    /// Shows the bar and puts the cursor in its field, to find in the whole document.
    pub fn open(&mut self, open: Option<&mut Open>) {
        if let Some(open) = open {
            open.find_within = None;
        }
        self.visible = true;
        self.focus_field = true;
    }

    /// Shows the bar to find only in the selected element (TEXT-26), or in the whole
    /// document when none is selected.
    pub fn open_within(&mut self, open: &mut Open) {
        open.find_within = open.selected;
        open.mark = None;
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

    /// Whether there is something to find.
    pub fn has_query(&self) -> bool {
        !self.query.is_empty()
    }

    /// Shows the bar and goes to the next match in `direction`: F3, Shift+F3 and their
    /// entries in the Edit menu.
    pub fn step(&mut self, open: &mut Open, direction: Direction) {
        self.visible = true;
        self.find(open, direction);
    }

    /// Global shortcuts; call once per frame before the panels are drawn.
    pub fn shortcuts(&mut self, ctx: &egui::Context, open: Option<&mut Open>) {
        let Some(open) = open else {
            if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
                self.open(None);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&OPEN_REPLACE)) {
                self.open_replace();
            }
            return;
        };
        // Ctrl+Shift+F first: the plain Ctrl+F shortcut would also match it.
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN_WITHIN)) {
            self.open_within(open);
        } else if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
            self.open(Some(open));
        }
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN_REPLACE)) {
            self.open_replace();
        }
        // Shift+F3 first: the plain F3 shortcut would also match it.
        if ctx.input_mut(|i| i.consume_shortcut(&PREVIOUS)) {
            self.step(open, Direction::Backward);
        } else if ctx.input_mut(|i| i.consume_shortcut(&NEXT)) {
            self.step(open, Direction::Forward);
        }
    }

    pub fn show(&mut self, ui: &mut Ui, open: &mut Open, lang: Lang) {
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
            if open.find_within.is_some() {
                ui.label(RichText::new(format!("in {}", self.within_label)).strong());
                if ui
                    .small_button("✕")
                    .on_hover_text("Find in the whole document")
                    .clicked()
                {
                    open.find_within = None;
                }
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
                .on_hover_text(format!(
                    "Previous ({})",
                    keys::text(ui.ctx(), lang, &PREVIOUS)
                ))
                .clicked()
            {
                direction = Some(Direction::Backward);
            }
            if ui.button("⬇").on_hover_text("Next (F3)").clicked() {
                direction = Some(Direction::Forward);
            }
            let count = self.count(open);
            let status = [count.as_str(), self.note.as_str()]
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
                let key = keys::text(ui.ctx(), lang, &OPEN_REPLACE);
                if ui.button(toggle).on_hover_text(key).clicked() {
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
                if open.find_within.is_some() {
                    ui.label(RichText::new("Replace All keeps to the element too").weak());
                } else {
                    ui.add_enabled(
                        scope(open).is_some(),
                        egui::Checkbox::new(&mut self.in_selection, "In selection"),
                    )
                    .on_hover_text(
                        "Replace All only inside the text selected in the text view or, when \
                         none is, the element selected in tree or grid",
                    );
                }
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
        if !self.visible {
            // Keeping to an element lasts while the bar is open.
            open.find_within = None;
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
        let within = match open.find_within {
            Some(_) => find_range(open),
            None => scope(open)
                .filter(|_| self.in_selection)
                .unwrap_or(0..open.doc.src.len()),
        };
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
        let within = find_range(open);
        let Some(found) = finder.next_within(&open.doc.src, within, from, direction) else {
            self.note = "Not found".into();
            return;
        };
        let range = found.range.start as u64..found.range.end as u64;
        if let Some(id) = open.element_at(range.start) {
            open.select(id);
        }
        open.scroll_text_to = Some(range.start);
        self.current = Some(range.start);
        open.mark = Some(range);
        let element = open.find_within.is_some();
        self.note = match (found.wrapped, direction, element) {
            (false, _, _) => String::new(),
            (true, Direction::Forward, false) => "Continued from the top".into(),
            (true, Direction::Backward, false) => "Continued from the bottom".into(),
            (true, Direction::Forward, true) => "Continued from the element's start".into(),
            (true, Direction::Backward, true) => "Continued from the element's end".into(),
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

    /// The count after the buttons, with the position of the match the bar went to while it
    /// is still outlined; empty while there is nothing to search for.
    fn count(&self, open: &Open) -> String {
        if !self.compiled.as_ref().is_some_and(|c| c.finder.is_some()) {
            return String::new();
        }
        let current = self
            .current
            .filter(|&c| open.mark.as_ref().is_some_and(|m| m.start == c));
        count_label(&self.starts, current)
    }

    /// Counts the matches of the current query for the status line.
    fn recount(&mut self, open: &Open) {
        self.counted = Some(doc_key(open));
        self.note.clear();
        self.within_label = open
            .find_within
            .map(|id| open.doc.index.path(&open.doc.src, id))
            .unwrap_or_default();
        let mut starts = std::mem::take(&mut self.starts);
        starts.clear();
        let src = &open.doc.src;
        let within = find_range(open);
        if let Some(finder) = self.finder() {
            let mut from = within.start;
            while starts.len() < COUNT_LIMIT
                && let Some(found) =
                    finder.next_within(src, within.clone(), from, Direction::Forward)
                && !found.wrapped
            {
                starts.push(found.range.start);
                from = found.range.end.max(found.range.start + 1);
            }
        }
        self.starts = starts;
    }
}

/// The count after the buttons: "No matches", "120 matches", or with the outlined match
/// `current` (where it starts) "3 of 120 matches" (TEXT-12). A match the count skipped, such
/// as one overlapping the one before, or one past [`COUNT_LIMIT`], has no position.
fn count_label(starts: &[usize], current: Option<u64>) -> String {
    let total = match starts.len() {
        0 => return "No matches".into(),
        n if n >= COUNT_LIMIT => format!("{COUNT_LIMIT}+"),
        n => n.to_string(),
    };
    let unit = if starts.len() == 1 {
        "match"
    } else {
        "matches"
    };
    let position = current
        .and_then(|c| usize::try_from(c).ok())
        .and_then(|c| starts.binary_search(&c).ok());
    match position {
        Some(i) => format!("{} of {total} {unit}", i + 1),
        None => format!("{total} {unit}"),
    }
}

/// Address and length of the source, its revision, and the element Find keeps to.
type DocKey = (usize, usize, u64, Option<xml_atlas_engine::xml::NodeId>);

fn doc_key(open: &Open) -> DocKey {
    (
        open.doc.src.as_ptr() as usize,
        open.doc.src.len(),
        open.doc.revision(),
        open.find_within,
    )
}

/// Where Find searches: the element it keeps to, its tags included (TEXT-26), or the whole
/// document.
fn find_range(open: &Open) -> Range<usize> {
    match open.find_within {
        Some(id) => open.doc.index.node(id).span(),
        None => 0..open.doc.src.len(),
    }
}

/// Applies a replacement as one undo step and says what it did. Replacing is plain text, so
/// it can break the markup: then the status bar says so, and Ctrl+Z undoes it.
fn apply(open: &mut Open, edit: Edit, done: String) {
    let was_broken = open.doc.index.error.is_some();
    open.note = Some(match open.edit(|doc| doc.apply(edit).map(|()| true)) {
        Ok(_) => match (&open.doc.index.error, was_broken) {
            (Some(e), false) => Note::refused(format!(
                "{done}; the document is no longer well-formed ({e}). Undo takes it back"
            )),
            _ => Note::info(done),
        },
        Err(EditError::Empty) => Note::info(format!("{done}: already as given, nothing changed")),
        Err(e) => Note::refused(format!("Cannot replace: {e}")),
    });
}

/// What *In selection* limits Replace All to (TEXT-14): the text selected in the text view,
/// or when none is, the element selected in tree or grid, its tags included.
fn scope(open: &Open) -> Option<Range<usize>> {
    match open.text_sel.filter(|s| !s.is_empty()) {
        Some(s) => Some(s.range()),
        None => open.selected.map(|id| open.doc.index.node(id).span()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_label_shows_the_position_of_the_outlined_match() {
        assert_eq!(count_label(&[], None), "No matches");
        assert_eq!(count_label(&[7], None), "1 match");
        assert_eq!(count_label(&[7], Some(7)), "1 of 1 match");
        let starts = [3, 10, 42];
        assert_eq!(count_label(&starts, None), "3 matches");
        assert_eq!(count_label(&starts, Some(3)), "1 of 3 matches");
        assert_eq!(count_label(&starts, Some(42)), "3 of 3 matches");
        // Not where a counted match starts: only the count.
        assert_eq!(count_label(&starts, Some(11)), "3 matches");
    }

    /// F3 and Shift+F3 move the position, and it goes on from the other end; a grid click
    /// that outlines something else leaves only the count.
    #[test]
    fn find_moves_the_position() {
        let src = "\u{feff}<r>\r\n  <a>Söhne</a>\r\n  <a>SÖHNE</a>\r\n  <a>söhne</a>\r\n</r>";
        let doc = xml_atlas_engine::Document::from_bytes(src.as_bytes().to_vec());
        let mut open = Open::new(doc, egui::Id::unique("find-position"));
        let mut bar = FindBar {
            query: "söhne".into(),
            ..FindBar::default()
        };
        bar.recount(&open);
        let label = |bar: &FindBar, open: &Open| bar.count(open);
        assert_eq!(label(&bar, &open), "3 matches");
        bar.find(&mut open, Direction::Forward);
        assert_eq!(label(&bar, &open), "1 of 3 matches");
        bar.find(&mut open, Direction::Forward);
        assert_eq!(label(&bar, &open), "2 of 3 matches");
        bar.find(&mut open, Direction::Backward);
        bar.find(&mut open, Direction::Backward);
        assert_eq!(label(&bar, &open), "3 of 3 matches");
        assert_eq!(bar.note, "Continued from the bottom");
        open.mark = Some(0..3);
        assert_eq!(label(&bar, &open), "3 matches");
    }

    /// Find in element (TEXT-26): count, position and wrapping keep to the element, the
    /// scope follows an edit before it and ends with the element, and Replace All keeps to it.
    #[test]
    fn find_in_element_keeps_to_it() {
        use xml_atlas_engine::command;
        let src = "\u{feff}<r>\r\n  <a>Söhne</a>\r\n  <b><c>söhne</c><c>x</c></b>\r\n  \
                   <b><c>SÖHNE</c><c>söhne</c></b>\r\n</r>\r\n";
        let doc = xml_atlas_engine::Document::from_bytes(src.as_bytes().to_vec());
        let mut open = Open::new(doc, egui::Id::unique("find-within"));
        let b2 = command::resolve(&open.doc, "/r/b[2]").unwrap();
        open.select(b2);
        let mut bar = FindBar {
            query: "söhne".into(),
            ..FindBar::default()
        };
        bar.open_within(&mut open);
        bar.recount(&open);
        assert_eq!(
            (bar.count(&open).as_str(), bar.within_label.as_str()),
            ("2 matches", "/r/b[2]")
        );
        let text = |open: &Open| {
            let m = open.mark.clone().unwrap();
            String::from_utf8_lossy(&open.doc.src[m.start as usize..m.end as usize]).into_owned()
        };
        bar.find(&mut open, Direction::Forward);
        assert_eq!(
            (bar.count(&open), text(&open)),
            ("1 of 2 matches".into(), "SÖHNE".into())
        );
        bar.find(&mut open, Direction::Forward);
        assert_eq!(bar.count(&open), "2 of 2 matches");
        // A match selects its `c`, yet Find still keeps to `b[2]`.
        assert_ne!(open.selected, Some(b2));
        bar.find(&mut open, Direction::Forward);
        assert_eq!(bar.count(&open), "1 of 2 matches");
        assert_eq!(bar.note, "Continued from the element's start");

        // Deleting the `b` before it: the scope follows its element, now `b[1]`.
        let b1 = command::resolve(&open.doc, "/r/b[1]").unwrap();
        let p = open.doc.index.element_position(b1);
        open.command(
            "delete",
            |d| command::delete(d, b1).map(Some),
            crate::app::Shift::Remove(p),
        );
        bar.recount(&open);
        assert_eq!(
            (bar.count(&open).as_str(), bar.within_label.as_str()),
            ("2 matches", "/r/b")
        );

        // Replace All changes only the element's matches; every other byte stays.
        bar.with = "Sons".into();
        bar.replace_all(&mut open);
        let want = src
            .replace("  <b><c>söhne</c><c>x</c></b>\r\n", "")
            .replace("<c>SÖHNE</c><c>söhne</c>", "<c>Sons</c><c>Sons</c>");
        assert_eq!(String::from_utf8_lossy(&open.doc.src), want);

        // Deleting the element itself ends the scope.
        let b = command::resolve(&open.doc, "/r/b").unwrap();
        let p = open.doc.index.element_position(b);
        open.command(
            "delete",
            |d| command::delete(d, b).map(Some),
            crate::app::Shift::Remove(p),
        );
        assert_eq!(open.find_within, None);
    }

    #[test]
    fn count_label_past_the_limit() {
        let starts: Vec<usize> = (0..COUNT_LIMIT).collect();
        assert_eq!(count_label(&starts, Some(4)), "5 of 1000000+ matches");
    }
}
