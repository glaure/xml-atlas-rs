//! Window layout, file loading and selection state.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Key, KeyboardShortcut, Modifiers, RichText, ThemePreference, Ui};
use xml_atlas_engine::command::CommandError;
use xml_atlas_engine::xml::{NodeId, NodeKind, ROOT};
use xml_atlas_engine::{DiskChange, Document, Edit, EditError, FormatError, FormatOptions};

use crate::disk_watch::DiskWatch;
use crate::settings::{Layout, Settings, ThemeChoice};
use crate::tabs::Tabs;
use crate::{problems_panel, tree};
use xml_atlas_engine::grid::Grid;
use xml_atlas_engine::problems::{Problem, Severity};

pub struct App {
    /// The open documents, one tab each (OPEN-5).
    docs: Tabs<Open>,
    /// Gives each document its own [`Open::id`].
    next_id: u64,
    /// The window title last set.
    title: String,
    /// Settings remembered between sessions (WIN-3), and what was last written to disk.
    settings: Settings,
    saved: Settings,
    /// The desktop's light or dark preference as it arrives (WIN-2); `None` until known.
    desktop_theme: Option<egui::Theme>,
    desktop_theme_rx: Option<Receiver<egui::Theme>>,
    /// The theme last handed to egui, so it is set only when it changes.
    applied_theme: Option<ThemePreference>,
    /// An action waiting for the answer to "save changes?" about the tab shown (OPEN-11).
    confirm: Option<Action>,
    /// Set once the user agreed to close, so the close request is not asked about again.
    quitting: bool,
    find: crate::find_bar::FindBar,
    /// Files being opened, in the order asked for.
    loading: Vec<Loading>,
    message: Option<String>,
    /// A note about the last command, shown in the status bar until the next one.
    notice: Option<Note>,
    /// The Format options dialog, with the values being edited.
    format_dialog: Option<FormatOptions>,
    /// Wakes the window when the file of an open document changes on disk (OPEN-14).
    disk_watch: DiskWatch,
}

/// The answers of the "save changes?" dialog.
enum Answer {
    Save,
    Discard,
    Cancel,
}

/// Something that closes documents, and so may lose unsaved changes (OPEN-11).
#[derive(Clone, Debug)]
enum Action {
    /// Close the tab of the document with this [`Open::id`].
    Close(egui::Id),
    Quit,
}

struct Loading {
    path: PathBuf,
    rx: Receiver<std::io::Result<Document>>,
    /// The document with this [`Open::id`] is read again (OPEN-14), rather than a new tab.
    reload: Option<egui::Id>,
}

/// A document plus everything the views remember about it.
pub struct Open {
    pub doc: Document,
    /// Tells this document's views apart from another tab's, so that each keeps its own
    /// scroll positions.
    pub id: egui::Id,
    /// While quitting: Don't save was answered for this document.
    discard: bool,
    /// The file changed on disk and the notice about it waits for an answer (OPEN-14).
    pub disk: Option<DiskChange>,
    /// Element shown in the grid and selected in the tree.
    pub selected: Option<NodeId>,
    /// Node the grid and tree mark: a grid row, or the selection.
    pub focus: Option<NodeId>,
    /// Source bytes of what was clicked inside the selection (a grid row or cell, a tree text
    /// or attribute row). The text view draws it as a second highlight (TEXT-11).
    pub mark: Option<Range<u64>>,
    pub tree: tree::TreeState,
    pub grid: Option<Grid>,
    pub scroll_text_to: Option<usize>,
    /// Warnings and errors, in document order (OPEN-13).
    pub problems: Vec<Problem>,
    /// Where the arrow keys go: the tree or the grid, whichever was clicked last.
    pub keys: Pane,
    /// Grid row to scroll into view on the next frame.
    pub grid_scroll_to: Option<usize>,
    /// The marked grid cell of the marked row: an index into the grid's columns. `None`
    /// marks the whole row.
    pub grid_cell: Option<usize>,
    /// The grid cell being edited (GRID-10).
    pub grid_edit: Option<crate::grid::CellEditor>,
    /// What the last edit command did, or why it could not; the app shows it.
    pub note: Option<Note>,
}

/// How a structured edit moves elements among their siblings, so that the selection and
/// the expanded tree rows follow them. Positions are as [`xml_atlas_engine::xml::Index::element_position`]
/// gives them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shift {
    /// No element moves (rename, set text or attribute).
    None,
    /// Two siblings swap places (move up or down).
    Swap(Vec<u32>, Vec<u32>),
    /// A new element at this position (duplicate); it becomes the selection.
    Insert(Vec<u32>),
    /// The element at this position is gone (delete); the next one takes its place.
    Remove(Vec<u32>),
}

impl Shift {
    /// Where the element at `p` is after the edit; `None` when it is gone.
    pub fn remap(&self, p: &[u32]) -> Option<Vec<u32>> {
        let mut out = p.to_vec();
        match self {
            Shift::None => {}
            Shift::Swap(a, b) => {
                if p.starts_with(a) {
                    out[a.len() - 1] = b[b.len() - 1];
                } else if p.starts_with(b) {
                    out[b.len() - 1] = a[a.len() - 1];
                }
            }
            Shift::Insert(at) => {
                if let Some(k) = sibling_level(at, p)
                    && p[k] >= at[k]
                {
                    out[k] += 1;
                }
            }
            Shift::Remove(at) => {
                if p.starts_with(at) {
                    return None;
                }
                if let Some(k) = sibling_level(at, p)
                    && p[k] > at[k]
                {
                    out[k] -= 1;
                }
            }
        }
        Some(out)
    }
}

/// The level at which `p` is at or under a sibling of `at`: they share `at`'s parent.
fn sibling_level(at: &[u32], p: &[u32]) -> Option<usize> {
    let k = at.len().checked_sub(1)?;
    (p.len() > k && p[..k] == at[..k]).then_some(k)
}

