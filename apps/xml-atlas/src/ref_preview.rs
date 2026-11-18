//! The reference preview (`docs/spec/references.md`, REF-5) and Go to reference target
//! (REF-3).
//!
//! The pane shows the selected element with its references resolved, read-only, as
//! [`references::preview`] makes it. It is made again only when the selection or the
//! document changes, and draws only the lines on screen.

use std::ops::Range;

use egui::{Color32, FontId, RichText, Sense, Ui, vec2};
use xml_atlas_engine::references::{self, Choice, Preview, RefKind, Resolution, Rules};
use xml_atlas_engine::text;
use xml_atlas_engine::tree::Row;
use xml_atlas_engine::xml::NodeId;

use crate::app::{Note, Open};
use crate::highlight::{self, State, after_line_break, line_job};
use crate::theme;

/// The pane makes at most this much text; the CLI's `preview` makes more.
const LIMIT: usize = 4 * 1024 * 1024;

/// How strongly imported lines and lines with a note are tinted.
const IMPORTED_ALPHA: f32 = 0.18;
const NOTE_ALPHA: f32 = 0.22;

/// A section chosen in the pane (REF-9), kept by position, which follows edits as the
/// selection does; the label checks it is still the same section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredChoice {
    scope: Vec<u32>,
    chosen: Vec<u32>,
    label: String,
}

/// A preview, and what it was made for.
pub struct Cached {
    element: NodeId,
    revision: u64,
    choices: Vec<Choice>,
    path: String,
    preview: Preview,
    /// Byte range of each line in the preview's text, without its line break.
    lines: Vec<Range<usize>>,
    /// The highlighting state each line starts in.
    states: Vec<State>,
    /// Per line: imported (1), with a note (2), both (3) or neither (0).
    marks: Vec<u8>,
    /// Characters of the longest line, for the width to scroll.
    widest: usize,
}

/// Draws the pane. `font` is the text view's.
pub fn show(ui: &mut Ui, open: &mut Open, font: &FontId) {
    let rules = match &open.references {
        None => {
            ui.label(RichText::new("No reference rules for this file").strong());
            ui.label(format!(
                "Put {} next to the file or in a folder above it, then open the file again. \
                 The file says which elements and attributes are references (REF-1).",
                references::RULES_FILE
            ));
            return;
        }
        Some(Err(e)) => {
            ui.label(RichText::new("The reference rules cannot be read").strong());
            ui.label(e.as_str());
            return;
        }
        Some(Ok((_, rules))) => std::sync::Arc::clone(rules),
    };
    let Some(element) = open.selected else {
        ui.label("Select an element to see it with its references resolved.");
        return;
    };
    if open.doc.stale().is_some() {
        ui.label("Waiting for the document to be read again…");
        return;
    }
    choose_sections(ui, open, &rules, element);
    let revision = open.doc.revision();
    let choices = chosen(open);
    let fresh = open
        .reference_preview
        .as_ref()
        .is_some_and(|c| c.element == element && c.revision == revision && c.choices == choices);
    if !fresh {
        let t = std::time::Instant::now();
        let preview = references::preview_with(&open.doc, &rules, element, LIMIT, &choices);
        log::debug!("reference preview made in {:?}", t.elapsed());
        let mut c = cache(
            element,
            revision,
            open.doc.index.path(&open.doc.src, element),
            preview,
        );
        c.choices = choices;
        open.reference_preview = Some(c);
    }
    let Some(c) = &open.reference_preview else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(&c.path).strong());
        if ui
            .small_button("Copy")
            .on_hover_text("Copy the preview's text")
            .clicked()
        {
            ui.ctx().copy_text(c.preview.text.clone());
        }
    });
    let p = &c.preview;
    let mut summary = match p.references {
        0 => "No references".to_string(),
        1 => "1 reference".to_string(),
        n => format!("{n} references"),
    };
    if p.unresolved > 0 {
        summary.push_str(&format!(", {} not resolved", p.unresolved));
    }
    if p.cut {
        summary.push_str(&format!(", cut after {} MiB", LIMIT / (1024 * 1024)));
    }
    ui.label(RichText::new(summary).weak()).on_hover_text(
        "Read-only. Imported lines are tinted; comments starting with \"XML Atlas:\" say \
         what did not resolve, or where a looked-up context came from",
    );
    ui.separator();

    let theme = theme::current(ui);
    let imported = theme
        .visuals
        .selection
        .bg_fill
        .gamma_multiply(IMPORTED_ALPHA);
    let noted = theme.visuals.warn_fg_color.gamma_multiply(NOTE_ALPHA);
    let row_h = ui.fonts_mut(|f| f.row_height(font));
    let char_w = ui.fonts_mut(|f| f.glyph_width(font, '0'));
    let width = (c.widest as f32 * char_w + 16.0).max(ui.available_width());
    egui::ScrollArea::both()
        .id_salt("reference-preview")
        .auto_shrink(false)
        .show_rows(ui, row_h, c.lines.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for n in range {
                let (rect, _) = ui.allocate_exact_size(vec2(width, row_h), Sense::hover());
                let fill = match c.marks[n] {
                    0 => Color32::TRANSPARENT,
                    1 => imported,
                    _ => noted,
                };
                if fill != Color32::TRANSPARENT {
                    ui.painter().rect_filled(rect, 0.0, fill);
                }
                let line = tabs(&p.text[c.lines[n].clone()]);
                let (job, _) = line_job(&line, font, &theme.syntax, c.states[n]);
                let galley = ui.painter().layout_job(job);
                ui.painter()
                    .galley(rect.left_top(), galley, theme.visuals.text_color());
            }
        });
}

