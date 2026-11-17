//! The error list below the text (`docs/spec/open.md`, OPEN-13). Clicking an entry goes to it.

use egui::{Color32, RichText, Ui, Visuals};
use xml_atlas_engine::problems::{Problem, Severity};

use crate::app::Open;

/// Height of the list before it scrolls, in entries.
const VISIBLE_ENTRIES: f32 = 5.0;

pub fn show(ui: &mut Ui, open: &mut Open) {
    let errors = count(&open.problems, Severity::Error);
    let warnings = count(&open.problems, Severity::Warning);
    ui.horizontal(|ui| {
        ui.strong("Problems");
        ui.label(RichText::new(format!("{errors} error(s), {warnings} warning(s)")).weak());
    });
    let row_h = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.spacing().interact_size.y);
    let mut go = None;
    // Only the entries on screen are laid out: a broken file can have up to 1,000 (OPEN-10).
    egui::ScrollArea::vertical()
        .id_salt("problems")
        .max_height((row_h + ui.spacing().item_spacing.y) * VISIBLE_ENTRIES)
        .auto_shrink([false, true])
        .show_rows(ui, row_h, open.problems.len(), |ui, rows| {
            for i in rows {
                let p = &open.problems[i];
                let line = open.doc.line_of(p.offset);
                let column = column_of(open, p.offset);
                let text = format!("Line {}, column {}:  {}", line + 1, column + 1, p.message);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(symbol(p.severity)).color(color(ui.visuals(), p.severity)),
                    );
                    if ui.link(text).clicked() {
                        go = Some(i);
                    }
                });
            }
        });
    if let Some(i) = go {
        go_to(open, i);
    }
}

/// Selects the element at problem `i`, outlines the place and scrolls the text to it.
pub fn go_to(open: &mut Open, i: usize) {
    let Some(p) = open.problems.get(i) else {
        return;
    };
    let at = p.offset;
    let end = next_char_boundary(&open.doc.src, at as usize) as u64;
    if let Some(id) = open.element_at(at) {
        open.select(id);
    }
    open.mark = Some(at..end.max(at + 1));
    open.scroll_text_to = Some(at);
}

/// Marker drawn for a problem: `!` for a warning, `×` for an error.
pub fn symbol(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "⚠",
        Severity::Error => "✖",
    }
}

pub fn color(visuals: &Visuals, severity: Severity) -> Color32 {
    match severity {
        Severity::Warning => visuals.warn_fg_color,
        Severity::Error => visuals.error_fg_color,
    }
}

fn count(problems: &[Problem], severity: Severity) -> usize {
    problems.iter().filter(|p| p.severity == severity).count()
}

/// Zero-based column of byte `offset`, in characters.
fn column_of(open: &Open, offset: u64) -> usize {
    let line = open.doc.line_of(offset);
    let start = open.doc.line_starts[line] as usize;
    let end = (offset as usize).min(open.doc.src.len());
    String::from_utf8_lossy(&open.doc.src[start..end])
        .chars()
        .count()
}

/// The byte after the character that starts at `at`; one byte on if that is not UTF-8.
fn next_char_boundary(src: &[u8], at: usize) -> usize {
    let mut i = at + 1;
    while i < src.len() && i < at + 4 && src[i] & 0b1100_0000 == 0b1000_0000 {
        i += 1;
    }
    i.min(src.len())
}