/// A line for the status bar about the last command.
#[derive(Clone, Debug)]
pub struct Note {
    pub text: String,
    /// The command was refused: drawn as a warning.
    pub refused: bool,
}

impl Note {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            refused: false,
        }
    }

    pub fn refused(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            refused: true,
        }
    }
}

/// A view that takes arrow keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pane {
    #[default]
    Tree,
    Grid,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Vec<PathBuf>) -> Self {
        let settings = Settings::load();
        let mut find = crate::find_bar::FindBar::default();
        find.set_options(settings.find);
        let mut app = Self {
            docs: Tabs::default(),
            next_id: 0,
            title: String::new(),
            settings: settings.clone(),
            saved: settings,
            desktop_theme: None,
            desktop_theme_rx: crate::system_theme::watch(cc.egui_ctx.clone()),
            applied_theme: None,
            confirm: None,
            quitting: false,
            find,
            loading: Vec::new(),
            message: None,
            notice: None,
            format_dialog: None,
            disk_watch: DiskWatch::start(cc.egui_ctx.clone()),
        };
        for path in paths {
            app.open_file(&cc.egui_ctx, path);
        }
        app
    }

    /// Opens `path` in a new tab, or shows its tab when it is open already (OPEN-5).
    fn open_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        let same = |p: &Path| same_file(p, &path);
        if let Some(i) = self
            .docs
            .position(|o| o.doc.path.as_deref().is_some_and(same))
        {
            self.docs.select(i);
            self.settings.add_recent(&path);
            self.message = None;
            self.notice = Some(Note::info(format!("{} is open already", file_name(&path))));
            return;
        }
        if self.loading.iter().any(|l| same(&l.path)) {
            return;
        }
        self.start_loading(ctx, path, None);
    }

    fn start_loading(&mut self, ctx: &egui::Context, path: PathBuf, reload: Option<egui::Id>) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        let p = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Document::open(&p));
            ctx.request_repaint();
        });
        self.loading.push(Loading { path, rx, reload });
        self.message = None;
        self.notice = None;
    }

    /// Adds the files that finished loading in tabs, in the order they finished (OPEN-2,
    /// OPEN-5).
    fn poll_loading(&mut self) {
        let mut i = 0;
        while i < self.loading.len() {
            let result = match self.loading[i].rx.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => {
                    i += 1;
                    continue;
                }
                Err(TryRecvError::Disconnected) => Err(std::io::Error::other("loading stopped")),
            };
            let loading = self.loading.remove(i);
            match loading.reload {
                Some(id) => self.reloaded(id, &loading.path, result),
                None => self.loaded(loading.path, result),
            }
        }
    }

    /// Notes which documents' files changed on disk since they were read or saved (OPEN-14):
    /// when the watch saw a change, and when a tab came or went. A file being read again is
    /// not looked at.
    fn check_disk(&mut self) {
        let paths = self
            .docs
            .iter()
            .filter_map(|o| o.doc.path.clone())
            .collect();
        let new_list = self.disk_watch.watch(paths);
        if !(self.disk_watch.take_changed() || new_list) {
            return;
        }
        for open in self.docs.iter_mut() {
            if !self.loading.iter().any(|l| l.reload == Some(open.id)) {
                open.disk = open.doc.disk_change();
            }
        }
    }

    /// Reads the file of the tab shown again, in the background (OPEN-14).
    fn reload(&mut self, ctx: &egui::Context) {
        let Some(open) = self.docs.active_mut() else {
            return;
        };
        let Some(path) = open.doc.path.clone() else {
            return;
        };
        open.disk = None;
        let id = open.id;
        self.start_loading(ctx, path, Some(id));
    }

    /// The document with `id` takes the contents its file has now. The selection and the
    /// expanded rows stay where they were, as far as the elements are still there.
    fn reloaded(&mut self, id: egui::Id, path: &Path, result: std::io::Result<Document>) {
        let Some(i) = self.docs.position(|o| o.id == id) else {
            return;
        };
        match result {
            Ok(fresh) => {
                if let Some(open) = self.docs.get_mut(i) {
                    let _ = open.edit(|doc| {
                        doc.reload_from(fresh);
                        Ok(true)
                    });
                    // It may have changed again while it was being read.
                    open.disk = open.doc.disk_change();
                }
                self.notice = Some(Note::info(format!("Reloaded {}", file_name(path))));
            }
            Err(e) => {
                self.message = Some(format!("Could not reload {}: {e}", path.display()));
            }
        }
    }

    fn loaded(&mut self, path: PathBuf, result: std::io::Result<Document>) {
        match result {
            Ok(doc) => {
                self.add_document(doc);
                self.settings.add_recent(&path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // A file that is gone leaves the recent list (OPEN-12).
                let listed = self.settings.recent.len();
                self.settings.recent.retain(|p| p != &path);
                let note = if self.settings.recent.len() < listed {
                    "; removed from recent files"
                } else {
                    ""
                };
                self.message = Some(format!("Could not open {}: {e}{note}", path.display()));
            }
            Err(e) => self.message = Some(format!("Could not open {}: {e}", path.display())),
        }
    }

    /// Shows `doc` in a new tab.
    fn add_document(&mut self, doc: Document) {
        self.next_id += 1;
        let mut open = Open {
            tree: tree::TreeState::new(&doc),
            doc,
            id: egui::Id::unique(("document", self.next_id)),
            discard: false,
            disk: None,
            selected: None,
            focus: None,
            mark: None,
            grid: None,
            scroll_text_to: None,
            problems: Vec::new(),
            keys: Pane::Tree,
            grid_scroll_to: None,
            grid_cell: None,
            grid_edit: None,
            note: None,
        };
        open.problems = open.doc.problems();
        open.tree.set_attributes(self.settings.tree_attributes);
        if let Some(root) = open.doc.index.root_element() {
            open.select(root);
        }
        self.docs.push(open);
    }

    /// Light, dark, or the desktop's preference (WIN-2). On Linux the desktop's preference comes
    /// from the settings portal; elsewhere egui follows the system by itself.
    fn apply_theme(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.desktop_theme_rx {
            while let Ok(theme) = rx.try_recv() {
                self.desktop_theme = Some(theme);
            }
        }
        let wanted = match (self.settings.theme, self.desktop_theme) {
            (ThemeChoice::Light, _) => ThemePreference::Light,
            (ThemeChoice::Dark, _) => ThemePreference::Dark,
            (ThemeChoice::System, Some(theme)) => theme.into(),
            (ThemeChoice::System, None) => ThemePreference::System,
        };
        if self.applied_theme != Some(wanted) {
            ctx.set_theme(wanted);
            self.applied_theme = Some(wanted);
        }
    }

    /// Writes the settings when they changed since they were last written (WIN-3).
    fn save_settings(&mut self) {
        self.settings.find = self.find.options();
        if self.settings == self.saved {
            return;
        }
        match self.settings.save() {
            Ok(()) => self.saved = self.settings.clone(),
            Err(e) => {
                log::warn!("cannot save settings: {e}");
                // Do not try again every frame.
                self.saved = self.settings.clone();
            }
        }
    }

    /// Runs `action`, or first shows the document it would lose changes in and asks whether
    /// to save them (OPEN-11). Quitting asks about each modified document in turn.
    fn request(&mut self, ctx: &egui::Context, action: Action) {
        let ask = match &action {
            Action::Close(id) => self
                .docs
                .position(|o| o.id == *id)
                .filter(|&i| self.docs.get(i).is_some_and(|o| o.doc.is_modified())),
            Action::Quit => self.docs.position(|o| o.doc.is_modified() && !o.discard),
        };
        match ask {
            Some(i) => {
                self.docs.select(i);
                self.confirm = Some(action);
            }
            None => self.perform(ctx, action),
        }
    }

    fn perform(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Close(id) => {
                if let Some(i) = self.docs.position(|o| o.id == id) {
                    self.docs.close(i);
                    self.notice = None;
                }
            }
            Action::Quit => {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// The "save changes?" dialog about the tab shown. Save goes on only if saving worked.
    fn confirm_dialog(&mut self, ui: &mut Ui) {
        let Some(action) = self.confirm.clone() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let name = self
            .docs
            .active()
            .and_then(|o| o.doc.path.as_deref())
            .map(file_name)
            .unwrap_or_else(|| "the document".into());
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("save-changes")).show(&ctx, |ui| {
            ui.set_width(360.0);
            ui.heading("Unsaved changes");
            ui.label(format!("Save the changes to {name}?"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    answer = Some(Answer::Save);
                }
                if ui.button("Don't save").clicked() {
                    answer = Some(Answer::Discard);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(Answer::Cancel);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(Answer::Cancel);
        }
        let Some(answer) = answer else { return };
        self.confirm = None;
        match (answer, action) {
            (Answer::Save, action) => {
                if self.save() {
                    self.request(&ctx, action);
                } else {
                    self.cancel_quit();
                }
            }
            (Answer::Discard, Action::Close(id)) => self.perform(&ctx, Action::Close(id)),
            (Answer::Discard, Action::Quit) => {
                if let Some(open) = self.docs.active_mut() {
                    open.discard = true;
                }
                self.request(&ctx, Action::Quit);
            }
            (Answer::Cancel, _) => self.cancel_quit(),
        }
    }

    /// Quitting stopped: a later quit asks about every modified document again.
    fn cancel_quit(&mut self) {
        for open in self.docs.iter_mut() {
            open.discard = false;
        }
    }

    /// Closes the tab shown, asking first if it has unsaved changes.
    fn close_tab(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.docs.active().map(|o| o.id) {
            self.request(ctx, Action::Close(id));
        }
    }

    /// The tabs, one per document (OPEN-5): click shows one, × or a middle click closes it.
    fn tab_bar(&mut self, ui: &mut Ui) {
        let mut show = None;
        let mut close = None;
        egui::ScrollArea::horizontal()
            .id_salt("tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, open) in self.docs.iter().enumerate() {
                        let path = open.doc.path.as_deref();
                        let mut name = path.map(file_name).unwrap_or_else(|| "untitled".into());
                        if open.doc.is_modified() {
                            name.push_str(" *");
                        }
                        let active = i == self.docs.active_index();
                        let mut hover = path.map(|p| p.display().to_string()).unwrap_or_default();
                        let name = match open.disk {
                            Some(change) => {
                                hover.push_str(&format!("\n{}", disk_text(change)));
                                RichText::new(name).color(ui.visuals().warn_fg_color)
                            }
                            None => RichText::new(name),
                        };
                        let tab = ui.selectable_label(active, name).on_hover_text(hover);
                        if tab.clicked() {
                            show = Some(i);
                        }
                        if tab.middle_clicked() {
                            close = Some(open.id);
                        }
                        let x = ui
                            .add(egui::Button::new(RichText::new("×").weak()).frame(false))
                            .on_hover_text("Close    Ctrl+W");
                        if x.clicked() {
                            close = Some(open.id);
                        }
                        ui.separator();
                    }
                });
            });
        if let Some(i) = show
            && i != self.docs.active_index()
        {
            self.docs.select(i);
            self.notice = None;
        }
        if let Some(id) = close {
            self.request(ui.ctx(), Action::Close(id));
        }
    }

    /// Ctrl+W closes the tab shown; Ctrl+Tab and Ctrl+Page Down show the next tab,
    /// Ctrl+Shift+Tab and Ctrl+Page Up the one before (OPEN-5).
    fn tab_shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |modifiers, key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)))
        };
        let back = Modifiers::COMMAND | Modifiers::SHIFT;
        if pressed(Modifiers::COMMAND, Key::W) {
            self.close_tab(ctx);
        } else if pressed(back, Key::Tab) || pressed(Modifiers::COMMAND, Key::PageUp) {
            self.docs.cycle(true);
            self.notice = None;
        } else if pressed(Modifiers::COMMAND, Key::Tab)
            || pressed(Modifiers::COMMAND, Key::PageDown)
        {
            self.docs.cycle(false);
            self.notice = None;
        }
    }

    /// The window title: the file of the tab shown.
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.docs.active().and_then(|o| o.doc.path.as_deref()) {
            Some(p) => format!("{} — XML Atlas", file_name(p)),
            None => "XML Atlas".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn pick_file(&mut self, ctx: &egui::Context) {
        let dialog = rfd::FileDialog::new()
            .add_filter(
                "XML",
                &[
                    "xml", "xsd", "xsl", "xslt", "svg", "config", "csproj", "plist",
                ],
            )
            .add_filter("All files", &["*"]);
        if let Some(paths) = dialog.pick_files() {
            for path in paths {
                self.open_file(ctx, path);
            }
        }
    }

    fn format_document(&mut self) {
        let Some(open) = self.docs.active() else {
            return;
        };
        let chosen = open.doc.format_options(self.settings.format);
        self.replace_with(
            "Format",
            open.doc.format(&chosen.options),
            |options| {
                let source = if chosen.from_document.is_empty() {
                    String::new()
                } else {
                    format!(
                        " (from the file's settings comment: {})",
                        chosen.from_document.join(", ")
                    )
                };
                format!("Formatted: {options}{source}")
            },
            chosen.options,
        );
    }

    /// Edit > Compact (FMT-14): the document on one line.
    fn compact_document(&mut self) {
        let Some(open) = self.docs.active() else {
            return;
        };
        let options = self.settings.format;
        let result = open.doc.compact(&options);
        self.replace_with(
            "Compact",
            result,
            |o| {
                let mut s = "Compacted".to_string();
                if o.remove_comments {
                    s.push_str(", comments removed");
                }
                if o.trim_text {
                    s.push_str(", text trimmed");
                }
                s
            },
            options,
        );
    }

    /// Replaces the document with the result of Format or Compact, as one undo step called
    /// `label` (FMT-9).
    fn replace_with(
        &mut self,
        label: &str,
        result: Result<Vec<u8>, FormatError>,
        notice: impl FnOnce(FormatOptions) -> String,
        options: FormatOptions,
    ) {
        let Some(open) = self.docs.active_mut() else {
            return;
        };
        match result {
            Ok(out) => {
                let edit = Edit::whole(label, open.doc.src.len(), out);
                match open.edit(|doc| doc.apply(edit).map(|()| true)) {
                    Ok(_) => self.notice = Some(Note::info(notice(options))),
                    // Already formatted: the edit changes nothing.
                    Err(EditError::Empty) => self.notice = Some(Note::info(notice(options))),
                    Err(e) => self.message = Some(format!("Cannot {label}: {e}")),
                }
            }
            Err(e) => self.message = Some(format!("Cannot format: {e}")),
        }
    }

    /// Edit > Undo.
    fn undo(&mut self) {
        let Some(open) = self.docs.active_mut() else {
            return;
        };
        let mut label = None;
        if let Ok(true) = open.edit(|doc| {
            label = doc.undo();
            Ok(label.is_some())
        }) {
            self.notice = label.map(|l| Note::info(format!("Undid {l}")));
        }
    }

    /// Edit > Redo.
    fn redo(&mut self) {
        let Some(open) = self.docs.active_mut() else {
            return;
        };
        let mut label = None;
        if let Ok(true) = open.edit(|doc| {
            label = doc.redo();
            Ok(label.is_some())
        }) {
            self.notice = label.map(|l| Note::info(format!("Redid {l}")));
        }
    }

    /// Edit > Format options: indent and attribute wrapping for this session.
    fn format_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut edit) = self.format_dialog else {
            return;
        };
        let overrides = self
            .docs
            .active()
            .map(|o| o.doc.format_options(edit).from_document)
            .unwrap_or_default();
        let mut close = None;
        let modal = egui::Modal::new(egui::Id::unique("format-options")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Format options");
            ui.add_space(6.0);
            egui::Grid::new("format-options-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Indent");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut edit.tabs, false, "Spaces:");
                        ui.add_enabled(
                            !edit.tabs,
                            egui::DragValue::new(&mut edit.indent_size).range(0..=16),
                        );
                        ui.radio_value(&mut edit.tabs, true, "Tabs");
                    });
                    ui.end_row();
                    ui.label("Attributes in a tag line");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut edit.max_attributes).range(0..=99));
                        ui.label(RichText::new("at most; 0: never wrap").weak());
                    });
                    ui.end_row();
                    ui.label("Also");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut edit.remove_comments, "Remove comments");
                        ui.checkbox(&mut edit.trim_text, "Trim text");
                        ui.label(
                            RichText::new(
                                "These change the content. The settings comment, and comments \
                                 and text inside mixed content, are kept.",
                            )
                            .weak(),
                        );
                    });
                    ui.end_row();
                });
            if !overrides.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "This file's settings comment overrides: {}.",
                        overrides.join(", ")
                    ))
                    .color(ui.visuals().warn_fg_color),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    close = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    close = Some(false);
                }
                if ui.button("Defaults").clicked() {
                    edit = FormatOptions::default();
                }
            });
        });
        if modal.should_close() && close.is_none() {
            close = Some(false);
        }
        match close {
            Some(true) => {
                self.settings.format = edit;
                self.format_dialog = None;
            }
            Some(false) => self.format_dialog = None,
            None => self.format_dialog = Some(edit),
        }
    }

    /// Saves the document. False if there is none, the user cancelled, or writing failed.
    fn save(&mut self) -> bool {
        let Some(open) = self.docs.active_mut() else {
            return false;
        };
        let path = match open.doc.path.clone() {
            Some(p) => p,
            None => match rfd::FileDialog::new().save_file() {
                Some(p) => p,
                None => return false,
            },
        };
        self.save_to(path)
    }

    /// File > Save As: the document under a name chosen now, which it keeps.
    fn save_as(&mut self) -> bool {
        let Some(open) = self.docs.active() else {
            return false;
        };
        let mut dialog = rfd::FileDialog::new();
        if let Some(p) = open.doc.path.as_deref() {
            if let Some(dir) = p.parent() {
                dialog = dialog.set_directory(dir);
            }
            if let Some(name) = p.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
        }
        match dialog.save_file() {
            Some(path) => self.save_to(path),
            None => false,
        }
    }

    /// Writes the tab shown to `path`. False if writing failed.
    fn save_to(&mut self, path: PathBuf) -> bool {
        let Some(open) = self.docs.active_mut() else {
            return false;
        };
        match open.doc.save(&path) {
            Ok(()) => {
                open.disk = None;
                self.settings.add_recent(&path);
                self.message = None;
                true
            }
            Err(e) => {
                self.message = Some(format!("Could not save {}: {e}", path.display()));
                false
            }
        }
    }

    /// The notice above the views when the file of the tab shown changed on disk (OPEN-14):
    /// read it again, write the open document over it or under another name, or keep the
    /// open document without saving.
    fn disk_bar(&mut self, ui: &mut Ui) {
        let Some(open) = self.docs.active() else {
            return;
        };
        let Some(change) = open.disk else {
            return;
        };
        let modified = open.doc.is_modified();
        let name = open.doc.path.as_deref().map(file_name).unwrap_or_default();
        let ctx = ui.ctx().clone();
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("⚠ {name} {}.", disk_text(change)),
            );
            if change == DiskChange::Changed {
                let (label, tip) = if modified {
                    (
                        "Reload, discarding changes",
                        "Show the file as it is now; your unsaved changes are lost",
                    )
                } else {
                    ("Reload", "Show the file as it is now")
                };
                if ui.button(label).on_hover_text(tip).clicked() {
                    self.reload(&ctx);
                }
            }
            if ui
                .button("Save")
                .on_hover_text("Write the document as it is open here to the file")
                .clicked()
            {
                self.save();
            }
            if ui.button("Save as…").clicked() {
                self.save_as();
            }
            if ui
                .button("Keep")
                .on_hover_text("Keep the document as it is open here, without saving")
                .clicked()
                && let Some(open) = self.docs.active_mut()
            {
                open.doc.keep_over_disk();
                open.disk = None;
            }
        });
    }

    /// File > Open Recent (OPEN-12): newest first, the file name with its folder beside it.
    fn recent_menu(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let enabled = !self.settings.recent.is_empty();
        let mut chosen = None;
        let mut clear = false;
        ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button("Open Recent", |ui| {
                for (i, path) in self.settings.recent.iter().enumerate() {
                    let folder = path
                        .parent()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default();
                    let name = RichText::new(format!("{}  {}", i + 1, file_name(path)));
                    let folder = RichText::new(format!("   {}", shorten_left(&folder, 48))).weak();
                    let button = ui
                        .add(
                            egui::Button::new((name, folder)).wrap_mode(egui::TextWrapMode::Extend),
                        )
                        .on_hover_text(path.display().to_string());
                    if button.clicked() {
                        chosen = Some(path.clone());
                    }
                }
                ui.separator();
                if ui.button("Clear list").clicked() {
                    clear = true;
                }
            });
        });
        if let Some(path) = chosen {
            ui.close();
            self.open_file(ctx, path);
        }
        if clear {
            ui.close();
            self.settings.recent.clear();
        }
    }

    fn menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open…    Ctrl+O").clicked() {
                    ui.close();
                    self.pick_file(&ctx);
                }
                self.recent_menu(ui, &ctx);
                let can_save = self.docs.active().is_some_and(|o| o.doc.is_modified());
                if ui
                    .add_enabled(can_save, egui::Button::new("Save     Ctrl+S"))
                    .clicked()
                {
                    ui.close();
                    self.save();
                }
                if ui
                    .add_enabled(
                        !self.docs.is_empty(),
                        egui::Button::new("Save As…    Ctrl+Shift+S"),
                    )
                    .clicked()
                {
                    ui.close();
                    self.save_as();
                }
                if ui
                    .add_enabled(!self.docs.is_empty(), egui::Button::new("Close    Ctrl+W"))
                    .clicked()
                {
                    ui.close();
                    self.close_tab(&ctx);
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.close();
                    self.request(&ctx, Action::Quit);
                }
            });
            ui.menu_button("Edit", |ui| {
                let history = self.docs.active().map(|o| &o.doc.history);
                let undo = history.and_then(|h| h.undo_label()).map(str::to_owned);
                let redo = history.and_then(|h| h.redo_label()).map(str::to_owned);
                let text = |what: &str, label: &Option<String>, keys: &str| match label {
                    Some(l) => format!("{what} {l}    {keys}"),
                    None => format!("{what}    {keys}"),
                };
                if ui
                    .add_enabled(
                        undo.is_some(),
                        egui::Button::new(text("Undo", &undo, "Ctrl+Z")),
                    )
                    .clicked()
                {
                    ui.close();
                    self.undo();
                }
                if ui
                    .add_enabled(
                        redo.is_some(),
                        egui::Button::new(text("Redo", &redo, "Ctrl+Y")),
                    )
                    .clicked()
                {
                    ui.close();
                    self.redo();
                }
                ui.separator();
                if ui
                    .add_enabled(!self.docs.is_empty(), egui::Button::new("Find…    Ctrl+F"))
                    .clicked()
                {
                    ui.close();
                    self.find.open();
                }
                if ui
                    .add_enabled(
                        !self.docs.is_empty(),
                        egui::Button::new("Replace…    Ctrl+H"),
                    )
                    .clicked()
                {
                    ui.close();
                    self.find.open_replace();
                }
                if ui
                    .add_enabled(!self.docs.is_empty(), egui::Button::new("Format"))
                    .clicked()
                {
                    ui.close();
                    self.format_document();
                }
                if ui
                    .add_enabled(!self.docs.is_empty(), egui::Button::new("Compact"))
                    .clicked()
                {
                    ui.close();
                    self.compact_document();
                }
                if ui.button("Format options…").clicked() {
                    ui.close();
                    self.format_dialog = Some(self.settings.format);
                }
            });
            ui.menu_button("View", |ui| {
                ui.add_enabled_ui(!self.docs.is_empty(), |ui| {
                    let mut show = self.docs.active().is_none_or(|o| o.tree.options.attributes);
                    if ui.checkbox(&mut show, "Attributes in the tree").changed() {
                        // A view setting: for every tab.
                        for open in self.docs.iter_mut() {
                            open.tree.set_attributes(show);
                        }
                        self.settings.tree_attributes = show;
                    }
                });
                ui.separator();
                ui.label("Theme");
                ui.radio_value(
                    &mut self.settings.theme,
                    ThemeChoice::System,
                    "Follow system",
                );
                ui.radio_value(&mut self.settings.theme, ThemeChoice::Light, "Light");
                ui.radio_value(&mut self.settings.theme, ThemeChoice::Dark, "Dark");
                ui.separator();
                ui.label("Layout");
                ui.radio_value(
                    &mut self.settings.layout,
                    Layout::GridAbove,
                    "Grid above text",
                );
                ui.radio_value(
                    &mut self.settings.layout,
                    Layout::TextAbove,
                    "Text above grid (as in XML Marker)",
                );
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if let Some(l) = self.loading.first() {
                ui.spinner();
                let doing = if l.reload.is_some() {
                    "Reloading"
                } else {
                    "Opening"
                };
                ui.label(format!("{doing} {}…", l.path.display()));
                return;
            }
            if let Some(m) = &self.message {
                ui.colored_label(ui.visuals().error_fg_color, m);
                return;
            }
            let Some(open) = self.docs.active_mut() else {
                ui.label("Open a file with Ctrl+O or drop it on the window.");
                return;
            };
            let s = open.doc.stats();
            let name = open.doc.path.as_deref().map(file_name).unwrap_or_default();
            ui.label(format!(
                "{name}{}  ·  {:.1} MiB  ·  {} lines  ·  {} elements  ·  opened in {:.0} ms",
                if open.doc.is_modified() {
                    " (modified)"
                } else {
                    ""
                },
                s.bytes as f64 / 1048576.0,
                s.lines,
                s.elements,
                s.timings.total().as_secs_f64() * 1000.0,
            ));
            if let Some(n) = &self.notice {
                let text = RichText::new(format!("  ·  {}", n.text));
                ui.label(if n.refused {
                    text.color(ui.visuals().warn_fg_color)
                } else {
                    text.weak()
                });
            }
            if let Some(i) = open
                .problems
                .iter()
                .position(|p| p.severity == Severity::Error)
            {
                let p = &open.problems[i];
                let text = format!("  {} {}", problems_panel::symbol(p.severity), p.message);
                let text =
                    RichText::new(text).color(problems_panel::color(ui.visuals(), p.severity));
                if ui.link(text).on_hover_text("Go to the error").clicked() {
                    problems_panel::go_to(open, i);
                }
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_loading();
        self.check_disk();
        self.apply_theme(&ctx);
        self.save_settings();

        // Every file dropped opens, each in its own tab.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        for path in dropped {
            self.open_file(&ctx, path);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O)))
        {
            self.pick_file(&ctx);
        }
        // The window's close button: hold the close until the user has answered.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quitting
            && self.docs.iter().any(|o| o.doc.is_modified())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.confirm.is_none() {
                self.request(&ctx, Action::Quit);
            }
        }
        if self.confirm.is_none() {
            self.tab_shortcuts(&ctx);
        }
        self.confirm_dialog(ui);
        self.format_dialog(&ctx);
        // Ctrl+Shift+S first: Ctrl+S would match it too.
        if ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::S,
            ))
        }) {
            self.save_as();
        } else if ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S)))
        {
            self.save();
        }
        // A focused text field (Find) has its own undo.
        if !ctx.text_edit_focused() && self.confirm.is_none() {
            let redo = ctx.input_mut(|i| {
                i.consume_shortcut(&KeyboardShortcut::new(
                    Modifiers::COMMAND | Modifiers::SHIFT,
                    Key::Z,
                )) || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y))
            });
            if redo {
                self.redo();
            } else if ctx.input_mut(|i| {
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z))
            }) {
                self.undo();
            }
        }

        self.find.shortcuts(&ctx, self.docs.active_mut());
        if self.confirm.is_none()
            && let Some(open) = self.docs.active_mut()
        {
            crate::nav::handle(&ctx, open);
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        if !self.docs.is_empty() {
            egui::Panel::top("tabs").show(ui, |ui| self.tab_bar(ui));
        }
        if self.docs.active().is_some_and(|o| o.disk.is_some()) {
            egui::Panel::top("disk").show(ui, |ui| self.disk_bar(ui));
        }
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        self.update_title(&ctx);

        let Some(open) = self.docs.active_mut() else {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.heading("Drop an XML file here, or press Ctrl+O");
                });
            });
            return;
        };

        egui::Panel::left("tree")
            .resizable(true)
            .default_size(320.0)
            .show(ui, |ui| {
                let id = open.id;
                ui.push_id(id, |ui| tree::show(ui, open));
            });
        let layout = self.settings.layout;
        egui::CentralPanel::default().show(ui, |ui| {
            let total = ui.available_height();
            // Each layout has its own panel, so each remembers its own height.
            let grid = match layout {
                Layout::GridAbove => egui::Panel::top("grid"),
                Layout::TextAbove => egui::Panel::bottom("grid-below"),
            };
            grid.resizable(true)
                .default_size(total * 0.45)
                .show(ui, |ui| {
                    ui.push_id(open.id, |ui| crate::grid::show(ui, open));
                });
            if !open.problems.is_empty() {
                egui::Panel::bottom("problems")
                    .resizable(true)
                    .show(ui, |ui| {
                        ui.push_id(open.id, |ui| problems_panel::show(ui, open));
                    });
            }
            if self.find.visible {
                egui::Panel::top("find").show(ui, |ui| self.find.show(ui, open));
            }
            egui::CentralPanel::default().show(ui, |ui| {
                ui.push_id(open.id, |ui| crate::text_view::show(ui, open));
            });
        });
        if let Some(note) = open.note.take() {
            self.notice = Some(note);
        }
    }
}

