//! Keyboard navigation (`docs/spec/tree.md`, TREE-8 and TREE-10).
//!
//! The arrow keys go to the tree or the grid, whichever was clicked last; nothing is taken
//! while a text field has the keyboard. Alt+Up and Alt+Down drill up and down from anywhere.
//! The edit keys (TREE-9, GRID-10) act on the tree's or the grid's marked row.

use egui::{Key, Modifiers};
use xml_atlas_engine::Document;
use xml_atlas_engine::command::Direction;
use xml_atlas_engine::xml::{NONE, NodeId, NodeKind, ROOT};

use crate::app::{Open, Pane};
use crate::grid::{self, GridKey};
use crate::tree::{self, TreeCommand, TreeKey};

const TREE_KEYS: [(Key, TreeKey); 8] = [
    (Key::ArrowUp, TreeKey::Up),
    (Key::ArrowDown, TreeKey::Down),
    (Key::ArrowLeft, TreeKey::Left),
    (Key::ArrowRight, TreeKey::Right),
    (Key::Home, TreeKey::Home),
    (Key::End, TreeKey::End),
    (Key::PageUp, TreeKey::PageUp),
    (Key::PageDown, TreeKey::PageDown),
];

const GRID_KEYS: [(Key, GridKey); 10] = [
    (Key::ArrowUp, GridKey::Up),
    (Key::ArrowDown, GridKey::Down),
    (Key::ArrowLeft, GridKey::Left),
    (Key::ArrowRight, GridKey::Right),
    (Key::F2, GridKey::Edit),
    (Key::Home, GridKey::Home),
    (Key::End, GridKey::End),
    (Key::PageUp, GridKey::PageUp),
    (Key::PageDown, GridKey::PageDown),
    (Key::Enter, GridKey::Enter),
];

pub fn handle(ctx: &egui::Context, open: &mut Open) {
    if ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_key(modifiers, key));
    // Most modifiers first: Alt+arrow would also match Alt+Shift+arrow, and a plain arrow
    // both.
    let alt_shift = Modifiers::ALT | Modifiers::SHIFT;
    for (key, direction) in [
        (Key::ArrowUp, Direction::Up),
        (Key::ArrowDown, Direction::Down),
    ] {
        if pressed(alt_shift, key) {
            if open.keys == Pane::Tree {
                tree_command(open, TreeCommand::Move(direction));
            }
            return;
        }
    }
    if pressed(Modifiers::ALT, Key::ArrowUp) {
        drill_up(open);
        return;
    }
    if pressed(Modifiers::ALT, Key::ArrowDown) {
        drill_down(open);
        return;
    }
    match open.keys {
        Pane::Tree => {
            if pressed(Modifiers::COMMAND, Key::D) {
                tree_command(open, TreeCommand::Duplicate);
            }
            if pressed(Modifiers::NONE, Key::F2) {
                tree_command(open, TreeCommand::Rename);
            }
            if pressed(Modifiers::NONE, Key::Delete) {
                tree_command(open, TreeCommand::Delete);
            }
            for (k, tk) in TREE_KEYS {
                if pressed(Modifiers::NONE, k)
                    && let Some(row) = open.tree.key(&open.doc, tk)
                {
                    tree::select_row(open, row);
                    open.tree.keep_visible(row);
                }
            }
        }
        Pane::Grid => {
            for (k, gk) in GRID_KEYS {
                if pressed(Modifiers::NONE, k) {
                    grid::key(open, gk);
                }
            }
            if pressed(Modifiers::NONE, Key::Backspace) {
                drill_up(open);
            }
        }
    }
}

fn tree_command(open: &mut Open, command: TreeCommand) {
    if let Some(row) = open.tree.selected() {
        tree::run(open, row, command);
    }
}

/// Selects the parent of the selected element and outlines the element it came from, so
/// that Drill Down goes back to it (TREE-10).
pub fn drill_up(open: &mut Open) {
    let Some(sel) = open.selected else { return };
    let parent = open.doc.index.node(sel).parent;
    if parent == ROOT || parent == NONE {
        return;
    }
    open.select(parent);
    open.focus(sel);
}

/// Selects the child of the selected element that leads to the outlined place, or its first
/// child element (TREE-10).
pub fn drill_down(open: &mut Open) {
    let Some(sel) = open.selected else { return };
    let mark = open.mark.as_ref().map(|m| m.start);
    if let Some(child) = drill_down_target(&open.doc, sel, mark) {
        let keep = open.mark.clone().filter(|m| {
            let n = open.doc.index.node(child);
            n.start <= m.start && m.end <= n.end && (m.start, m.end) != (n.start, n.end)
        });
        open.select(child);
        // Still inside the new selection: keep outlining it, so drilling on goes deeper.
        if keep.is_some() {
            open.mark = keep;
        }
    }
}

/// The child element of `parent` that contains byte `mark`, or else its first child element.
fn drill_down_target(doc: &Document, parent: NodeId, mark: Option<u64>) -> Option<NodeId> {
    let idx = &doc.index;
    let elements = || {
        idx.children(parent)
            .filter(|&c| idx.node(c).kind == NodeKind::Element)
    };
    mark.and_then(|m| {
        elements().find(|&c| {
            let n = idx.node(c);
            n.start <= m && m < n.end
        })
    })
    .or_else(|| elements().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "<r><a><x/></a><b><y/><z/></b></r>";

    fn el(doc: &Document, name: &str) -> NodeId {
        (0..doc.index.len() as NodeId)
            .find(|&id| {
                doc.index.node(id).kind == NodeKind::Element
                    && doc.index.name(&doc.src, id) == name.as_bytes()
            })
            .unwrap()
    }

    #[test]
    fn drill_down_follows_the_mark_or_takes_the_first_child() {
        let doc = Document::from_bytes(SRC.as_bytes().to_vec());
        let r = el(&doc, "r");
        let z = SRC.find("<z/>").unwrap() as u64;
        assert_eq!(drill_down_target(&doc, r, None), Some(el(&doc, "a")));
        assert_eq!(drill_down_target(&doc, r, Some(z)), Some(el(&doc, "b")));
        assert_eq!(
            drill_down_target(&doc, el(&doc, "b"), Some(z)),
            Some(el(&doc, "z"))
        );
        // A mark outside every child falls back to the first child.
        assert_eq!(drill_down_target(&doc, r, Some(0)), Some(el(&doc, "a")));
        assert_eq!(drill_down_target(&doc, el(&doc, "x"), None), None);
    }
}
