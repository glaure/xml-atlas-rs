//! The editor of a rules file (`docs/spec/references.md`, REF-15; D39, D41): its text, the
//! check beside it as one types, and Save. The file's form and the check are the engine's.

use crate::lang::Lang;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::text::{CCursor, CCursorRange};
use egui::{Id, Key, KeyboardShortcut, Modifiers, RichText, Ui};
use xml_atlas_engine::problems::Severity;
use xml_atlas_engine::references::check::{Report, check};
use xml_atlas_engine::rules_text::{RulesText, TextError};

use crate::app::Open;

/// The check waits for this pause in typing, and after an edit of the document.
pub(crate) const PAUSE: Duration = Duration::from_millis(350);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
/// Above this, a document's matches are counted only when asked: 100 MiB takes about 70 ms,
/// a hitch at each pause in typing.
pub(crate) const COUNT_AT_MOST: usize = 16 * 1024 * 1024;
/// Height of the check below the text.
const CHECK_HEIGHT: f32 = 120.0;

/// What Insert puts at the cursor: the menu's name, where it belongs, and the text.
const TEMPLATES: [(&str, &str, &str); 8] = [
    (
        "Includes",
        "In the outer { }",
        "\"includes\": [\n    {\"element\": \"Include\", \"attribute\": \"src\"}\n  ]",
    ),
    (
        "One include",
        "In includes [ ]",
        "{\"element\": \"Include\", \"attribute\": \"src\"}",
    ),
    (
        "Attribute references",
        "In the outer { }",
        "\"attribute_references\": {\"skip_names_with\": \"-Later\"}",
    ),
    (
        "Context by path",
        "In contexts { }",
        "\"Shared\": {\"path\": \"/Doc/Shared\"}",
    ),
    (
        "Context by nearest element",
        "In contexts { }",
        "\"Var\": {\"nearest\": \"Var\", \"up_to\": \"Unit\"}",
    ),
    (
        "Run-time contexts",
        "In the outer { }",
        "\"runtime_contexts\": [\"Live\"]",
    ),
    (
        "Variables",
        "In the outer { }",
        "\"variables\": {\"$PARENT\": \"parent_name\"}",
    ),
    (
        "Repeat",
        "In the outer { }",
        "\"repeat\": {\"count\": \"Count\", \"index\": [\"[Index]\"]}",
    ),
];

/// The editor, with the file open in it.
#[derive(Default)]
pub struct RulesEditor {
    file: Option<Editing>,
    /// What waits for the answer to "save the changes?".
    leaving: Option<Then>,
    /// The answer allowed quitting; the app takes it.
    quit: bool,
    /// The answer stopped quitting; the app takes that too.
    quit_cancelled: bool,
    error: Option<String>,
    raise: bool,
}

struct Editing {
    disk: RulesText,
    text: String,
    changed_on_disk: bool,
    report: Report,
    /// The document and its revision the report is for.
    checked: Option<Checked>,
    due: Option<Instant>,
    /// The document is too large to count in unasked.
    large: bool,
    count_large: bool,
    /// A cursor to set on the next frame, scrolled to.
    go_to: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Checked(Option<(Id, u64)>);

/// What to do once the changes are saved or given up.
#[derive(Clone)]
enum Then {
    Close,
    Quit,
    Edit(PathBuf),
}

#[derive(Clone, Copy)]
enum Answer {
    Save,
    Discard,
    Cancel,
}

impl RulesEditor {
    pub fn is_open(&self) -> bool {
        self.file.is_some()
    }

    pub fn is_modified(&self) -> bool {
        self.file.as_ref().is_some_and(Editing::is_modified)
    }

    /// Opens `path`; with changes in another file, asks about those first.
    pub fn edit(&mut self, path: &Path) {
        self.raise = true;
        if self.file.as_ref().is_some_and(|f| f.disk.path() == path) {
            return;
        }
        if self.is_modified() {
            self.leaving = Some(Then::Edit(path.to_owned()));
        } else {
            self.load(path);
        }
    }

    /// Whether the program may quit now; if not, the window asks, and [`Self::take_quit`]
    /// says when it may.
    pub fn may_quit(&mut self) -> bool {
        if !self.is_modified() {
            return true;
        }
        self.leaving = Some(Then::Quit);
        self.raise = true;
        false
    }

    pub fn take_quit(&mut self) -> bool {
        std::mem::take(&mut self.quit)
    }

    /// Whether quitting was given up at the question: Cancel, or saving failed.
    pub fn take_quit_cancelled(&mut self) -> bool {
        std::mem::take(&mut self.quit_cancelled)
    }

    #[cfg(test)]
    pub fn set_text(&mut self, text: &str) {
        if let Some(file) = &mut self.file {
            file.text = text.to_owned();
        }
    }