impl Open {
    /// Runs `change` on the document and, when it reports a change, rebuilds the views around
    /// the same elements: the selection and the expanded tree rows are found again by their
    /// position among their element siblings (FMT-8).
    pub fn edit(
        &mut self,
        change: impl FnOnce(&mut Document) -> Result<bool, EditError>,
    ) -> Result<bool, EditError> {
        self.edit_with(change, &Shift::None)
    }

    /// Runs a structured edit (EDIT-5 to EDIT-10) as one undo step and notes what it did.
    /// `what` names the command in a refusal; `shift` says how it moves elements, so that
    /// the selection and the expanded rows follow.
    pub fn command(
        &mut self,
        what: &str,
        make: impl FnOnce(&Document) -> Result<Option<Edit>, CommandError>,
        shift: Shift,
    ) {
        if let Some(e) = &self.doc.index.error {
            self.note = Some(Note::refused(format!(
                "Cannot {what}: the document is not well-formed ({e})"
            )));
            return;
        }
        let edit = match make(&self.doc) {
            Ok(Some(edit)) => edit,
            Ok(None) => return,
            Err(e) => {
                self.note = Some(Note::refused(format!("Cannot {what}: {e}")));
                return;
            }
        };
        let label = edit.label().to_string();
        self.note = Some(
            match self.edit_with(|doc| doc.apply(edit).map(|()| true), &shift) {
                Ok(_) => Note::info(label),
                Err(EditError::Empty) => {
                    // Two equal siblings swapped: the bytes stay, but the selection moves as
                    // asked, so that pressing the key again goes on moving.
                    if let Shift::Swap(_, to) = &shift
                        && let Some(id) = self.doc.index.element_at_position(to)
                    {
                        self.select(id);
                    }
                    Note::info(format!("{label}: nothing to change"))
                }
                Err(e) => Note::refused(format!("Cannot {what}: {e}")),
            },
        );
    }

