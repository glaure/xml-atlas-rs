//! The menu bar (WIN-14, D37): File, Edit, View, Navigate, Tools and Help. An entry that does
//! not apply stays in its place, greyed out, and says why on hover.

use egui::{KeyboardShortcut, RichText, Ui};
use xml_atlas_engine::command::Direction;
use xml_atlas_engine::find;
use xml_atlas_engine::xml::{NONE, NodeId, NodeKind, ROOT};

use super::{Action, App, Open, Pane};
use crate::keys;
use crate::lang::Lang;
use crate::menu_keys::{MenuPick, choose, flip};
use crate::settings::{Layout, ThemeChoice};
use crate::settings_window::Section;
use crate::tree::{self, TreeCommand};

/// Why an entry does not apply; `None` when it does.
type WhyNot = Option<&'static str>;

impl App {
    pub(super) fn menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let lang = self.lang();
        // A menu does not stay open behind another window (#167).
        if !ctx.input(|i| i.focused) {
            egui::Popup::close_all(&ctx);
        }
        // A submenu has the keys: Left and Right are its own then.
        let mut sub = false;
        self.menu_keys.before(&ctx);
        egui::MenuBar::new().ui(ui, |ui| {
            let bar = [
                ui.menu_button(lang.menu_file(), |ui| self.file_menu(ui, &mut sub))
                    .response,
                ui.menu_button(lang.menu_edit(), |ui| self.edit_menu(ui, &mut sub))
                    .response,
                ui.menu_button(lang.menu_view(), |ui| self.view_menu(ui, &mut sub))
                    .response,
                ui.menu_button(lang.menu_navigate(), |ui| self.navigate_menu(ui))
                    .response,
                ui.menu_button(lang.menu_tools(), |ui| self.tools_menu(ui, &mut sub))
                    .response,
                ui.menu_button(lang.menu_help(), |ui| self.help_menu(ui))
                    .response,
            ];
            self.menu_keys.bar(&ctx, &bar, sub);
        });
    }

    fn file_menu(&mut self, ui: &mut Ui, sub: &mut bool) {
        let lang = self.lang();
        let ctx = ui.ctx().clone();
        let document = self.needs_document();
        if entry(ui, lang, lang.menu_file_new(), Some(&keys::NEW), None) {
            self.new_document();
        }
        if entry(ui, lang, lang.menu_file_open(), Some(&keys::OPEN), None) {
            self.pick_file(&ctx);
        }
        self.recent_menu(ui, sub);
        ui.separator();
        let modified = self.docs.active().is_some_and(|o| o.doc.is_modified());
        let why = document.or((!modified).then_some(lang.why_no_changes()));
        if entry(ui, lang, lang.menu_file_save(), Some(&keys::SAVE), why) {
            self.save();
        }
        if entry(
            ui,
            lang,
            lang.menu_file_save_as(),
            Some(&keys::SAVE_AS),
            document,
        ) {
            self.save_as();
        }
        let converted = self
            .docs
            .active()
            .is_some_and(|o| o.doc.encoding().is_some());
        let why = document.or((!converted).then_some(lang.why_already_utf8()));
        if entry(ui, lang, lang.menu_file_save_utf8(), None, why) {
            self.save_as_utf8(None);
        }
        ui.separator();
        if entry(
            ui,
            lang,
            lang.menu_file_close(),
            Some(&keys::CLOSE),
            document,
        ) {
            self.close_tab(&ctx);
        }
        ui.separator();
        if entry(
            ui,
            lang,
            lang.menu_file_settings(),
            Some(&keys::SETTINGS),
            None,
        ) {
            self.settings_window.open = true;
        }
        ui.separator();
        if entry(ui, lang, lang.menu_file_quit(), Some(&keys::QUIT), None) {
            self.request(&ctx, Action::Quit);
        }
    }

    /// File > Open Recent (OPEN-12): newest first, the file name with its folder beside it.
    fn recent_menu(&mut self, ui: &mut Ui, sub: &mut bool) {
        let lang = self.lang();
        let ctx = ui.ctx().clone();
        let why = self
            .settings
            .recent
            .is_empty()
            .then_some(lang.why_no_recent());
        let mut chosen = None;
        let mut clear = false;
        submenu(ui, lang.menu_file_recent(), why, sub, |ui| {
            for (i, path) in self.settings.recent.iter().enumerate() {
                let folder = path
                    .parent()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default();
                let name = RichText::new(format!("{}  {}", i + 1, super::file_name(path)));
                let folder =
                    RichText::new(format!("   {}", super::shorten_left(&folder, 48))).weak();
                let button = ui
                    .add(egui::Button::new((name, folder)).wrap_mode(egui::TextWrapMode::Extend))
                    .on_hover_text(path.display().to_string());
                if button.picked() {
                    chosen = Some(path.clone());
                }
            }
            ui.separator();
            if ui.button(lang.menu_file_recent_clear()).picked() {
                clear = true;
            }
        });
        if let Some(path) = chosen {
            ui.close();
            self.open_file(&ctx, path);
        }
        if clear {
            ui.close();
            self.settings.recent.clear();
        }
    }

    fn edit_menu(&mut self, ui: &mut Ui, sub: &mut bool) {
        let lang = self.lang();
        let history = self.docs.active().map(|o| &o.doc.history);
        let undo = history
            .and_then(|h| h.undo_label())
            .map(|step| crate::engine_words::step_name(lang, step));
        let redo = history
            .and_then(|h| h.redo_label())
            .map(|step| crate::engine_words::step_name(lang, step));
        let undo_text = match &undo {
            Some(step) => lang.menu_edit_undo_step(step),
            None => lang.menu_edit_undo().to_owned(),
        };
        let redo_text = match &redo {
            Some(step) => lang.menu_edit_redo_step(step),
            None => lang.menu_edit_redo().to_owned(),
        };
        let why = undo.is_none().then_some(lang.why_no_undo());
        if entry(ui, lang, &undo_text, Some(&keys::UNDO), why) {
            self.undo();
        }
        let why = redo.is_none().then_some(lang.why_no_redo());
        if entry(ui, lang, &redo_text, Some(&keys::REDO), why) {
            self.redo();
        }
        ui.separator();
        self.clipboard_entries(ui);
        ui.separator();
        self.find_entries(ui);
        ui.separator();
        self.element_menu(ui, sub);
    }

    /// Cut, Copy, Paste and Select All (TEXT-17, TEXT-18). They act in the text view: it gets
    /// the keyboard back, and egui hands it the same events the keys make, on the next frame.
    fn clipboard_entries(&mut self, ui: &mut Ui) {
        let lang = self.lang();
        let sel = self.docs.active().and_then(|o| o.text_sel);
        let document = self.needs_document();
        let selection =
            document.or((!sel.is_some_and(|s| !s.is_empty())).then_some(lang.why_no_selection()));
        let items = [
            (
                lang.menu_edit_cut(),
                keys::CUT,
                selection,
                Some(egui::ViewportCommand::RequestCut),
            ),
            (
                lang.menu_edit_copy(),
                keys::COPY,
                selection,
                Some(egui::ViewportCommand::RequestCopy),
            ),
            (
                lang.menu_edit_paste(),
                keys::PASTE,
                document,
                Some(egui::ViewportCommand::RequestPaste),
            ),
            (
                lang.menu_edit_select_all(),
                keys::SELECT_ALL,
                document,
                None,
            ),
        ];
        for (label, shortcut, why, command) in items {
            if entry(ui, lang, label, Some(&shortcut), why) {
                self.clipboard(ui.ctx(), command);
            }
        }
    }

    fn find_entries(&mut self, ui: &mut Ui) {
        let lang = self.lang();
        use crate::find_bar;
        let document = self.needs_document();
        if entry(
            ui,
            lang,
            lang.menu_edit_find(),
            Some(&find_bar::OPEN),
            document,
        ) {
            self.find.open(self.docs.active_mut());
        }
        let why = document.or((!self.find.has_query()).then_some(lang.why_no_query()));
        for (label, shortcut, direction) in [
            (
                lang.menu_edit_find_next(),
                find_bar::NEXT,
                find::Direction::Forward,
            ),
            (
                lang.menu_edit_find_previous(),
                find_bar::PREVIOUS,
                find::Direction::Backward,
            ),
        ] {
            if entry(ui, lang, label, Some(&shortcut), why)
                && let Some(open) = self.docs.active_mut()
            {
                self.find.step(open, direction);
            }
        }
        let selected = self.docs.active().is_some_and(|o| o.selected.is_some());
        let why = document.or((!selected).then_some(lang.why_no_element()));
        if hinted(
            ui,
            lang,
            lang.menu_edit_find_in_element(),
            Some(&find_bar::OPEN_WITHIN),
            why,
            lang.menu_edit_find_in_element_hint(),
        ) && let Some(open) = self.docs.active_mut()
        {
            self.find.open_within(open);
        }
        if entry(
            ui,
            lang,
            lang.menu_edit_replace(),
            Some(&find_bar::OPEN_REPLACE),
            document,
        ) {
            self.find.open_replace();
        }
    }

    /// Edit > Element: the tree's edit commands (TREE-9) on the selected element.
    fn element_menu(&mut self, ui: &mut Ui, sub: &mut bool) {
        let lang = self.lang();
        let why = self.needs_document().or(self
            .selected_element()
            .is_none()
            .then_some(lang.why_no_element()));
        submenu(ui, lang.menu_edit_element(), why, sub, |ui| {
            let Some((open, id)) = self.docs.active().zip(self.selected_element()) else {
                return;
            };
            let root = Some(id) == open.doc.index.root_element();
            let inner = root.then_some(lang.why_root_stays());
            let sibling = |direction, why| {
                inner.or((!tree::has_sibling(&open.doc, id, direction)).then_some(why))
            };
            let items = [
                (
                    lang.menu_element_rename(),
                    keys::EDIT,
                    TreeCommand::Rename,
                    None,
                ),
                (
                    lang.menu_element_delete(),
                    keys::DELETE,
                    TreeCommand::Delete,
                    inner,
                ),
                (
                    lang.menu_element_duplicate(),
                    keys::DUPLICATE,
                    TreeCommand::Duplicate,
                    inner,
                ),
                (
                    lang.menu_element_move_up(),
                    keys::MOVE_UP,
                    TreeCommand::Move(Direction::Up),
                    sibling(Direction::Up, lang.why_nothing_before()),
                ),
                (
                    lang.menu_element_move_down(),
                    keys::MOVE_DOWN,
                    TreeCommand::Move(Direction::Down),
                    sibling(Direction::Down, lang.why_nothing_after()),
                ),
            ];
            let mut chosen = None;
            for (label, shortcut, command, why) in items {
                if entry(ui, lang, label, Some(&shortcut), why) {
                    chosen = Some(command);
                }
            }
            if let Some(command) = chosen {
                self.element_command(command);
            }
        });
    }

    /// Runs `command` on the selected element as its tree row does. The tree shows the row
    /// and gets the arrow keys.
    fn element_command(&mut self, command: TreeCommand) {
        let Some(id) = self.selected_element() else {
            return;
        };
        if let Some(open) = self.docs.active_mut() {
            open.tree.reveal(&open.doc, id);
            open.keys = Pane::Tree;
            tree::run(open, xml_atlas_engine::tree::Row::Node(id), command);
        }
    }

    fn view_menu(&mut self, ui: &mut Ui, sub: &mut bool) {
        let lang = self.lang();
        let document = self.needs_document();
        submenu(ui, lang.menu_view_layout(), None, sub, |ui| {
            for (value, label) in [
                (Layout::GridAbove, lang.switch_grid_above()),
                (Layout::TextAbove, lang.switch_text_above()),
            ] {
                let radio = ui.radio_value(&mut self.settings.layout, value, label);
                choose(&radio, &mut self.settings.layout, value);
            }
            ui.separator();
            if hinted(
                ui,
                lang,
                lang.menu_view_reset_layout(),
                None,
                None,
                lang.menu_view_reset_layout_hint(),
            ) {
                self.settings.panes = Default::default();
            }
        });
        ui.separator();
        self.pane_switches(ui);
        ui.separator();
        let mut show = self.docs.active().is_none_or(|o| o.tree.options.attributes);
        let check = ui
            .add_enabled(
                document.is_none(),
                egui::Checkbox::new(&mut show, lang.switch_attributes()),
            )
            .on_disabled_hover_text(lang.why_no_document());
        if check.changed() | flip(&check, &mut show) {
            // A view setting: for every tab.
            for open in self.docs.iter_mut() {
                open.tree.set_attributes(show);
            }
            self.settings.tree_attributes = show;
        }
        let expand = ui
            .checkbox(
                &mut self.settings.expand_on_text_click,
                lang.switch_expand_on_click(),
            )
            .on_hover_text(lang.switch_expand_on_click_hint());
        flip(&expand, &mut self.settings.expand_on_text_click);
        if entry(ui, lang, lang.menu_view_collapse_tree(), None, document)
            && let Some(open) = self.docs.active_mut()
        {
            open.tree.collapse_all(&open.doc);
        }
        ui.separator();
        self.wrap_switch(ui);
        ui.separator();
        let toolbar = ui.checkbox(&mut self.settings.toolbar, lang.switch_toolbar());
        flip(&toolbar, &mut self.settings.toolbar);
        if entry(ui, lang, lang.menu_view_font(), None, None) {
            self.font_dialog = Some(crate::fonts::FontDialog::new(&self.settings.text_font));
        }
        submenu(ui, lang.menu_view_theme(), None, sub, |ui| {
            for (value, label) in [
                (ThemeChoice::System, lang.switch_theme_system()),
                (ThemeChoice::Light, lang.switch_theme_light()),
                (ThemeChoice::Dark, lang.switch_theme_dark()),
            ] {
                let radio = ui.radio_value(&mut self.settings.theme, value, label);
                choose(&radio, &mut self.settings.theme, value);
            }
        });
    }

    /// View: the grid or the diagram in its place, and the cell pane.
    fn pane_switches(&mut self, ui: &mut Ui) {
        let lang = self.lang();
        let mut grid = self.settings.grid;
        let grid_check = ui
            .checkbox(&mut grid, lang.switch_grid())
            .on_hover_text(lang.switch_grid_hint());
        if grid_check.changed() | flip(&grid_check, &mut grid) {
            self.settings.show_grid(grid);
        }
        let mut diagram = self.settings.diagram;
        let key = keys::text(ui.ctx(), lang, &crate::diagram::TOGGLE);
        let diagram_check = ui
            .add(egui::Checkbox::new(
                &mut diagram,
                (
                    lang.switch_diagram(),
                    egui::Atom::grow(),
                    RichText::new(key).weak(),
                ),
            ))
            .on_hover_text(lang.switch_diagram_hint());
        if diagram_check.changed() | flip(&diagram_check, &mut diagram) {
            self.settings.show_diagram(diagram);
        }
        let cell_pane = ui
            .add_enabled(
                self.settings.grid,
                egui::Checkbox::new(&mut self.settings.cell_pane, lang.switch_cell_pane()),
            )
            .on_disabled_hover_text(lang.why_no_grid());
        flip(&cell_pane, &mut self.settings.cell_pane);
    }

    fn wrap_switch(&mut self, ui: &mut Ui) {
        let lang = self.lang();
        let setting = self.settings.word_wrap;
        let mut wrap = self.docs.active().map_or(setting, |o| o.wraps(setting));
        // A tab with a choice of its own (a minified file, TEXT-23) is named so: the entry
        // then switches that tab alone.
        let own = self.docs.active().is_some_and(|o| o.wrap.is_some());
        let label = if own {
            lang.switch_wrap_tab()
        } else {
            lang.switch_wrap()
        };
        let key = keys::text(ui.ctx(), lang, &keys::WORD_WRAP);
        let wrap_check = ui.add(egui::Checkbox::new(
            &mut wrap,
            (label, egui::Atom::grow(), RichText::new(key).weak()),
        ));
        if wrap_check.changed() || wrap_check.released_over() {
            self.toggle_wrap();
        }
    }

    fn navigate_menu(&mut self, ui: &mut Ui) {
        let lang = self.lang();
        self.drill_entries(ui);
        ui.separator();
        let document = self.needs_document();
        if entry(
            ui,
            lang,
            lang.menu_navigate_reference(),
            Some(&keys::GO_TO_REFERENCE),
            self.no_reference_target(),
        ) && let Some(open) = self.docs.active_mut()
        {
            crate::ref_preview::go_to_target(open);
        }
        if entry(
            ui,
            lang,
            lang.menu_navigate_line(),
            Some(&crate::goto_bar::OPEN),
            document,
        ) {
            self.goto.open();
        }
        ui.separator();
        let why = document.or((self.docs.len() < 2).then_some(lang.why_one_tab()));
        if entry(
            ui,
            lang,
            lang.menu_navigate_next_tab(),
            Some(&keys::NEXT_TAB),
            why,
        ) {
            self.switch_tab(false);
        }
        if entry(
            ui,
            lang,
            lang.menu_navigate_previous_tab(),
            Some(&keys::PREVIOUS_TAB),
            why,
        ) {
            self.switch_tab(true);
        }
        ui.separator();
        self.bookmark_entries(ui);
    }

    /// Drill Up and Drill Down (TREE-10).
    fn drill_entries(&mut self, ui: &mut Ui) {
        let lang = self.lang();
        let document = self.needs_document();
        let at = self.docs.active().zip(self.selected_element());
        let up = document.or(match at {
            None => Some(lang.why_no_element()),
            Some((open, id)) => matches!(open.doc.index.node(id).parent, ROOT | NONE)
                .then_some(lang.why_no_parent()),
        });
        let down = document.or(match at {
            None => Some(lang.why_no_element()),
            Some((open, id)) => {
                let idx = &open.doc.index;
                (!idx
                    .children(id)
                    .any(|c| idx.node(c).kind == NodeKind::Element))
                .then_some(lang.why_no_children())
            }
        });
        if entry(
            ui,
            lang,
            lang.menu_navigate_drill_up(),
            Some(&keys::DRILL_UP),
            up,
        ) && let Some(open) = self.docs.active_mut()
        {
            crate::nav::drill_up(open);
        }
        if entry(
            ui,
            lang,
            lang.menu_navigate_drill_down(),
            Some(&keys::DRILL_DOWN),
            down,
        ) && let Some(open) = self.docs.active_mut()
        {
            crate::nav::drill_down(open);
        }
    }

    /// The bookmarks (TREE-15).
    fn bookmark_entries(&mut self, ui: &mut Ui) {
        let lang = self.lang();
        use crate::bookmarks;
        let marks = self.docs.active().map_or(0, |o| o.doc.bookmarks.len());
        let document = self.needs_document();
        let marked = document.or((marks == 0).then_some(lang.why_no_marks()));
        type Item<'a> = (&'a str, Option<KeyboardShortcut>, WhyNot, fn(&mut Open));
        let items: [Item; 4] = [
            (
                lang.menu_navigate_bookmark_toggle(),
                Some(bookmarks::TOGGLE),
                document,
                bookmarks::toggle,
            ),
            (
                lang.menu_navigate_bookmark_next(),
                Some(bookmarks::NEXT),
                marked,
                |open| bookmarks::step(open, true),
            ),
            (
                lang.menu_navigate_bookmark_last(),
                Some(bookmarks::PREVIOUS),
                marked,
                |open| bookmarks::step(open, false),
            ),
            (
                lang.menu_navigate_bookmark_clear(),
                None,
                marked,
                bookmarks::clear,
            ),
        ];
        for (label, shortcut, why, run) in items {
            if entry(ui, lang, label, shortcut.as_ref(), why)
                && let Some(open) = self.docs.active_mut()
            {
                run(open);
            }
        }
    }

    fn tools_menu(&mut self, ui: &mut Ui, sub: &mut bool) {
        let lang = self.lang();
        let document = self.needs_document();
        if hinted(
            ui,
            lang,
            lang.menu_tools_xpath(),
            Some(&crate::xpath_bar::OPEN),
            document,
            lang.menu_tools_xpath_hint(),
        ) {
            self.xpath.open();
        }
        ui.separator();
        if entry(
            ui,
            lang,
            lang.menu_tools_format(),
            Some(&keys::FORMAT),
            document,
        ) {
            self.format_document();
        }
        if entry(ui, lang, lang.menu_tools_compact(), None, document) {
            self.compact_document();
        }
        if entry(ui, lang, lang.menu_tools_format_options(), None, None) {
            self.format_dialog = Some(self.settings.format);
        }
        ui.separator();
        submenu(ui, lang.menu_tools_references(), None, sub, |ui| {
            crate::ref_preview::switches(ui, lang, &mut self.settings);
            ui.separator();
            if hinted(
                ui,
                lang,
                lang.menu_tools_rules_files(),
                None,
                None,
                lang.menu_tools_rules_files_hint(),
            ) {
                self.settings_window.show_section(Section::References);
            }
        });
    }

    fn help_menu(&mut self, ui: &mut Ui) {
        let lang = self.lang();
        if entry(ui, lang, lang.menu_help_guide(), Some(&keys::HELP), None) {
            self.help.guide = true;
        }
        let why = self.updates.running().then_some(lang.why_check_running());
        if entry(ui, lang, lang.menu_help_updates(), None, why) {
            self.check_for_updates_now(ui.ctx());
        }
        if entry(ui, lang, lang.menu_help_about(), None, None) {
            self.help.about.open = true;
        }
    }

    fn needs_document(&self) -> WhyNot {
        self.docs
            .is_empty()
            .then_some(self.lang().why_no_document())
    }

    fn selected_element(&self) -> Option<NodeId> {
        self.docs.active().and_then(|o| o.selected)
    }

    /// Why Go to reference target (REF-3) cannot be tried. Whether the selection is a
    /// reference is told when it is chosen.
    fn no_reference_target(&self) -> WhyNot {
        let lang = self.lang();
        let Some(open) = self.docs.active() else {
            return Some(lang.why_no_document());
        };
        if open.selected.is_none() {
            Some(lang.why_no_element())
        } else if !self.settings.references {
            Some(lang.why_references_off())
        } else {
            open.rules().is_none().then_some(lang.why_no_rules())
        }
    }
}

