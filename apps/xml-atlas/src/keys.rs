//! The keys of the commands the menus, context menus and tooltips name (WIN-13). The code that
//! handles a key and the label that shows it use the same constant. The keys of Find, XPath,
//! Go to line, the diagram and the bookmarks are in their modules.

use egui::{Context, Key, KeyboardShortcut, ModifierNames, Modifiers};

use crate::lang::Lang;

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
/// Ctrl+Page Down and Ctrl+Page Up switch tabs too.
pub const NEXT_TAB: KeyboardShortcut = command(Key::Tab);
pub const PREVIOUS_TAB: KeyboardShortcut = command_shift(Key::Tab);
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

/// A key as it is shown: with Cmd for Ctrl on macOS, whose names are the same in every
/// language, and elsewhere with the language's names (LANG-9).
pub fn text(ctx: &Context, lang: Lang, shortcut: &KeyboardShortcut) -> String {
    if ctx.os().is_mac() {
        // egui spells the comma out.
        return ctx.format_shortcut(shortcut).replace("Comma", ",");
    }
    let names = ModifierNames {
        ctrl: lang.key_ctrl(),
        alt: lang.key_alt(),
        shift: lang.key_shift(),
        ..ModifierNames::NAMES
    };
    let mut text = names.format(&shortcut.modifiers, false);
    if !text.is_empty() {
        text.push_str(names.concat);
    }
    text.push_str(key_name(lang, shortcut.logical_key));
    text
}

fn key_name(lang: Lang, key: Key) -> &'static str {
    match key {
        Key::Comma => ",",
        Key::Delete => lang.key_delete(),
        Key::Enter => lang.key_enter(),
        Key::Escape => lang.key_escape(),
        Key::Tab => lang.key_tab(),
        Key::Backspace => lang.key_backspace(),
        Key::PageUp => lang.key_page_up(),
        Key::PageDown => lang.key_page_down(),
        Key::Home => lang.key_home(),
        Key::End => lang.key_end(),
        Key::ArrowUp => lang.key_up(),
        Key::ArrowDown => lang.key_down(),
        Key::ArrowLeft => lang.key_left(),
        Key::ArrowRight => lang.key_right(),
        key => key.name(),
    }
}

/// A tooltip: what a button does, and its key.
pub fn tip(ctx: &Context, lang: Lang, what: &str, shortcut: &KeyboardShortcut) -> String {
    format!("{what}    {}", text(ctx, lang, shortcut))
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
        assert_eq!(text(&ctx, Lang::En, &SETTINGS), "Ctrl+,");
        assert_eq!(text(&ctx, Lang::En, &SAVE_AS), "Ctrl+Shift+S");
        assert_eq!(text(&ctx, Lang::En, &MOVE_UP), "Alt+Shift+Up");
        assert_eq!(tip(&ctx, Lang::En, "Save", &SAVE), "Save    Ctrl+S");
        let german = |s| text(&ctx, Lang::De, s);
        assert_eq!(german(&SAVE_AS), "Strg+Umschalt+S");
        assert_eq!(german(&MOVE_UP), "Alt+Umschalt+Pfeil hoch");
        assert_eq!(german(&DELETE), "Entf");
        assert_eq!(german(&SETTINGS), "Strg+,");
        assert_eq!(german(&HELP), "F1");
        // English names are egui's own.
        for s in [SAVE_AS, MOVE_UP, DELETE, ENTER, NEXT_TAB] {
            assert_eq!(text(&ctx, Lang::En, &s), ctx.format_shortcut(&s), "{s:?}");
        }
        ctx.set_os(OperatingSystem::Mac);
        // The symbols need fonts, which exist inside a frame.
        let mut shown = String::new();
        let mut german = String::new();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            shown = text(ui.ctx(), Lang::En, &SAVE);
            german = text(ui.ctx(), Lang::De, &SAVE);
        });
        out.textures_delta.clear();
        assert!(!shown.contains("Ctrl") && shown.ends_with('S'), "{shown}");
        // macOS names its keys the same in German.
        assert_eq!(german, shown);
    }
}