/// F12, and Go to reference target in the tree (REF-3): selects the first element the
/// selected reference finds, or says the value it stands for, or why it finds nothing.
pub fn go_to_target(open: &mut Open) {
    let Some(rules) = open.rules().cloned() else {
        open.note = Some(Note::refused(format!(
            "No reference rules for this file: put {} next to it or above it",
            references::RULES_FILE
        )));
        return;
    };
    let Some(site) = open.selected else {
        return;
    };
    let doc = &open.doc;
    let reference = match open.tree.selected() {
        Some(Row::Attribute { element, index }) if element == site => {
            references::references(doc, &rules, site)
                .into_iter()
                .find(|r| r.site == site && r.kind == RefKind::Attribute(index as usize))
        }
        _ => references::include_at(doc, &rules, site),
    };
    let Some(r) = reference else {
        open.note = Some(Note::refused(
            "The selection is not a reference: select one, or one of its attribute rows",
        ));
        return;
    };
    let choices = chosen(open);
    let doc = &open.doc;
    let resolved = references::resolve_with(doc, &rules, &r, r.site, &choices);
    let via = resolved
        .via
        .map(|v| format!(", from {}", doc.index.path(&doc.src, v)))
        .unwrap_or_default();
    open.note = Some(match resolved.resolution {
        Resolution::Nodes(nodes) => {
            let first = nodes[0];
            let path = doc.index.path(&doc.src, first);
            let what = match nodes.len() {
                1 => format!("{} goes to {path}{via}", r.expr),
                n => format!("{} finds {n} elements; the first is {path}{via}", r.expr),
            };
            open.select(first);
            Note::info(what)
        }
        Resolution::Value(v) => {
            let v: String = v.chars().take(200).collect();
            Note::info(format!("{} is the value {v:?}{via}", r.expr))
        }
        Resolution::Skipped => Note::info(format!("{} is left as it is (the rules)", r.expr)),
        Resolution::Runtime(why) | Resolution::Elsewhere(why) | Resolution::Unresolved(why) => {
            Note::refused(format!("{}: {why}", r.expr))
        }
    });
}

/// The chosen sections that still stand (REF-9): a stored choice whose place now holds
/// another section, or none, is left out.
pub fn chosen(open: &Open) -> Vec<Choice> {
    let Some(rules) = open.rules() else {
        return Vec::new();
    };
    let doc = &open.doc;
    open.reference_choices
        .iter()
        .filter_map(|s| {
            let chosen = doc.index.element_at_position(&s.chosen)?;
            let c = references::choice_for(doc, rules, chosen)?;
            (doc.index.element_position(c.scope) == s.scope
                && references::section_label(doc, chosen) == s.label)
                .then_some(c)
        })
        .collect()
}