    /// [`Open::edit`], with positions moved by `shift`. When nothing is at the selection's
    /// position any more (it was deleted), the selection goes to the element after it, else
    /// the one before, else its parent. A marked grid row and cell stay marked.
    fn edit_with(
        &mut self,
        change: impl FnOnce(&mut Document) -> Result<bool, EditError>,
        shift: &Shift,
    ) -> Result<bool, EditError> {
        let idx = &self.doc.index;
        let selected = self.selected.map(|id| {
            let p = idx.element_position(id);
            match shift {
                Shift::Insert(at) => at.clone(),
                _ => shift.remap(&p).unwrap_or(p),
            }
        });
        let focus = self
            .focus
            .filter(|&f| Some(f) != self.selected)
            .map(|id| idx.element_position(id))
            .filter(|p| !p.is_empty())
            .and_then(|p| shift.remap(&p));
        let column = self
            .grid
            .as_ref()
            .zip(self.grid_cell)
            .and_then(|(g, c)| g.columns.get(c).cloned());
        // The sort and hidden columns stay when the grid shows the same element again.
        let view = self
            .grid
            .as_ref()
            .map(|g| (g.path.clone(), g.view().clone()));
        let expanded: Vec<Vec<u32>> = self
            .tree
            .expanded_ids()
            .filter_map(|id| shift.remap(&idx.element_position(id)))
            .collect();
        if !change(&mut self.doc)? {
            return Ok(false);
        }
        let attributes = self.tree.options.attributes;
        self.tree = tree::TreeState::new(&self.doc);
        self.tree.set_attributes(attributes);
        let idx = &self.doc.index;
        let found: Vec<NodeId> = expanded
            .iter()
            .filter_map(|p| idx.element_at_position(p))
            .collect();
        for id in found {
            self.tree.expand(id);
        }
        self.problems = self.doc.problems();
        self.focus = None;
        self.mark = None;
        let selected = selected
            .and_then(|p| nearest_element(&self.doc, p))
            .or_else(|| self.doc.index.root_element());
        match selected {
            Some(id) => self.select(id),
            None => {
                self.selected = None;
                self.grid = None;
            }
        }
        if let (Some(grid), Some((path, view))) = (&mut self.grid, view)
            && grid.path == path
        {
            grid.set_view(&self.doc, view);
        }
        let focus = match focus {
            Some(p) => self.doc.index.element_at_position(&p),
            // The grid shows the selection itself as its one row (GRID-5).
            None => self.selected.filter(|_| column.is_some()),
        };
        if let Some(id) = focus {
            self.focus(id);
            if let Some(grid) = &self.grid {
                self.grid_scroll_to = grid.rows.iter().position(|&r| r == id);
                self.grid_cell = column.and_then(|c| grid.columns.iter().position(|g| *g == c));
            }
        }
        Ok(true)
    }

