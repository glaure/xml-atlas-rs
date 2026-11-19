//! The reference preview (`docs/spec/references.md`, REF-5) and Go to reference target
//! (REF-3).
//!
//! The pane shows the selected element with its references resolved, read-only, as
//! [`references::preview`] makes it. It is made again only when the selection or the
//! document changes, and draws only the lines on screen. Beside it, the same text can be shown
//! as a tree (REF-10, [`show_tree`]): indexed as a document of its own, once per preview.

use std::ops::Range;

use egui::{Color32, FontId, Label, RichText, Sense, Ui, vec2};
use xml_atlas_engine::Document;
use xml_atlas_engine::references::{self, Choice, Preview, RefKind, Resolution, Rules};
use xml_atlas_engine::text;
use xml_atlas_engine::tree::Row;
use xml_atlas_engine::xml::{NONE, NodeId};

use crate::app::{Note, Open};
use crate::highlight::{self, State, after_line_break, line_job};
use crate::theme;
use crate::tree::{self, TreeState};

/// The pane makes at most this much text; the CLI's `preview` makes more.
const LIMIT: usize = 4 * 1024 * 1024;

/// How strongly imported lines and lines with a note are tinted.
const IMPORTED_ALPHA: f32 = 0.18;
const NOTE_ALPHA: f32 = 0.22;
/// Lines kept above an element the preview text scrolls to from its tree, for context.
const CONTEXT_LINES: usize = 3;
/// Share of the line the section chooser's combo box takes, and its widest.
const COMBO_SHARE: f32 = 0.45;
const COMBO_MAX: f32 = 360.0;

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
    /// The lines drawn in the last frame.
    shown: Range<usize>,
    /// Repeats were expanded (REF-11).
    expanded: bool,
    /// The preview's text as a document and its tree (REF-10), made when the tree pane first
    /// shows this preview.
    tree: Option<(Document, TreeState)>,
    /// Lines of the element clicked in the tree, marked in the text.
    marked: Option<Range<usize>>,
    /// Line to show at the top of the text on the next frame.
    scroll_to: Option<usize>,
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
    if let Some(rule) = rules.repeat() {
        let mut on = open.expand_repeats.unwrap_or(rule.enabled);
        if ui
            .checkbox(&mut on, "Expand repeats")
            .on_hover_text(format!(
                "Show an element with {} as that many numbered copies, as the program reads                  it (REF-11)",
                rule.count
            ))
            .changed()
        {
            open.expand_repeats = Some(on);
        }
    }
    let revision = open.doc.revision();
    let choices = chosen(open);
    let repeat = rules
        .repeat()
        .filter(|r| open.expand_repeats.unwrap_or(r.enabled));
    let expanded = repeat.is_some();
    let fresh = open.reference_preview.as_ref().is_some_and(|c| {
        c.element == element
            && c.revision == revision
            && c.choices == choices
            && c.expanded == expanded
    });
    if !fresh {
        let t = std::time::Instant::now();
        let mut preview = references::preview_with(&open.doc, &rules, element, LIMIT, &choices);
        if let Some(rule) = repeat {
            preview = xml_atlas_engine::repeat::expand(&preview, rule, LIMIT);
        }
        log::debug!("reference preview made in {:?}", t.elapsed());
        let mut c = cache(
            element,
            revision,
            open.doc.index.path(&open.doc.src, element),
            preview,
        );
        c.choices = choices;
        c.expanded = expanded;
        open.reference_preview = Some(c);
        // The tree pane (REF-10) is drawn before this one: it shows the new preview next frame.
        ui.ctx().request_repaint();
    }
    let scroll_to = open
        .reference_preview
        .as_mut()
        .and_then(|c| c.scroll_to.take());
    let Some(c) = &open.reference_preview else {
        return;
    };
    // The path alone on its line, cut to the pane's width and whole on hover, so a deep
    // element does not widen the pane (see `choose_sections`).
    ui.add(Label::new(RichText::new(&c.path).strong()).truncate());
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
    ui.horizontal(|ui| {
        ui.label(RichText::new(summary).weak()).on_hover_text(
            "Read-only. Imported lines are tinted; comments starting with \"XML Atlas:\" say \
             what did not resolve, or where a looked-up context came from",
        );
        if ui
            .small_button("Copy")
            .on_hover_text("Copy the preview's text")
            .clicked()
        {
            ui.ctx().copy_text(c.preview.text.clone());
        }
    });
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
    // Rows touch: `show_rows` counts the item spacing into each row, so it must be zero
    // before it works out which rows are on screen, or a band below them stays empty.
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut area = egui::ScrollArea::both()
        .id_salt("reference-preview")
        .auto_shrink(false);
    if let Some(top) = scroll_to {
        area = area.vertical_scroll_offset(top as f32 * row_h);
    }
    let shown = area
        .show_rows(ui, row_h, c.lines.len(), |ui, range| {
            for n in range.clone() {
                let (rect, _) = ui.allocate_exact_size(vec2(width, row_h), Sense::hover());
                if c.marked.as_ref().is_some_and(|m| m.contains(&n)) {
                    // The element clicked in the tree (REF-10), as the text view marks one.
                    ui.painter().rect_filled(rect, 0.0, theme.selected_element);
                }
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
            range
        })
        .inner;
    if let Some(c) = &mut open.reference_preview {
        c.shown = shown;
    }
}

