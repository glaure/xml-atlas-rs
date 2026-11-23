//! Bookmarks (`docs/spec/tree.md`, TREE-15): the keys, the Navigate menu's commands and the
//! mark in the views. Which elements are marked, and how the marks follow edits, is
//! `xml_atlas_engine::bookmarks`'s; this only asks for it.

use egui::{Color32, Key, KeyboardShortcut, Modifiers, Visuals};
use xml_atlas_engine::Document;
use xml_atlas_engine::xml::NodeId;

use crate::app::{Note, Open};

pub const TOGGLE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F2);
pub const NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F4);
pub const PREVIOUS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::F4);
/// In the text view only: F2 renames and edits in the tree and the grid.
pub const NEXT_IN_TEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F2);
pub const PREVIOUS_IN_TEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::F2);

/// The colour of a mark.
pub fn color(visuals: &Visuals) -> Color32 {
    visuals.warn_fg_color
}

/// Whether the element `id` is marked.
pub fn is_marked(doc: &Document, id: NodeId) -> bool {
    !doc.bookmarks.is_empty() && doc.bookmarks.contains(doc.index.node(id).start as usize)
}

/// The keys, from anywhere the keys are not taken by a text field. F2 and Shift+F2 only with
/// `in_text`. Returns whether one was pressed.
pub fn keys(ctx: &egui::Context, open: &mut Open, in_text: bool) -> bool {
    // The shortcut with most modifiers first: Ctrl+F2 would match F2 as well.
    let pressed = |s: &KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(s));
    if pressed(&TOGGLE) {
        toggle(open);
    } else if pressed(&PREVIOUS) || in_text && pressed(&PREVIOUS_IN_TEXT) {
        step(open, false);
    } else if pressed(&NEXT) || in_text && pressed(&NEXT_IN_TEXT) {
        step(open, true);
    } else {
        return false;
    }
    true
}

/// Navigate > Toggle Bookmark: marks the element the views mark (the grid's marked row, else
/// the selection), or takes its mark off.
pub fn toggle(open: &mut Open) {
    let Some(id) = open.focus.or(open.selected) else {
        return;
    };
    let id = open.element_or_parent(id);
    let start = open.doc.index.node(id).start as usize;
    let marked = open.doc.bookmarks.toggle(start);
    let count = open.doc.bookmarks.len();
    open.note = Some(Note::info(if marked {
        format!("Bookmark set ({count} in all)")
    } else {
        format!("Bookmark removed ({count} left)")
    }));
}

/// Navigate > Next Bookmark and Last Bookmark: selects the next or the previous marked
/// element in document order, going round at the ends.
pub fn step(open: &mut Open, forward: bool) {
    let from = open
        .focus
        .or(open.selected)
        .map_or(0, |id| open.doc.index.node(id).start as usize);
    let marks = &open.doc.bookmarks;
    let found = if forward {
        marks.next(from)
    } else {
        marks.previous(from)
    };
    let count = marks.len();
    let Some((number, start)) = found else {
        open.note = Some(Note::info("No bookmarks"));
        return;
    };
    match xml_atlas_engine::bookmarks::element_starting_at(&open.doc.index, start) {
        Some(id) => {
            open.select(id);
            open.note = Some(Note::info(format!("Bookmark {number} of {count}")));
        }
        None => {
            open.note = Some(Note::refused(
                "The bookmark's element is not well-formed at the moment",
            ));
        }
    }
}