    /// Makes `id` the selected element: grid shows its children, tree reveals it, text scrolls.
    pub fn select(&mut self, id: NodeId) {
        let id = self.element_or_parent(id);
        self.selected = Some(id);
        self.focus = Some(id);
        self.mark = None;
        self.grid = Some(Grid::build(&self.doc, id));
        self.grid_cell = None;
        self.grid_edit = None;
        self.tree.reveal(&self.doc, id);
        self.scroll_text_to = Some(self.doc.line_of(self.doc.index.node(id).start));
    }

    /// Marks `id` in the grid and tree and highlights it in the text view, without changing the
    /// grid.
    pub fn focus(&mut self, id: NodeId) {
        self.focus = Some(id);
        let n = self.doc.index.node(id);
        self.mark = (self.selected != Some(id)).then_some(n.start..n.end);
        self.scroll_text_to = Some(self.doc.line_of(n.start));
    }

    /// Highlights the `index`th attribute of `element` in the text view.
    pub fn mark_attribute(&mut self, element: NodeId, index: u32) {
        let spans = self.doc.index.attribute_spans(&self.doc.src, element);
        if let Some(span) = spans.get(index as usize) {
            self.mark = Some(span.start as u64..span.end as u64);
            self.scroll_text_to = Some(self.doc.line_of(span.start as u64));
        }
    }