/// The preview as a tree (REF-10): its text indexed as a document of its own, drawn as the
/// main tree is, with its attribute rows and `font`, read-only. A click marks the element's
/// lines in the preview text and scrolls there.
pub fn show_tree(ui: &mut Ui, open: &mut Open, font: Option<&FontId>) {
    let attributes = open.tree.options.attributes;
    let Some(c) = open.reference_preview.as_mut() else {
        ui.label("Select an element to see it resolved, as a tree.");
        return;
    };
    let (doc, state) = c.tree.get_or_insert_with(|| {
        let t = std::time::Instant::now();
        let doc = Document::from_bytes(c.preview.text.clone().into_bytes());
        log::debug!("preview tree indexed in {:?}", t.elapsed());
        let state = TreeState::new(&doc);
        (doc, state)
    });
    state.set_attributes(attributes);
    let preview = &c.preview;
    // Offered where the row's element was copied from the file, greyed out where the preview
    // wrote it itself.
    let menu = |ui: &mut Ui, doc: &Document, row: Row| {
        let found = copied_from(preview, doc, row).is_some();
        let button = ui
            .add_enabled(found, egui::Button::new("Show in the main tree"))
            .on_disabled_hover_text("The preview wrote this itself: it is not in the file");
        button.clicked().then(|| {
            ui.close();
        })
    };
    let clicks = tree::draw(ui, doc, state, font, "reference-tree", NONE, Some(&menu));
    if let Some(id) = clicks.toggle {
        state.toggle(doc, id);
    }
    if let Some(row) = clicks.clicked {
        mark(c, row);
    }
    if let Some((row, ())) = clicks.chosen {
        show_in_main_tree(open, row);
    }
}

/// The file's offset of the node `row` of the preview's tree stands for (its element, for an
/// attribute row), when it was copied from the file (REF-10).
fn copied_from(preview: &Preview, doc: &Document, row: Row) -> Option<u64> {
    // The node itself: a note the preview added is a comment of its own, not its parent.
    preview
        .source_of(doc.index.node(row.id()).start as usize)
        .map(|at| at as u64)
}

/// Shows the element the preview tree's `row` was copied from in the main tree: expanded to,
/// scrolled to and marked there and in the text, as a click in the grid marks one. The
/// selection, and so the preview, stay as they are.
fn show_in_main_tree(open: &mut Open, row: Row) {
    let Some(c) = &open.reference_preview else {
        return;
    };
    if c.revision != open.doc.revision() {
        // The file changed since the preview was made: it is made again this frame.
        return;
    }
    let Some((doc, _)) = &c.tree else {
        return;
    };
    let Some(at) = copied_from(&c.preview, doc, row) else {
        return;
    };
    let id = open.doc.index.innermost_at(at);
    if open.doc.index.node(id).start != at {
        return;
    }
    open.tree.reveal(&open.doc, id);
    open.focus(id);
}