impl StoredChoice {
    /// The choice after a structured edit that moves elements by `shift`; `None` when its
    /// section or scope was deleted.
    pub fn remap(&self, shift: &crate::app::Shift) -> Option<Self> {
        Some(Self {
            scope: shift.remap(&self.scope)?,
            chosen: shift.remap(&self.chosen)?,
            label: self.label.clone(),
        })
    }
}

/// Stores `choices`, live ones, as positions again: after typing changed the nodes.
pub fn store(open: &mut Open, choices: &[Choice]) {
    open.reference_choices.clear();
    for c in choices {
        choose(open, c.scope, Some(c.chosen));
    }
}

/// Makes `section` the chosen one of its scope, or with `None` drops the choice for
/// `scope`.
pub fn choose(open: &mut Open, scope: NodeId, section: Option<NodeId>) {
    let doc = &open.doc;
    let scope_pos = doc.index.element_position(scope);
    open.reference_choices.retain(|s| s.scope != scope_pos);
    if let Some(section) = section {
        open.reference_choices.push(StoredChoice {
            scope: scope_pos,
            chosen: doc.index.element_position(section),
            label: references::section_label(doc, section),
        });
    }
}

// Private helpers

/// A list per `nearest` context around the selection: which section to resolve it from.
fn choose_sections(ui: &mut Ui, open: &mut Open, rules: &Rules, element: NodeId) {
    let sections = references::sections(&open.doc, rules, element);
    let current = chosen(open);
    for s in sections {
        let picked = current
            .iter()
            .find(|c| c.scope == s.scope)
            .map(|c| c.chosen);
        let doc = &open.doc;
        let label = |id: Option<NodeId>| match id {
            Some(id) => references::section_label(doc, id),
            None => "the nearest that has it".to_string(),
        };
        let scope = references::element_label(doc, s.scope);
        let mut new = picked;
        ui.horizontal(|ui| {
            ui.label(format!("{} in {scope} from", s.context));
            egui::ComboBox::from_id_salt(("reference-section", s.scope))
                .selected_text(label(picked))
                .width(ui.available_width().min(360.0))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut new, None, label(None));
                    for &c in &s.candidates {
                        ui.selectable_value(&mut new, Some(c), label(Some(c)));
                    }
                });
        });
        if new != picked {
            choose(open, s.scope, new);
        }
    }
}

fn cache(element: NodeId, revision: u64, path: String, preview: Preview) -> Cached {
    let text = &preview.text;
    let mut lines = Vec::new();
    let mut at = 0;
    for piece in text.split_inclusive('\n') {
        let end = at + piece.len();
        let body = piece.trim_end_matches(['\n', '\r']);
        lines.push(at..at + body.len());
        at = end;
    }
    if lines.is_empty() {
        lines.push(0..0);
    }
    let mut states = Vec::with_capacity(lines.len());
    let mut state = State::default();
    let mut widest = 0;
    for l in &lines {
        states.push(state);
        let line = tabs(&text[l.clone()]);
        widest = widest.max(line.chars().count());
        state = after_line_break(highlight::end_state(&line, state));
    }
    let mut marks = vec![0u8; lines.len()];
    let starts: Vec<usize> = lines.iter().map(|l| l.start).collect();
    let mut mark = |r: &Range<usize>, bit: u8| {
        let first = starts.partition_point(|&s| s <= r.start).saturating_sub(1);
        let last = starts
            .partition_point(|&s| s < r.end.max(r.start + 1))
            .saturating_sub(1);
        for m in &mut marks[first..=last.max(first)] {
            *m |= bit;
        }
    };
    for r in &preview.imported {
        mark(r, 1);
    }
    for r in &preview.notes {
        mark(r, 2);
    }
    Cached {
        element,
        revision,
        choices: Vec::new(),
        path,
        preview,
        lines,
        states,
        marks,
        widest,
    }
}

fn tabs(line: &str) -> String {
    line.replace('\t', &" ".repeat(text::TAB_WIDTH))
}

