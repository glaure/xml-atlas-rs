//! The keys of the commands the menus, context menus and tooltips name (WIN-13). The code that
//! handles a key and the label that shows it use the same constant. The keys of Find, XPath,
//! Go to line, the diagram and the bookmarks are in their modules.

use egui::{Context, Key, KeyboardShortcut, Modifiers};

const fn command(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND, key)
}

const fn command_shift(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), key)
}

const fn plain(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::NONE, key)
}

const fn alt(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::ALT, key)
}

const fn alt_shift(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::ALT.plus(Modifiers::SHIFT), key)
}

pub const NEW: KeyboardShortcut = command(Key::N);
pub const OPEN: KeyboardShortcut = command(Key::O);
pub const SAVE: KeyboardShortcut = command(Key::S);
pub const SAVE_AS: KeyboardShortcut = command_shift(Key::S);
pub const CLOSE: KeyboardShortcut = command(Key::W);
pub const SETTINGS: KeyboardShortcut = command(Key::Comma);
pub const QUIT: KeyboardShortcut = command(Key::Q);
pub const UNDO: KeyboardShortcut = command(Key::Z);
pub const REDO: KeyboardShortcut = command(Key::Y);
pub const REDO_ALSO: KeyboardShortcut = command_shift(Key::Z);
pub const FORMAT: KeyboardShortcut = command(Key::I);
/// Cut, copy, paste and select all arrive as egui's own events and the text view's keys;
/// these only name them.
pub const CUT: KeyboardShortcut = command(Key::X);
pub const COPY: KeyboardShortcut = command(Key::C);
pub const PASTE: KeyboardShortcut = command(Key::V);
pub const SELECT_ALL: KeyboardShortcut = command(Key::A);
pub const WORD_WRAP: KeyboardShortcut = alt(Key::Z);
pub const HELP: KeyboardShortcut = plain(Key::F1);
pub const DRILL_UP: KeyboardShortcut = alt(Key::ArrowUp);
pub const DRILL_DOWN: KeyboardShortcut = alt(Key::ArrowDown);
pub const MOVE_UP: KeyboardShortcut = alt_shift(Key::ArrowUp);
pub const MOVE_DOWN: KeyboardShortcut = alt_shift(Key::ArrowDown);
/// Rename in the tree, edit a cell in the grid.
pub const EDIT: KeyboardShortcut = plain(Key::F2);
pub const DELETE: KeyboardShortcut = plain(Key::Delete);
pub const DUPLICATE: KeyboardShortcut = command(Key::D);
/// Open the grid's marked row or cell.
pub const ENTER: KeyboardShortcut = plain(Key::Enter);
pub const GO_TO_REFERENCE: KeyboardShortcut = plain(Key::F12);

/// A key as it is shown: with Cmd for Ctrl on macOS.
pub fn text(ctx: &Context, shortcut: &KeyboardShortcut) -> String {
    // egui spells the comma out.
    ctx.format_shortcut(shortcut).replace("Comma", ",")
}

/// A tooltip: what a button does, and its key.
pub fn tip(ctx: &Context, what: &str, shortcut: &KeyboardShortcut) -> String {
    format!("{what}    {}", text(ctx, shortcut))
}

/// Whether the key was pressed; it is taken, so nothing after sees it.
pub fn consume(ctx: &Context, shortcut: &KeyboardShortcut) -> bool {
    ctx.input_mut(|i| i.consume_shortcut(shortcut))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::os::OperatingSystem;

    #[test]
    fn shown_with_the_platforms_modifier() {
        let ctx = Context::default();
        ctx.set_os(OperatingSystem::Nix);
        assert_eq!(text(&ctx, &SETTINGS), "Ctrl+,");
        assert_eq!(text(&ctx, &SAVE_AS), "Ctrl+Shift+S");
        assert_eq!(text(&ctx, &MOVE_UP), "Alt+Shift+Up");
        assert_eq!(tip(&ctx, "Save", &SAVE), "Save    Ctrl+S");
        ctx.set_os(OperatingSystem::Mac);
        // The symbols need fonts, which exist inside a frame.
        let mut shown = String::new();
        let mut out = ctx.run_ui(Default::default(), |ui| shown = text(ui.ctx(), &SAVE));
        out.textures_delta.clear();
        assert!(!shown.contains("Ctrl") && shown.ends_with('S'), "{shown}");
    }
}