    /// The file got another name in the list.
    pub fn renamed(&mut self, from: &Path, to: PathBuf) {
        if let Some(f) = self.file.as_mut().filter(|f| f.disk.path() == from) {
            f.disk.moved(to);
        }
    }

    /// The file was deleted from the list: closes it, unless it has changes to keep.
    pub fn deleted(&mut self, path: &Path) {
        let unchanged = self
            .file
            .as_ref()
            .is_some_and(|f| f.disk.path() == path && !f.is_modified());
        if unchanged {
            self.file = None;
        }
    }

    /// Looks at the file again: without changes the editor follows it, with changes it says
    /// that it changed.
    pub fn refresh(&mut self) {
        let Some(f) = &mut self.file else { return };
        f.changed_on_disk = f.disk.changed_on_disk();
        if f.changed_on_disk
            && !f.is_modified()
            && let Ok(now) = RulesText::read(f.disk.path())
        {
            *f = Editing::new(now);
        }
    }

    /// Draws the window. `document`: the tab shown, to count matches in. True when the file
    /// was written.
    pub fn show(&mut self, ctx: &egui::Context, lang: Lang, document: Option<&Open>) -> bool {
        let Some(f) = &mut self.file else {
            return false;
        };
        f.check(ctx, document);

        let window_id = Id::unique("rules-editor");
        let text_id = Id::unique("rules-editor-text");
        if std::mem::take(&mut self.raise) {
            ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, window_id));
        }
        // Before the app's Ctrl+S, which saves the document.
        let mut save =
            ctx.memory(|m| m.has_focus(text_id)) && ctx.input_mut(|i| i.consume_shortcut(&SAVE));
        let mut overwrite = false;
        let mut reload = false;
        let mut open = true;
        let mut close = false;
        let screen = ctx.content_rect().size();
        let default = egui::vec2((screen.x - 80.0).min(720.0), (screen.y - 120.0).min(520.0));
        egui::Window::new("Edit rules")
            .id(window_id)
            .open(&mut open)
            .collapsible(false)
            .default_size(default)
            .max_size(screen - egui::vec2(32.0, 64.0))
            .show(ctx, |ui| {
                let name = f
                    .disk
                    .path()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                let mark = if f.is_modified() { " *" } else { "" };
                let title = RichText::new(format!("{name}{mark}")).strong();
                ui.add(egui::Label::new(title).truncate())
                    .on_hover_text(f.disk.path().display().to_string());
                ui.horizontal(|ui| {
                    let can = f.is_modified() && !f.changed_on_disk;
                    save |= ui
                        .add_enabled(can, egui::Button::new("Save"))
                        .on_hover_text(format!(
                            "{}, while the text has the keys",
                            crate::keys::text(ui.ctx(), lang, &SAVE)
                        ))
                        .clicked();
                    if ui
                        .add_enabled(f.is_modified(), egui::Button::new("Revert"))
                        .on_hover_text("The text as last saved")
                        .clicked()
                    {
                        f.text = f.disk.text().to_owned();
                        f.due = Some(Instant::now());
                    }
                    ui.menu_button("Insert", |ui| {
                        for (name, belongs, text) in TEMPLATES {
                            if ui.button(name).on_hover_text(belongs).clicked() {
                                ui.close();
                                f.insert(ui.ctx(), text_id, text);
                            }
                        }
                    });
                    close = ui.button("Close").clicked();
                });
                let red = ui.visuals().error_fg_color;
                if f.changed_on_disk {
                    ui.horizontal(|ui| {
                        ui.colored_label(red, "The file changed on disk.");
                        overwrite = ui.button("Overwrite").clicked();
                        reload = ui.button("Load it, losing my changes").clicked();
                    });
                }
                if let Some(e) = &self.error {
                    ui.colored_label(red, e.as_str());
                }
                ui.separator();
                let height = (ui.available_height() - CHECK_HEIGHT - 20.0).max(60.0);
                f.editor(ui, text_id, height);
                ui.separator();
                f.findings(ui);
            });