#[cfg(test)]
mod tests {
    use super::*;

    use xml_atlas_engine::{Document, Edit, command};

    const RULES: &str = r#"{
        "includes": [{"element": "Include", "attribute": "src"}],
        "attribute_references": true,
        "contexts": {"Shared": {"path": "/Doc/Shared"}}
    }"#;

    /// A rules file next to a document with a byte order mark, CRLF and non-ASCII names.
    fn opened(name: &str) -> (Open, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("xml-atlas-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(references::RULES_FILE), RULES).unwrap();
        let file = dir.join("doc.xml");
        let src = "\u{feff}<Doc>\r\n  <Shared><Größe N=\"5\"><Wert/></Größe></Shared>\r\n  \
                   <Use M=\"[Shared:Größe/@N]\"><Include src=\"Shared:Größe\"/>\
                   <Include src=\"Shared:Fehlt\"/></Use>\r\n</Doc>\r\n";
        std::fs::write(&file, src).unwrap();
        let doc = Document::open(&file).unwrap();
        (Open::new(doc, egui::Id::unique(name)), dir)
    }

    #[test]
    fn rules_next_to_the_file_list_what_finds_nothing() {
        let (open, dir) = opened("ref-problems");
        assert!(open.rules().is_some());
        let warnings: Vec<&str> = open.problems.iter().map(|p| p.message.as_str()).collect();
        assert_eq!(warnings, ["Reference \"Shared:Fehlt\" finds nothing"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn f12_goes_to_the_target_or_says_why_not() {
        let (mut open, dir) = opened("ref-f12");
        let found = command::resolve(&open.doc, "/Doc/Use/Include[1]").unwrap();
        open.select(found);
        go_to_target(&mut open);
        let target = command::resolve(&open.doc, "/Doc/Shared/Größe").unwrap();
        assert_eq!(open.selected, Some(target));
        assert_eq!(
            open.note.as_ref().unwrap().text,
            "Shared:Größe goes to /Doc/Shared/Größe"
        );

        let missing = command::resolve(&open.doc, "/Doc/Use/Include[2]").unwrap();
        open.select(missing);
        go_to_target(&mut open);
        let note = open.note.take().unwrap();
        assert!(
            note.refused && note.text == "Shared:Fehlt: finds nothing",
            "{}",
            note.text
        );
        assert_eq!(open.selected, Some(missing));

        // An attribute row: its value.
        let r#use = command::resolve(&open.doc, "/Doc/Use").unwrap();
        open.select(r#use);
        open.tree.mark(Row::Attribute {
            element: r#use,
            index: 0,
        });
        go_to_target(&mut open);
        assert_eq!(
            open.note.take().unwrap().text,
            "[Shared:Größe/@N] is the value \"5\""
        );

        // Not a reference.
        open.select(target);
        go_to_target(&mut open);
        assert!(open.note.take().unwrap().refused);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn typing_checks_the_references_after_a_pause() {
        let (mut open, dir) = opened("ref-typing");
        // Typing the missing name: the warning goes at once, the check waits.
        let at = open.doc.src.windows(5).position(|w| w == b"Fehlt").unwrap();
        let edit = Edit::new("Typing").replace(at..at + 5, "Größe");
        open.type_edit(edit).unwrap();
        assert!(open.problems.is_empty());
        assert!(open.references_due.is_some());
        open.check_references();
        assert!(open.problems.is_empty(), "{:?}", open.problems);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// REF-9: a chosen section decides F12 and the preview, survives an edit before it, and
    /// is dropped when its place holds another section.
    #[test]
    fn a_chosen_section_decides_and_follows_edits() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-sections-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rules = r#"{"includes": [{"element": "Include", "attribute": "src"}],
                        "contexts": {"Var": {"nearest": "Var", "up_to": "Unit"}}}"#;
        std::fs::write(dir.join(references::RULES_FILE), rules).unwrap();
        let file = dir.join("doc.xml");
        let src = "\u{feff}<Doc>\r\n<Note/>\r\n<Unit>\r\n\
                   <General><Var/><Box><Include src=\"Var:Größe\"/></Box></General>\r\n\
                   <Sub V=\"1\"><Var><Größe>a</Größe></Var></Sub>\r\n\
                   <Sub V=\"2\"><Var><Größe>b</Größe></Var></Sub>\r\n</Unit>\r\n</Doc>\r\n";
        std::fs::write(&file, src).unwrap();
        let mut open = Open::new(Document::open(&file).unwrap(), egui::Id::unique("sections"));
        let include = |o: &Open| command::resolve(&o.doc, "/Doc/Unit/General/Box/Include").unwrap();
        let f12 = |o: &mut Open| {
            let at = include(o);
            o.select(at);
            go_to_target(o);
            o.doc.index.path(&o.doc.src, o.selected.unwrap())
        };
        // No choice: the first Sub that has it.
        assert_eq!(f12(&mut open), "/Doc/Unit/Sub[1]/Var/Größe");

        let unit = command::resolve(&open.doc, "/Doc/Unit").unwrap();
        let sub2 = command::resolve(&open.doc, "/Doc/Unit/Sub[2]/Var").unwrap();
        choose(&mut open, unit, Some(sub2));
        assert_eq!(f12(&mut open), "/Doc/Unit/Sub[2]/Var/Größe");
        let box_ = command::resolve(&open.doc, "/Doc/Unit/General/Box").unwrap();
        let p = references::preview_with(
            &open.doc,
            open.rules().unwrap(),
            box_,
            LIMIT,
            &chosen(&open),
        );
        assert!(p.text.starts_with("<Box><Größe>b</Größe>"), "{}", p.text);

        // Deleting `Note` before the Unit moves it; the choice moves with it.
        let delete = |o: &mut Open, path: &str| {
            let id = command::resolve(&o.doc, path).unwrap();
            let pos = o.doc.index.element_position(id);
            o.command(
                "delete",
                |d| command::delete(d, id).map(Some),
                crate::app::Shift::Remove(pos),
            );
        };
        delete(&mut open, "/Doc/Note");
        assert_eq!(chosen(&open).len(), 1);
        assert_eq!(f12(&mut open), "/Doc/Unit/Sub[2]/Var/Größe");
        // Deleting Sub 1 moves Sub 2 up, and the choice with it.
        delete(&mut open, "/Doc/Unit/Sub[1]");
        assert_eq!(f12(&mut open), "/Doc/Unit/Sub/Var/Größe");
        // Typing an element before it keeps it too.
        let at = open.doc.src.windows(5).position(|w| w == b"<Box>").unwrap();
        open.type_edit(Edit::new("Typing").replace(at..at, "<Neu/>"))
            .unwrap();
        assert_eq!(chosen(&open).len(), 1);
        // Deleting the chosen section drops the choice.
        delete(&mut open, "/Doc/Unit/Sub");
        assert!(open.reference_choices.is_empty());
        let general = command::resolve(&open.doc, "/Doc/Unit/General/Var").unwrap();
        choose(&mut open, unit, Some(general));
        assert_eq!(open.reference_choices.len(), 1);
        choose(&mut open, unit, None);
        assert!(open.reference_choices.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lines_and_marks_follow_the_preview_text() {
        let text = "<a>\r\n  <b/>\r\n  x <!-- XML Atlas: y -->\r\n</a>".to_string();
        let b = text.find("<b/>").unwrap();
        let note = text.find("<!--").unwrap();
        let preview = Preview {
            imported: std::iter::once(b..b + 4).collect(),
            notes: std::iter::once(note..text.find("-->").unwrap() + 3).collect(),
            text,
            ..Preview::default()
        };
        let c = cache(1, 0, "/a".into(), preview);
        let lines: Vec<&str> = c.lines.iter().map(|l| &c.preview.text[l.clone()]).collect();
        assert_eq!(
            lines,
            ["<a>", "  <b/>", "  x <!-- XML Atlas: y -->", "</a>"]
        );
        assert_eq!(c.marks, [0, 1, 2, 0]);
        // The comment's line starts as text; the state carries over line breaks.
        assert!(c.states.iter().all(|&s| s == State::Text));
        assert_eq!(c.widest, 25);
    }
}
