//! Edit > Go to line… (`docs/spec/text.md`, TEXT-27).
//!
//! Ctrl+G opens a one-field bar. Enter moves the text cursor to `12` or `12:5` (line and
//! column, from 1) and gives the keyboard back to the text; Escape closes it. What the
//! numbers mean is `xml_atlas_engine::text::go_to`'s, which the bar only calls. While the
//! document has an error the field's hint is the line of the first one.

use egui::{Key, KeyboardShortcut, Modifiers, RichText, Ui};
use xml_atlas_engine::problems::Severity;
use xml_atlas_engine::text::{self, Selection};

use crate::app::Open;

pub const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::G);

/// The bar: shared by the tabs.
#[derive(Default)]
pub struct GoToBar {
    pub visible: bool,
    text: String,
    focus_field: bool,
    /// Why the last Enter did nothing; cleared by typing.
    error: Option<String>,
}

impl GoToBar {
    /// Shows the bar with its field selected, ready to type over.
    pub fn open(&mut self) {
        self.visible = true;
        self.focus_field = true;
        self.error = None;
    }

    /// Ctrl+G.
    pub fn shortcuts(&mut self, ctx: &egui::Context, has_document: bool) {
        if has_document && ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
            self.open();
        }
    }

    pub fn close(&mut self, open: Option<&mut Open>) {
        self.visible = false;
        // The keys go back to the text, where Go to line was asked for.
        if let Some(open) = open {
            open.text_focus = true;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, open: &mut Open) {
        let mut go = false;
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label("Go to line");
            let lines = open.doc.line_count();
            let hint = match error_line(open) {
                Some(line) => format!("{line} (the error)"),
                None => format!("1–{lines}, or line:column"),
            };
            let before = self.text.clone();
            let field = egui::TextEdit::singleline(&mut self.text)
                .desired_width(160.0)
                .font(egui::TextStyle::Monospace)
                .hint_text(hint)
                .show(ui)
                .response;
            if self.text != before {
                self.error = None;
            }
            if std::mem::take(&mut self.focus_field) {
                field.request_focus();
                // Selected: a number typed replaces the last one.
                select_all(ui.ctx(), field.id);
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                go = true;
                field.request_focus();
                // The text view takes the keyboard in this very frame: without this it would
                // see the Enter too, and type a line break.
                ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
            }
            // The field gives up focus on Escape before we see the key, so check both.
            if (field.has_focus() || field.lost_focus()) && ui.input(|i| i.key_pressed(Key::Escape))
            {
                close = true;
                ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
            }
            ui.label(RichText::new(format!("of {lines}")).weak());
            if let Some(e) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕").on_hover_text("Close (Escape)").clicked() {
                    close = true;
                }
            });
        });
        if go {
            self.go(open);
        }
        if close {
            self.close(Some(open));
        }
    }

    /// Enter: moves the cursor, or says why not. An empty field takes the error's line.
    fn go(&mut self, open: &mut Open) {
        let input = match (self.text.trim().is_empty(), error_line(open)) {
            (true, Some(line)) => line.to_string(),
            _ => self.text.clone(),
        };
        match text::go_to(&open.doc, &input) {
            Ok(at) => {
                open.text_sel = Some(Selection::cursor(at));
                open.text_goal = None;
                open.text_reveal = true;
                open.scroll_text_to = Some(at as u64);
                self.error = None;
                self.close(Some(open));
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
}

/// The line, from 1, of the document's first error, if it has one.
fn error_line(open: &Open) -> Option<usize> {
    open.problems
        .iter()
        .find(|p| p.severity == Severity::Error)
        .map(|p| open.doc.line_of(p.offset) + 1)
}

fn select_all(ctx: &egui::Context, id: egui::Id) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        let end = egui::text::CCursor::new(usize::MAX);
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                end,
            )));
        state.store(ctx, id);
    }
}