        let mut written = false;
        if save || overwrite {
            written = self.save(overwrite);
        } else if reload && let Some(path) = self.file.as_ref().map(|f| f.disk.path().to_owned()) {
            self.load(&path);
        }
        if !open || close {
            if self.is_modified() {
                self.leaving = Some(Then::Close);
            } else {
                self.file = None;
            }
        }
        written | self.ask(ctx)
    }

    fn load(&mut self, path: &Path) {
        match RulesText::read(path) {
            Ok(disk) => {
                self.file = Some(Editing::new(disk));
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// An error from opening, when there is no window to show it.
    pub fn take_open_error(&mut self) -> Option<String> {
        if self.file.is_some() {
            return None;
        }
        self.error.take()
    }

    fn save(&mut self, overwrite: bool) -> bool {
        let Some(f) = &mut self.file else {
            return false;
        };
        match f.disk.save(&f.text, overwrite) {
            Ok(()) => {
                f.changed_on_disk = false;
                self.error = None;
                true
            }
            Err(TextError::Changed(_)) => {
                f.changed_on_disk = true;
                false
            }
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        }
    }

    /// "Save the changes?" while something waits for it. True when that wrote the file.
    fn ask(&mut self, ctx: &egui::Context) -> bool {
        if self.leaving.is_none() {
            return false;
        }
        let mut answer = None;
        let name = self
            .file
            .as_ref()
            .and_then(|f| f.disk.path().file_name())
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let modal = egui::Modal::new(Id::unique("save-rules")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading("Unsaved rules");
            ui.label(format!("Save the changes to {name}?"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (label, a) in [
                    ("Save rules", Answer::Save),
                    ("Don't save", Answer::Discard),
                    ("Cancel", Answer::Cancel),
                ] {
                    if ui.button(label).clicked() {
                        answer = Some(a);
                    }
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(Answer::Cancel);
        }
        answer.is_some_and(|a| self.answer(a))
    }

    /// True when it wrote the file.
    fn answer(&mut self, answer: Answer) -> bool {
        let Some(then) = self.leaving.take() else {
            return false;
        };
        let mut written = false;
        match answer {
            Answer::Cancel => {
                self.quit_cancelled = matches!(then, Then::Quit);
                return false;
            }
            Answer::Save => {
                written = self.save(false);
                if !written {
                    // It changed on disk or cannot be written: the window says which.
                    self.quit_cancelled = matches!(then, Then::Quit);
                    return false;
                }
            }
            Answer::Discard => {}
        }
        match then {
            Then::Close => self.file = None,
            Then::Quit => {
                self.file = None;
                self.quit = true;
            }
            Then::Edit(path) => {
                self.file = None;
                self.load(&path);
            }
        }
        written
    }
}

impl Editing {
    fn new(disk: RulesText) -> Self {
        Self {
            text: disk.text().to_owned(),
            disk,
            changed_on_disk: false,
            report: Report::default(),
            checked: None,
            due: None,
            large: false,
            count_large: false,
            go_to: None,
        }
    }

    fn is_modified(&self) -> bool {
        self.text != self.disk.text()
    }

    /// Checks again once the text or the document has rested for [`PAUSE`].
    fn check(&mut self, ctx: &egui::Context, document: Option<&Open>) {
        self.large = document.is_some_and(|o| o.doc.src.len() > COUNT_AT_MOST);
        let counted = !self.large || self.count_large;
        let document = document.filter(|o| counted && o.doc.stale().is_none());
        let key = Checked(document.map(|o| (o.id, o.doc.revision())));
        let now = Instant::now();
        match self.checked {
            None => self.due = Some(now),
            Some(checked) if checked != key && self.due.is_none() => self.due = Some(now + PAUSE),
            _ => {}
        }
        let Some(due) = self.due else { return };
        if now < due {
            ctx.request_repaint_after(due - now);
            return;
        }
        self.due = None;
        self.report = check(&self.text, document.map(|o| &o.doc));
        self.checked = Some(key);
    }

    fn editor(&mut self, ui: &mut Ui, id: Id, height: f32) {
        egui::ScrollArea::both()
            .id_salt("rules-text")
            .max_height(height)
            .auto_shrink(false)
            .show(ui, |ui| {
                if let Some(at) = self.go_to {
                    set_cursor(ui.ctx(), id, at);
                }
                let out = egui::TextEdit::multiline(&mut self.text)
                    .id(id)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(14)
                    .show(ui);
                if out.response.response.changed() {
                    self.due = Some(Instant::now() + PAUSE);
                }
                if let Some(at) = self.go_to.take() {
                    let rect = out
                        .galley
                        .pos_from_cursor(CCursor::new(at))
                        .translate(out.galley_pos.to_vec2());
                    ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    ui.ctx().memory_mut(|m| m.request_focus(id));
                }
            });
    }

    fn findings(&mut self, ui: &mut Ui) {
        let report = &self.report;
        let warnings = report.findings.len() - report.errors();
        let summary = if self.due.is_some() {
            "Checking…".to_owned()
        } else {
            format!("{} error(s), {warnings} warning(s)", report.errors())
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(summary).strong());
            if self.large {
                ui.checkbox(&mut self.count_large, "Count in this large document")
                    .on_hover_text("Each pause in typing then waits for the count");
            }
        });
        let mut go = None;
        egui::ScrollArea::vertical()
            .id_salt("rules-check")
            .max_height(CHECK_HEIGHT - 20.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for f in &report.findings {
                    let colour = match f.severity {
                        Severity::Error => ui.visuals().error_fg_color,
                        Severity::Warning => ui.visuals().warn_fg_color,
                    };
                    let text = format!("Line {}, column {}: {}", f.line, f.column, f.message);
                    if ui.link(RichText::new(text).color(colour)).clicked() {
                        go = Some((f.line, f.column));
                    }
                }
                if !report.counts.is_empty() {
                    ui.label(RichText::new("Places in the document shown:").weak());
                }
                for c in &report.counts {
                    let text = format!("{}: {}", c.rule, c.places);
                    ui.label(if c.places == 0 {
                        RichText::new(text).color(ui.visuals().warn_fg_color)
                    } else {
                        RichText::new(text).weak()
                    });
                }
            });
        if let Some((line, column)) = go {
            self.go_to = Some(position(&self.text, line, column));
        }
    }

    /// Puts `text` in place of the selection, or at the end without a cursor.
    fn insert(&mut self, ctx: &egui::Context, id: Id, text: &str) {
        let length = self.text.chars().count();
        let selected = egui::TextEdit::load_state(ctx, id)
            .and_then(|state| state.cursor.char_range())
            .map_or(length..length, |range| {
                let range = range.as_sorted_char_range();
                range.start.0.min(length)..range.end.0.min(length)
            });
        let byte = |chars: usize| {
            self.text
                .char_indices()
                .nth(chars)
                .map_or(self.text.len(), |(i, _)| i)
        };
        let bytes = byte(selected.start)..byte(selected.end);
        self.text.replace_range(bytes, text);
        self.go_to = Some(selected.start + text.chars().count());
        self.due = Some(Instant::now() + PAUSE);
    }
}

fn set_cursor(ctx: &egui::Context, id: Id, at: usize) {
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(at))));
    egui::TextEdit::store_state(ctx, id, state);
}