/// Marks `row` in the preview's tree and its element's lines in the preview text, and scrolls
/// the text there (REF-10).
fn mark(c: &mut Cached, row: Row) {
    let Some((doc, state)) = &mut c.tree else {
        return;
    };
    state.mark(row);
    let node = doc.index.node(row.id());
    let line = |at: u64| {
        c.lines
            .partition_point(|l| l.start as u64 <= at)
            .saturating_sub(1)
    };
    let first = line(node.start);
    let last = line(node.end.saturating_sub(1).max(node.start));
    c.marked = Some(first..last + 1);
    c.scroll_to = Some(first.saturating_sub(CONTEXT_LINES));
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
    // Finding the sections walks the whole scope: once per selection and change, not per
    // frame.
    let revision = open.doc.revision();
    let fresh =
        matches!(&open.reference_sections, Some((e, r, _)) if *e == element && *r == revision);
    if !fresh {
        let found = references::sections(&open.doc, rules, element);
        open.reference_sections = Some((element, revision, found));
    }
    let sections = match &open.reference_sections {
        Some((_, _, s)) => s.clone(),
        None => Vec::new(),
    };
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
            // Both cut to the pane's width, shown whole on hover: an egui side panel is never
            // narrower than its content, and a long label made the pane cover the text view.
            let combo = (ui.available_width() * COMBO_SHARE).min(COMBO_MAX);
            let room = (ui.available_width() - combo - ui.spacing().item_spacing.x).max(0.0);
            ui.scope(|ui| {
                ui.set_max_width(room);
                ui.add(Label::new(format!("{} in {scope} from", s.context)).truncate());
            });
            egui::ComboBox::from_id_salt(("reference-section", s.scope))
                .selected_text(label(picked))
                .width(combo)
                .truncate()
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
        shown: 0..0,
        expanded: false,
        tree: None,
        marked: None,
        scroll_to: None,
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

    /// Draws the preview pane, and the tree pane when `tree`, for two frames.
    fn frames(open: &mut Open, tree: bool) {
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                if tree {
                    show_tree(ui, open, None);
                }
                show(ui, open, &FontId::monospace(13.0));
            });
            // No renderer here takes the font atlas.
            out.textures_delta.clear();
        }
    }

    /// The tree shows the preview, not the file: the import replaced by the element it finds,
    /// the attribute reference by its value. It is made only when its pane is shown, and a
    /// click on a row marks that element's lines in the preview text (REF-10).
    #[test]
    fn the_tree_shows_the_resolved_element() {
        let (mut open, dir) = opened("ref-tree");
        let use_ = command::resolve(&open.doc, "/Doc/Use").unwrap();
        open.select(use_);
        frames(&mut open, false);
        let c = open.reference_preview.as_ref().unwrap();
        assert!(c.tree.is_none(), "the tree was made with its pane off");

        frames(&mut open, true);
        let c = open.reference_preview.as_mut().unwrap();
        let (doc, _) = c.tree.as_ref().unwrap();
        let imported = command::resolve(doc, "/Use/Größe/Wert").unwrap();
        let groesse = doc.index.node(imported).parent;
        let use_tag = doc.index.node(doc.index.root_element().unwrap());
        let start = &doc.src[use_tag.start as usize..];
        assert!(
            start.starts_with(b"<Use M=\"5\""),
            "{:?}",
            String::from_utf8_lossy(start)
        );
        // Still in the file as the source says, not resolved.
        assert!(command::resolve(&open.doc, "/Doc/Use/Größe").is_err());

        let c = open.reference_preview.as_mut().unwrap();
        mark(c, Row::Node(groesse));
        let marked = c.marked.clone().unwrap();
        let first = &c.preview.text[c.lines[marked.start].clone()];
        assert!(first.contains("<Größe N=\"5\">"), "{first:?}");
        assert_eq!(
            c.scroll_to,
            Some(marked.start.saturating_sub(CONTEXT_LINES))
        );
        let (_, state) = c.tree.as_ref().unwrap();
        assert_eq!(state.selected(), Some(Row::Node(groesse)));

        // Show in the main tree: the imported element where it stands in the file, marked
        // there; the selection, and so the preview, stay.
        show_in_main_tree(&mut open, Row::Node(groesse));
        let shared = command::resolve(&open.doc, "/Doc/Shared/Größe").unwrap();
        assert_eq!(open.tree.selected(), Some(Row::Node(shared)));
        assert_eq!(open.focus, Some(shared));
        assert_eq!(open.selected, Some(use_));
        // A note the preview added is not in the file.
        let c = open.reference_preview.as_ref().unwrap();
        let (doc, _) = c.tree.as_ref().unwrap();
        let note = (1..doc.index.len() as NodeId)
            .find(|&id| {
                let n = doc.index.node(id);
                doc.src[n.start as usize..].starts_with(b"<!-- XML Atlas")
            })
            .unwrap();
        assert_eq!(copied_from(&c.preview, doc, Row::Node(note)), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// With a `repeat` rule the pane shows the copies, and its tree too; Show in the main
    /// tree on a copy goes to the element it was copied from; switched off, the preview is
    /// made again as written (REF-11).
    #[test]
    fn repeats_expand_in_the_pane_and_its_tree() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-repeat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(references::RULES_FILE),
            r#"{"repeat": {"count": "Count", "start": "Start", "index": ["[Index]"]}}"#,
        )
        .unwrap();
        let file = dir.join("doc.xml");
        let src = "\u{feff}<Doc>\r\n  <Größe Count=\"3\" Start=\"1\" N=\"[Index]\"><W/></Größe>\r\n\
                   </Doc>\r\n";
        std::fs::write(&file, src).unwrap();
        let mut open = Open::new(Document::open(&file).unwrap(), egui::Id::unique("repeat"));
        let root = command::resolve(&open.doc, "/Doc").unwrap();
        open.select(root);
        frames(&mut open, true);

        let c = open.reference_preview.as_ref().unwrap();
        assert!(
            c.preview.text.contains("<Größe3 N=\"3\"><W/></Größe3>"),
            "{}",
            c.preview.text
        );
        let (doc, _) = c.tree.as_ref().unwrap();
        let third = command::resolve(doc, "/Doc/Größe3/W").unwrap();
        show_in_main_tree(&mut open, Row::Node(third));
        let original = command::resolve(&open.doc, "/Doc/Größe/W").unwrap();
        assert_eq!(open.tree.selected(), Some(Row::Node(original)));

        open.expand_repeats = Some(false);
        frames(&mut open, false);
        let c = open.reference_preview.as_ref().unwrap();
        assert!(
            c.preview.text.contains("<Größe Count=\"3\""),
            "{}",
            c.preview.text
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A preview cut at its limit ends inside an element: the tree keeps the elements before
    /// the cut.
    #[test]
    fn a_cut_preview_keeps_the_elements_before_the_cut() {
        let text = "<Use>\r\n  <Größe N=\"5\"><Wert/></Größe>\r\n  <Ha".to_string();
        let mut c = cache(
            0,
            0,
            "/Use".into(),
            Preview {
                text,
                ..Default::default()
            },
        );
        let doc = Document::from_bytes(c.preview.text.clone().into_bytes());
        assert!(command::resolve(&doc, "/Use/Größe/Wert").is_ok());
        let state = TreeState::new(&doc);
        c.tree = Some((doc, state));
        let wert = command::resolve(&c.tree.as_ref().unwrap().0, "/Use/Größe/Wert").unwrap();
        mark(&mut c, Row::Node(wert));
        assert_eq!(c.marked, Some(1..2));
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

    /// However long the element's path and its section's label are, the pane's content fits
    /// the pane: an egui side panel is never narrower than its content, so a wide header
    /// pushed the pane over the text view and its border snapped back when dragged.
    #[test]
    fn a_long_header_fits_the_pane() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-wide-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rules = r#"{"includes": [{"element": "Include", "attribute": "src"}],
                        "contexts": {"Var": {"nearest": "Var", "up_to": "Unit"}}}"#;
        std::fs::write(dir.join(references::RULES_FILE), rules).unwrap();
        let long = "Größe-".repeat(40);
        let levels: Vec<String> = (0..12).map(|i| format!("Äußere-Ebene-{i}")).collect();
        let mut src = format!("\u{feff}<Doc>\r\n<Unit Name=\"{long}\">\r\n");
        for l in &levels {
            src.push_str(&format!("<{l}>\r\n"));
        }
        src.push_str("<General><Var/><Box><Include src=\"Var:Wert\"/></Box></General>\r\n");
        for l in levels.iter().rev() {
            src.push_str(&format!("</{l}>\r\n"));
        }
        src.push_str(&format!(
            "<Sub V=\"{long}\"><Var><Wert>a</Wert></Var></Sub>\r\n</Unit>\r\n</Doc>\r\n"
        ));
        let file = dir.join("doc.xml");
        std::fs::write(&file, src).unwrap();
        let mut open = Open::new(Document::open(&file).unwrap(), egui::Id::unique("wide"));
        let path = format!("/Doc/Unit/{}/General/Box", levels.join("/"));
        let at = command::resolve(&open.doc, &path).unwrap();
        open.select(at);

        let ctx = egui::Context::default();
        let width = 300.0;
        let mut used = 0.0;
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.allocate_ui(vec2(width, 400.0), |ui| {
                    show(ui, &mut open, &FontId::monospace(13.0));
                    used = ui.min_rect().width();
                });
            });
            // No renderer here takes the font atlas.
            out.textures_delta.clear();
        }
        let sections = open
            .reference_sections
            .as_ref()
            .map_or(0, |(_, _, s)| s.len());
        assert_eq!(sections, 1, "the pane did not offer the section");
        assert!(
            used <= width,
            "the pane's content needs {used} points of the {width} it has"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The preview draws as many lines as fit its height, so no band is left empty below
    /// them: egui's `show_rows` counts the item spacing into each row.
    #[test]
    fn the_preview_fills_its_height() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-tall-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(references::RULES_FILE), RULES).unwrap();
        let mut src = String::from("\u{feff}<Doc>\r\n");
        for i in 0..200 {
            src.push_str(&format!("  <Größe N=\"{i}\"/>\r\n"));
        }
        src.push_str("</Doc>\r\n");
        let file = dir.join("doc.xml");
        std::fs::write(&file, src).unwrap();
        let mut open = Open::new(Document::open(&file).unwrap(), egui::Id::unique("tall"));
        let root = command::resolve(&open.doc, "/Doc").unwrap();
        open.select(root);

        let ctx = egui::Context::default();
        let font = FontId::monospace(13.0);
        let height = 800.0;
        let mut room = 0.0;
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.allocate_ui(vec2(300.0, height), |ui| {
                    ui.set_min_height(height);
                    show(ui, &mut open, &font);
                    room = ui.max_rect().bottom();
                });
            });
            out.textures_delta.clear();
        }
        let row_h = ctx.fonts_mut(|f| f.row_height(&font));
        let c = open.reference_preview.as_ref().unwrap();
        // The header takes some of the height; the rest is rows, with at most one cut off.
        let header = 90.0;
        let fit = ((room - header) / row_h).floor() as usize;
        assert!(
            c.shown.len() >= fit,
            "{} lines drawn where {fit} fit",
            c.shown.len()
        );
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