    /// Highlights the text that the index keeps inside `element` (D11), like a text node.
    pub fn mark_text(&mut self, element: NodeId) {
        if let Some(span) = self.doc.index.folded_span(&self.doc.src, element) {
            self.focus = Some(element);
            self.mark = Some(span.start as u64..span.end as u64);
            self.scroll_text_to = Some(self.doc.line_of(span.start as u64));
        }
    }

    fn element_or_parent(&self, mut id: NodeId) -> NodeId {
        while id != ROOT && self.doc.index.node(id).kind != NodeKind::Element {
            id = self.doc.index.node(id).parent;
        }
        id
    }

    /// Deepest element whose span contains `offset`.
    pub fn element_at(&self, offset: u64) -> Option<NodeId> {
        let idx = &self.doc.index;
        let mut cur = ROOT;
        'descend: loop {
            for c in idx.children(cur) {
                let n = idx.node(c);
                if n.kind == NodeKind::Element && n.start <= offset && offset < n.end {
                    cur = c;
                    continue 'descend;
                }
                if n.start > offset {
                    break;
                }
            }
            break;
        }
        (cur != ROOT).then_some(cur)
    }
}

/// The element at `position`, else the one before it among its siblings, else the nearest
/// ancestor that is still there. After a delete the element at the position is the one that
/// followed the deleted one.
fn nearest_element(doc: &Document, mut position: Vec<u32>) -> Option<NodeId> {
    let idx = &doc.index;
    while let Some(&last) = position.last() {
        if let Some(id) = idx.element_at_position(&position) {
            return Some(id);
        }
        if last > 0 {
            *position.last_mut().expect("not empty") = last - 1;
            if let Some(id) = idx.element_at_position(&position) {
                return Some(id);
            }
        }
        position.pop();
    }
    None
}