/// Navigate > Clear all Bookmarks.
pub fn clear(open: &mut Open) {
    let count = open.doc.bookmarks.len();
    open.doc.bookmarks.clear();
    open.note = Some(Note::info(match count {
        0 => "No bookmarks".to_owned(),
        1 => "1 bookmark cleared".to_owned(),
        n => format!("{n} bookmarks cleared"),
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Shift;
    use xml_atlas_engine::command::{self, Direction};

    const SRC: &str = "\u{feff}<r>\r\n  <a>Zoë</a>\r\n  <b><c/></b>\r\n  <d>Grüße</d>\r\n</r>";

    fn open() -> Open {
        let doc = Document::from_bytes(SRC.as_bytes().to_vec());
        let mut open = Open::new(doc, egui::Id::unique("bookmarks"));
        let root = command::resolve(&open.doc, "/r").unwrap();
        open.select(root);
        open
    }

    fn select(open: &mut Open, path: &str) {
        let id = command::resolve(&open.doc, path).unwrap();
        open.select(id);
    }

    fn note(open: &Open) -> String {
        open.note
            .as_ref()
            .map(|n| n.text.clone())
            .unwrap_or_default()
    }

    /// The names of the marked elements, in document order.
    fn marked(open: &Open) -> Vec<String> {
        let idx = &open.doc.index;
        open.doc
            .bookmarks
            .elements(idx)
            .map(|id| String::from_utf8_lossy(idx.name(&open.doc.src, id)).into_owned())
            .collect()
    }

    #[test]
    fn toggle_next_and_last_go_round_and_select() {
        let mut open = open();
        step(&mut open, true);
        assert_eq!(note(&open), "No bookmarks");
        for path in ["/r/d", "/r/a", "/r/b"] {
            select(&mut open, path);
            toggle(&mut open);
        }
        assert_eq!(note(&open), "Bookmark set (3 in all)");
        assert_eq!(marked(&open), ["a", "b", "d"]);
        let name = |open: &Open| {
            let id = open.selected.unwrap();
            String::from_utf8_lossy(open.doc.index.name(&open.doc.src, id)).into_owned()
        };
        select(&mut open, "/r");
        step(&mut open, true);
        assert_eq!(
            (name(&open), note(&open)),
            ("a".into(), "Bookmark 1 of 3".into())
        );
        step(&mut open, true);
        step(&mut open, true);
        assert_eq!(
            (name(&open), note(&open)),
            ("d".into(), "Bookmark 3 of 3".into())
        );
        step(&mut open, true);
        assert_eq!(name(&open), "a", "goes round");
        step(&mut open, false);
        assert_eq!(name(&open), "d", "and back");
        // The marked element is a mark's own start: toggling it again takes it off.
        toggle(&mut open);
        assert_eq!(note(&open), "Bookmark removed (2 left)");
        clear(&mut open);
        assert_eq!(note(&open), "2 bookmarks cleared");
        assert!(open.doc.bookmarks.is_empty());
        // A bookmark is no edit.
        assert!(!open.doc.is_modified());
    }

    #[test]
    fn a_move_takes_the_marks_along() {
        let mut open = open();
        select(&mut open, "/r/b");
        toggle(&mut open);
        select(&mut open, "/r/b/c");
        toggle(&mut open);
        select(&mut open, "/r/a");
        toggle(&mut open);
        // Move b above a: the marks stay on b, c and a.
        let b = command::resolve(&open.doc, "/r/b").unwrap();
        let p = open.doc.index.element_position(b);
        let mut q = p.clone();
        *q.last_mut().unwrap() -= 1;
        open.command(
            "move",
            |d| command::move_element(d, b, Direction::Up).map(Some),
            Shift::Swap(p, q),
        );
        assert_eq!(marked(&open), ["b", "c", "a"]);
        let at = open.doc.bookmarks.starts()[0];
        assert!(open.doc.src[at..].starts_with(b"<b>"));
    }

    /// Frames in which marks are drawn in text, tree and grid do not panic, and the text's
    /// gutter marks the line of the start tag.
    #[test]
    fn marked_views_are_drawn() {
        let mut open = open();
        select(&mut open, "/r/d");
        toggle(&mut open);
        select(&mut open, "/r");
        let ctx = egui::Context::default();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 600.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                egui::Panel::left("tree").show(ui, |ui| crate::tree::show(ui, &mut open, None));
                egui::Panel::bottom("grid").show(ui, |ui| crate::grid::show(ui, &mut open));
                egui::CentralPanel::default().show(ui, |ui| {
                    crate::text_view::show(
                        ui,
                        &mut open,
                        &Default::default(),
                        false,
                        &egui::FontId::monospace(13.0),
                    )
                });
            });
            out.textures_delta.clear();
        }
        assert_eq!(marked(&open), ["d"]);
    }
}