/// The character of `text` at a line and a column counted from 1.
fn position(text: &str, line: usize, column: usize) -> usize {
    let mut at = 0;
    for (i, l) in text.split('\n').enumerate() {
        let length = l.chars().count();
        if i + 1 == line {
            return at + (column - 1).min(length);
        }
        at += length + 1;
    }
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editing(name: &str) -> (RulesEditor, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "xml-atlas-editor-{name}-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, "{\n}\n").unwrap();
        let mut editor = RulesEditor::default();
        editor.edit(&path);
        (editor, path)
    }

    fn set(editor: &mut RulesEditor, text: &str) {
        editor.file.as_mut().unwrap().text = text.to_owned();
    }

    /// Changes are never lost by quitting, closing or editing another file, unless the
    /// user says so.
    #[test]
    fn changes_are_asked_about_before_they_go() {
        let (mut editor, path) = editing("ask");
        assert!(editor.may_quit(), "nothing changed");
        set(&mut editor, "{\"repeat\": 1}");
        assert!(!editor.may_quit());
        editor.answer(Answer::Cancel);
        assert!(editor.is_modified() && !editor.take_quit());

        assert!(!editor.may_quit());
        assert!(editor.answer(Answer::Save));
        assert!(editor.take_quit() && !editor.is_open());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"repeat\": 1}");

        editor.edit(&path);
        set(&mut editor, "{}");
        editor.edit(Path::new("another.json"));
        assert_eq!(
            editor.file.as_ref().unwrap().disk.path(),
            path,
            "asks first"
        );
        assert!(editor.is_modified() && editor.leaving.is_some());
        std::fs::remove_file(&path).unwrap();
    }

    /// A change on disk is followed without changes, and stops Save with some.
    #[test]
    fn a_change_on_disk_is_followed_or_said() {
        let (mut editor, path) = editing("disk");
        std::fs::write(&path, "{ }").unwrap();
        editor.refresh();
        assert_eq!(editor.file.as_ref().unwrap().text, "{ }");

        set(&mut editor, "{\"mine\": 1}");
        std::fs::write(&path, "{\"theirs\": 1}").unwrap();
        editor.refresh();
        assert!(editor.file.as_ref().unwrap().changed_on_disk);
        assert!(!editor.save(false));
        assert!(!editor.may_quit());
        assert!(!editor.answer(Answer::Save), "not over theirs");
        assert!(editor.is_modified() && !editor.take_quit());
        assert!(editor.take_quit_cancelled(), "a failed save stops quitting");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"theirs\": 1}");
        assert!(editor.save(true));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"mine\": 1}");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_finding_is_found_in_characters() {
        let text = "Größe\nü x\n";
        assert_eq!(position(text, 1, 1), 0);
        assert_eq!(position(text, 2, 3), 8);
        assert_eq!(position(text, 2, 99), 9, "the line's end");
        assert_eq!(position(text, 9, 1), 10);
    }
}