/// `s` cut from the left to at most `max` characters, with `…` where it was cut: the end of
/// a folder path says more than its start.
fn shorten_left(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let tail: String = s.chars().skip(count - (max - 1)).collect();
    format!("…{tail}")
}

/// Whether two paths name the same file: equal, or equal once links and `..` are resolved.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || std::fs::canonicalize(a)
            .ok()
            .zip(std::fs::canonicalize(b).ok())
            .is_some_and(|(a, b)| a == b)
}

/// What happened to the file, after its name.
fn disk_text(change: DiskChange) -> &'static str {
    match change {
        DiskChange::Changed => "changed on disk",
        DiskChange::Removed => "was deleted or moved on disk",
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifts_follow_the_moved_elements() {
        let swap = Shift::Swap(vec![0, 2], vec![0, 3]);
        assert_eq!(swap.remap(&[0, 2]), Some(vec![0, 3]));
        assert_eq!(swap.remap(&[0, 3, 1]), Some(vec![0, 2, 1]));
        assert_eq!(swap.remap(&[0, 4]), Some(vec![0, 4]));
        assert_eq!(swap.remap(&[0]), Some(vec![0]));
        let insert = Shift::Insert(vec![0, 2]);
        assert_eq!(insert.remap(&[0, 1]), Some(vec![0, 1]));
        assert_eq!(insert.remap(&[0, 2, 5]), Some(vec![0, 3, 5]));
        assert_eq!(insert.remap(&[1, 3]), Some(vec![1, 3]));
        let remove = Shift::Remove(vec![0, 2]);
        assert_eq!(remove.remap(&[0, 2]), None);
        assert_eq!(remove.remap(&[0, 2, 0]), None);
        assert_eq!(remove.remap(&[0, 3, 1]), Some(vec![0, 2, 1]));
        assert_eq!(remove.remap(&[0, 1]), Some(vec![0, 1]));
        assert_eq!(remove.remap(&[0]), Some(vec![0]));
    }

    #[test]
    fn nearest_element_falls_back_to_the_previous_sibling_then_the_parent() {
        let doc = Document::from_bytes("<r><a/><b><c/></b></r>".as_bytes().to_vec());
        let name = |p: Vec<u32>| {
            let id = nearest_element(&doc, p).unwrap();
            String::from_utf8_lossy(doc.index.name(&doc.src, id)).into_owned()
        };
        assert_eq!(name(vec![0, 1]), "b");
        assert_eq!(name(vec![0, 2]), "b");
        assert_eq!(name(vec![0, 1, 1]), "c");
        assert_eq!(name(vec![0, 0, 0]), "a");
        assert_eq!(name(vec![0, 5]), "r");
    }

    /// OPEN-5: a file already open is found again under another spelling of its path.
    #[test]
    fn same_file_sees_through_dot_dot() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-same-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let file = dir.join("größe.xml");
        std::fs::write(&file, "<a/>").unwrap();
        let other = dir.join("sub").join("..").join("größe.xml");
        assert!(same_file(&file, &other));
        assert!(!same_file(&file, &dir.join("sub")));
        // A file that does not exist is only the same as its own path.
        let gone = dir.join("gone.xml");
        assert!(same_file(&gone, &gone));
        assert!(!same_file(&gone, &file));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