/// A menu entry, with its key. True when it was chosen; the menu closes then.
fn entry(
    ui: &mut Ui,
    lang: Lang,
    label: &str,
    shortcut: Option<&KeyboardShortcut>,
    why_not: WhyNot,
) -> bool {
    hinted(ui, lang, label, shortcut, why_not, "")
}

/// [`entry`] with a hint on hover while it applies.
fn hinted(
    ui: &mut Ui,
    lang: Lang,
    label: &str,
    shortcut: Option<&KeyboardShortcut>,
    why_not: WhyNot,
    hint: &str,
) -> bool {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(keys::text(ui.ctx(), lang, shortcut));
    }
    let mut response = ui.add_enabled(why_not.is_none(), button);
    if !hint.is_empty() {
        response = response.on_hover_text(hint);
    }
    if let Some(why) = why_not {
        response = response.on_disabled_hover_text(why);
    }
    let chosen = response.picked();
    if chosen {
        ui.close();
    }
    chosen
}

/// A submenu. Sets `has_keys` while it is open or its name has the keyboard: Left and Right
/// are its own then.
fn submenu(
    ui: &mut Ui,
    label: &str,
    why_not: WhyNot,
    has_keys: &mut bool,
    content: impl FnOnce(&mut Ui),
) {
    ui.add_enabled_ui(why_not.is_none(), |ui| {
        let button = ui.menu_button(label, content);
        *has_keys |= button.response.has_focus() || button.inner.is_some();
        if let Some(why) = why_not {
            button.response.on_disabled_hover_text(why);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguageChoice;
    use crate::settings::Settings;
    use egui::{Event, Pos2, Rect};
    use xml_atlas_engine::Document;
    use xml_atlas_engine::Edit;

    const SRC: &str = "\u{feff}<r>\r\n  <a>Zoë</a>\r\n  <b><c/></b>\r\n</r>\r\n";

    /// The menu bar in a window of its own; only the menu is drawn, so no settings are
    /// written.
    struct Bar {
        ctx: egui::Context,
        app: App,
        /// Whether the tree is drawn under the menu too.
        tree: bool,
        /// Or the grid.
        grid: bool,
        /// The texts of the last frame, with where they are.
        texts: Vec<(String, Rect)>,
    }

    impl Bar {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = App::new(&cc, Vec::new(), None);
            app.settings = Settings::default();
            Self {
                ctx,
                app,
                tree: false,
                grid: false,
                texts: Vec::new(),
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = egui::RawInput {
                events,
                focused: true,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 900.0))),
                ..Default::default()
            };
            let app = &mut self.app;
            let (tree, grid, lang) = (self.tree, self.grid, app.lang());
            let mut out = self.ctx.run_ui(input, |ui| {
                egui::Panel::top("menu").show(ui, |ui| app.menu(ui));
                if let Some(open) = app.docs.active_mut().filter(|_| tree) {
                    egui::CentralPanel::default().show(ui, |ui| tree::show(ui, open, None, lang));
                }
                if let Some(open) = app.docs.active_mut().filter(|_| grid) {
                    open.scan_grid(ui.ctx());
                    egui::CentralPanel::default().show(ui, |ui| crate::grid::show(ui, open, lang));
                }
            });
            out.textures_delta.clear();
            self.texts.clear();
            for clipped in out.shapes {
                collect(clipped.shape, &mut self.texts);
            }
        }

        fn settle(&mut self) {
            for _ in 0..4 {
                self.frame(Vec::new());
            }
        }

        fn at(&self, text: &str) -> Pos2 {
            let found = self.texts.iter().find(|(t, _)| t == text);
            found
                .unwrap_or_else(|| panic!("no {text:?} in {:?}", self.names()))
                .1
                .center()
        }

        fn point_at(&mut self, text: &str) {
            let pos = self.at(text);
            self.frame(vec![Event::PointerMoved(pos)]);
            self.settle();
        }

        fn right_click(&mut self, text: &str) {
            let pos = self.at(text);
            let button = |pressed| Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(pos)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.settle();
        }

        fn click(&mut self, text: &str) {
            let pos = self.at(text);
            let button = |pressed| Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(pos)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.settle();
        }

        fn press(&mut self, key: egui::Key) {
            let event = |pressed| Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![event(true)]);
            self.frame(vec![event(false)]);
            // A menu opened from the keys is entered some frames later.
            for _ in 0..3 {
                self.settle();
            }
        }

        fn shows(&self, text: &str) -> bool {
            self.texts.iter().any(|(t, _)| t == text)
        }

        /// The text of the entry that has the keyboard.
        fn focused(&self) -> Option<&str> {
            let id = self.ctx.memory(|m| m.focused())?;
            let rect = self.ctx.read_response(id)?.rect;
            let inside = |r: &Rect| rect.contains(r.center());
            let found = self.texts.iter().find(|(_, r)| inside(r));
            found.map(|(t, _)| t.as_str())
        }

        fn names(&self) -> Vec<&str> {
            self.texts.iter().map(|(t, _)| t.as_str()).collect()
        }

        /// The texts below the bar, without the keys, from top to bottom.
        fn entries(&self) -> Vec<&str> {
            let bar = self.at("Help").y + 10.0;
            let mut entries: Vec<&(String, Rect)> = self
                .texts
                .iter()
                .filter(|(t, r)| r.center().y > bar && !is_key(t))
                .collect();
            entries.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
            entries.iter().map(|(t, _)| t.as_str()).collect()
        }
    }

    /// A key's text: with modifiers, however the platform names them, a `+` joins them,
    /// which no entry's name has; or it is a function key.
    fn is_key(text: &str) -> bool {
        let function = |t: &str| t.strip_prefix('F').is_some_and(|n| n.parse::<u8>().is_ok());
        text.contains('+') || function(text)
    }

    fn collect(shape: egui::Shape, out: &mut Vec<(String, Rect)>) {
        match shape {
            egui::Shape::Text(t) => {
                let rect = t.galley.rect.translate(t.pos.to_vec2());
                out.push((t.galley.text().to_owned(), rect));
            }
            egui::Shape::Vec(shapes) => shapes.into_iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }

    fn with_document() -> Bar {
        let mut bar = Bar::new();
        bar.app
            .add_document(Document::from_bytes(SRC.as_bytes().to_vec()));
        bar.settle();
        bar
    }

    /// WIN-14: the six menus and their entries, in order, also without a document, when
    /// entries are greyed out and none vanishes.
    #[test]
    fn the_six_menus_hold_their_entries_with_or_without_a_document() {
        let expected: [(&str, &[&str]); 6] = [
            (
                "File",
                &[
                    "New",
                    "Open…",
                    "Open Recent",
                    "Save",
                    "Save As…",
                    "Save as UTF-8",
                    "Close",
                    "Settings…",
                    "Quit",
                ],
            ),
            (
                "Edit",
                &[
                    "Undo",
                    "Redo",
                    "Cut",
                    "Copy",
                    "Paste",
                    "Select All",
                    "Find…",
                    "Find next",
                    "Find previous",
                    "Find in element…",
                    "Replace…",
                    "Element",
                ],
            ),
            (
                "View",
                &[
                    "Layout",
                    "Grid",
                    "Diagram",
                    "Cell pane beside the grid (experiment)",
                    "Attributes in the tree",
                    "Expand the tree on a click in the text",
                    "Collapse the tree",
                    "Word wrap",
                    "Toolbar",
                    "Font…",
                    "Theme",
                ],
            ),
            (
                "Navigate",
                &[
                    "Drill Up",
                    "Drill Down",
                    "Go to reference target",
                    "Go to line…",
                    "Next tab",
                    "Previous tab",
                    "Toggle Bookmark",
                    "Next Bookmark",
                    "Last Bookmark",
                    "Clear all Bookmarks",
                ],
            ),
            (
                "Tools",
                &[
                    "XPath…",
                    "Format",
                    "Compact",
                    "Format options…",
                    "References",
                ],
            ),
            (
                "Help",
                &["XML Atlas Help", "Check for updates now", "About XML Atlas"],
            ),
        ];
        // As macOS too, wherever the test runs: it names the keys differently.
        use egui::os::OperatingSystem;
        for (os, document) in [OperatingSystem::Nix, OperatingSystem::Mac]
            .into_iter()
            .flat_map(|os| [(os, false), (os, true)])
        {
            for (menu, entries) in &expected {
                let mut bar = if document {
                    with_document()
                } else {
                    Bar::new()
                };
                bar.ctx.set_os(os);
                bar.settle();
                bar.click(menu);
                // The checkboxes' keys sit in their own atoms and are left out too.
                assert_eq!(
                    bar.entries(),
                    *entries,
                    "{menu}, document: {document}, {os:?}"
                );
            }
        }
    }

    /// The submenus open from the pointer, and their commands act: Edit > Element on the
    /// selected element, References and Layout with what View held before.
    #[test]
    fn submenus_hold_their_entries_and_act() {
        let mut bar = with_document();
        let b = xml_atlas_engine::command::resolve(&bar.app.docs.active().unwrap().doc, "/r/b")
            .unwrap();
        bar.app.docs.active_mut().unwrap().select(b);
        bar.click("Edit");
        bar.point_at("Element");
        for entry in ["Rename", "Delete", "Duplicate", "Move up", "Move down"] {
            bar.at(entry);
        }
        bar.click("Move up");
        let source =
            |bar: &Bar| String::from_utf8(bar.app.docs.active().unwrap().doc.src.to_vec()).unwrap();
        let moved = source(&bar);
        assert!(
            moved.find("<b>").unwrap() < moved.find("<a>").unwrap(),
            "{moved}"
        );
        assert!(moved.starts_with("\u{feff}<r>\r\n"));
        assert_eq!(bar.app.docs.active().unwrap().keys, Pane::Tree);

        bar.click("Tools");
        bar.point_at("References");
        for entry in ["Resolve references", "Preview tree", "Rules files…"] {
            bar.at(entry);
        }
        bar.click("Rules files…");
        assert!(bar.app.settings_window.open && bar.app.settings_window.shows_references());

        bar.click("View");
        bar.point_at("Layout");
        for entry in ["Grid above text", "Text above grid", "Reset layout"] {
            bar.at(entry);
        }
        bar.click("Grid above text");
        assert_eq!(bar.app.settings.layout, Layout::GridAbove);
    }

    /// #167 with D37's submenus: the arrows enter a submenu and leave it again, and only
    /// outside it do Left and Right move along the bar.
    #[test]
    fn the_arrows_enter_and_leave_a_submenu() {
        use egui::Key;
        let mut bar = with_document();
        bar.click("Tools");
        // A pointer left on a name would reopen that menu.
        bar.frame(vec![Event::PointerGone]);
        for _ in 0..8 {
            if bar.focused() == Some("References") {
                break;
            }
            bar.press(Key::ArrowDown);
        }
        assert_eq!(bar.focused(), Some("References"));
        bar.press(Key::ArrowRight);
        assert!(bar.shows("Rules files…"), "{:?}", bar.names());
        assert!(!bar.shows("About XML Atlas"), "moved along the bar instead");
        bar.press(Key::ArrowLeft);
        assert_eq!(bar.focused(), Some("References"));
        assert!(
            !bar.shows("Rules files…") && bar.shows("Compact"),
            "{:?}",
            bar.names()
        );
        // From an entry that is no submenu: the next menu.
        bar.press(Key::ArrowUp);
        bar.press(Key::ArrowRight);
        assert!(bar.shows("About XML Atlas"), "{:?}", bar.names());
    }

    /// Edit > Element > Rename leaves the keyboard in the tree's name field, and Enter
    /// renames, also when the menu has a focus to give back after Alt: the field, drawn
    /// after the menu, asks last.
    #[test]
    fn rename_from_the_menu_keeps_its_field() {
        let mut bar = with_document();
        bar.tree = true;
        let doc = &bar.app.docs.active().unwrap().doc;
        let a = xml_atlas_engine::command::resolve(doc, "/r/a").unwrap();
        bar.app.docs.active_mut().unwrap().select(a);
        bar.settle();
        bar.click("Edit");
        bar.point_at("Element");
        // As if Alt had opened the menu while something else had the keyboard.
        bar.app
            .menu_keys
            .remember_focus(egui::Id::unique("elsewhere"));
        bar.click("Rename");
        bar.frame(vec![Event::Text("x".into())]);
        bar.press(egui::Key::Enter);
        let src = bar.app.docs.active().unwrap().doc.src.to_vec();
        let src = String::from_utf8(src).unwrap();
        assert!(src.contains("x>Zoë</") && !src.contains("<a>"), "{src}");
    }

    /// Every text drawn is whole and none lies over another (LANG-15).
    fn assert_nothing_cut(bar: &Bar, what: &str) {
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 900.0));
        for (i, (text, rect)) in bar.texts.iter().enumerate() {
            assert!(
                screen.contains_rect(*rect),
                "{what}: {text:?} at {rect:?} leaves the window"
            );
            for (other, their) in &bar.texts[i + 1..] {
                let shared = rect.intersect(*their);
                let overlap = shared.width() > 1.0 && shared.height() > 1.0;
                // An open submenu lies over its menu.
                let layered = what.contains('>');
                assert!(layered || !overlap, "{what}: {text:?} lies over {other:?}");
            }
        }
    }

    /// LANG-1, LANG-9, LANG-15: the menus in German, with German key names.
    #[test]
    fn the_menus_are_german_when_chosen() {
        let expected: [(&str, &[&str]); 6] = [
            ("Datei", &["Neu", "Öffnen…", "Speichern unter…", "Beenden"]),
            (
                "Bearbeiten",
                &["Rückgängig", "Alles auswählen", "Weitersuchen", "Element"],
            ),
            (
                "Ansicht",
                &["Anordnung", "Tabelle", "Zeilenumbruch", "Farbschema"],
            ),
            (
                "Navigieren",
                &[
                    "Ebene höher",
                    "Gehe zu Zeile…",
                    "Alle Lesezeichen entfernen",
                ],
            ),
            ("Extras", &["Formatieren", "Kompakt", "Verweise"]),
            ("Hilfe", &["Hilfe zu XML Atlas", "Über XML Atlas"]),
        ];
        for (menu, entries) in expected {
            let mut bar = with_document();
            bar.app.settings.language = LanguageChoice::German;
            bar.ctx.set_os(egui::os::OperatingSystem::Nix);
            bar.settle();
            bar.click(menu);
            for entry in entries {
                bar.at(entry);
            }
            for (text, _) in &bar.texts {
                assert!(!text.contains("Ctrl"), "{menu}: {text}");
            }
            assert_nothing_cut(&bar, menu);
            if menu == "Datei" {
                bar.at("Strg+Umschalt+S");
            }
        }
    }

    /// LANG-8: a new language applies at once and leaves the document, its changes and its
    /// undo history as they are.
    #[test]
    fn a_new_language_applies_at_once_and_changes_nothing_else() {
        let mut bar = with_document();
        let open = bar.app.docs.active_mut().unwrap();
        open.type_edit(
            Edit::new(xml_atlas_engine::StepName::Other("Type".into())).replace(3..3, " "),
        )
        .unwrap();
        let before = open.doc.src.to_vec();
        bar.settle();
        bar.at("File");
        bar.app.settings.language = LanguageChoice::German;
        bar.settle();
        bar.at("Datei");
        assert!(!bar.shows("File"));
        let open = bar.app.docs.active().unwrap();
        assert_eq!(open.doc.src.to_vec(), before);
        assert!(open.doc.is_modified());
        let step = open.doc.history.undo_label().map(ToString::to_string);
        assert_eq!(step.as_deref(), Some("Type"));
        // The step's own name is the engine's, still in English (D45, step 4).
        bar.click("Bearbeiten");
        bar.at("Rückgängig: Type");
    }

    /// LANG-1, LANG-9: the tree's and the grid's right-click menus in German.
    #[test]
    fn the_right_click_menus_are_german_when_chosen() {
        let open = |grid: bool| {
            let mut bar = with_document();
            bar.app.settings.language = LanguageChoice::German;
            bar.ctx.set_os(egui::os::OperatingSystem::Nix);
            (bar.tree, bar.grid) = (!grid, grid);
            bar.settle();
            bar
        };
        let mut tree = open(false);
        tree.right_click("a");
        for entry in [
            "Umbenennen",
            "Löschen",
            "Entf",
            "Strg+D",
            "Im Element suchen…",
            "Nach unten",
            "Alt+Umschalt+Pfeil runter",
            "Alles zuklappen",
        ] {
            tree.at(entry);
        }
        let mut grid = open(true);
        grid.right_click("#");
        for entry in [
            "Reihenfolge im Dokument",
            "Leere Spalten ausblenden",
            "Spalte einblenden",
        ] {
            grid.at(entry);
        }
    }

    /// A menu opens when the button goes down (#167), and letting go on its name leaves it
    /// there: once on screen it is in every frame, however long the button was held. egui
    /// takes the release as the click that closes it, and a menu closed for one frame is
    /// laid out unseen again: it flickered.
    #[test]
    fn a_click_on_a_menu_does_not_make_it_flicker() {
        for held in [0, 1, 2, 3, 8] {
            let mut bar = Bar::new();
            bar.settle();
            let pos = bar.at("File");
            let button = |pressed| Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            bar.frame(vec![Event::PointerMoved(pos)]);
            let mut frames = vec![vec![button(true)]];
            frames.extend(std::iter::repeat_with(Vec::new).take(held));
            frames.push(vec![button(false)]);
            frames.extend(std::iter::repeat_with(Vec::new).take(6));
            let shown: Vec<bool> = frames
                .into_iter()
                .map(|events| {
                    bar.frame(events);
                    bar.shows("Save As…")
                })
                .collect();
            let first = shown.iter().position(|&s| s).expect("the menu opens");
            assert!(
                shown[first..].iter().all(|&s| s),
                "held for {held} frames: {shown:?}"
            );
        }
    }
}
