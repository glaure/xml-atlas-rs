//! Window layout, file loading and selection state.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use egui::{Key, KeyboardShortcut, Modifiers, RichText, ThemePreference, Ui};
use xml_atlas_engine::command::CommandError;
use xml_atlas_engine::xml::{NodeId, NodeKind, ROOT};
use xml_atlas_engine::{
    DiskChange, Document, Edit, EditError, FormatError, FormatOptions, REPARSE_INLINE_BYTES,
    Reparsed, SaveError, Unencodable,
};

use crate::disk_watch::DiskWatch;
use crate::settings::{Layout, LightStyle, Settings, ThemeChoice};
use crate::tabs::Tabs;
use crate::tool_icons::{self, Tool};
use crate::update::{self, Notice};
use crate::{problems_panel, tree};
use xml_atlas_engine::grid::Grid;
use xml_atlas_engine::problems::{Problem, Severity};
use xml_atlas_engine::references::{self, Rules};

/// Time a frame spends looking at a long grid's rows for columns (GRID-3).
const GRID_SCAN_TIME: Duration = Duration::from_millis(6);

/// Children of the grid's element looked at between checks of [`GRID_SCAN_TIME`], per thread
/// that looks ([`xml_atlas_engine::grid::scan_threads`]): about half a millisecond on one
/// thread, 1 to 2 ms on eight at 300 MiB.
const GRID_SCAN_CHILDREN: usize = 4096;

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
    /// The light style handed to egui last (WIN-8).
    applied_light: Option<LightStyle>,
    /// An action waiting for the answer to "save changes?" about the tab shown (OPEN-11).
    confirm: Option<Action>,
    /// Set once the user agreed to close, so the close request is not asked about again.
    quitting: bool,
    find: crate::find_bar::FindBar,
    /// Edit > XPath… (D35).
    xpath: crate::xpath_bar::XPathBar,
    /// Files being opened, in the order asked for.
    loading: Vec<Loading>,
    /// Files the system asks to open while the app runs: from Finder on macOS (D26).
    opened: Option<Receiver<PathBuf>>,
    /// The help windows (WIN-4).
    help: crate::help::Help,
    /// View > Font… while it is open (TEXT-25).
    font_dialog: Option<crate::fonts::FontDialog>,
    /// File > Settings… (WIN-11).
    settings_window: crate::settings_window::SettingsWindow,
    /// The update check under way (PKG-7, D30).
    updates: update::Checker,
    /// What the last update check found, in the status bar until it is closed.
    update_notice: Option<Notice>,
    /// Agents' calls, while View > Allow AI agents (MCP) is on (D28).
    agent: Option<xml_atlas_mcp::bridge::Listener>,
    message: Option<String>,
    /// A note about the last command, shown in the status bar until the next one.
    notice: Option<Note>,
    /// The Format options dialog, with the values being edited.
    format_dialog: Option<FormatOptions>,
    /// Wakes the window when the file of an open document changes on disk (OPEN-14).
    disk_watch: DiskWatch,
    /// Saving the tab shown was refused because its text does not fit the file's encoding;
    /// the dialog offers Save as UTF-8 (OPEN-8).
    unencodable: Option<(PathBuf, Unencodable)>,
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
    /// The tab's number for agents (D28); 0 until the tab is added.
    pub number: u64,
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
    /// The element Find keeps to (TEXT-26), while the Find bar is open; it stays when a match
    /// selects another element, and follows edits as the selection does.
    pub find_within: Option<NodeId>,
    /// The tree asked for the Find bar, to find in `find_within`.
    pub find_requested: bool,
    /// The last XPath query on this document and what it gave (D35).
    pub xpath: Option<crate::xpath_bar::Results>,
    /// The structure diagram (D36): its boxes, what is collapsed, zoom and position.
    pub diagram: crate::diagram::Diagram,
    pub tree: tree::TreeState,
    pub grid: Option<Grid>,
    /// Where the text view jumps on the next frame: a byte of the source. Its line, or with
    /// word wrap its row, goes near the top, and a long line scrolls sideways to it.
    pub scroll_text_to: Option<u64>,
    /// Warnings and errors, in document order (OPEN-13), with the references that find
    /// nothing (REF-4).
    pub problems: Vec<Problem>,
    /// The reference rules for this file (REF-1), read when it opened, or why they could
    /// not be read.
    pub references: Option<Result<(PathBuf, Arc<Rules>), String>>,
    /// References that find nothing, as of the last check.
    reference_problems: Vec<Problem>,
    /// When to check the references again: typing waits for a pause.
    pub(crate) references_due: Option<Instant>,
    /// The reference preview of the selected element (REF-5), kept until it changes.
    pub reference_preview: Option<crate::ref_preview::Cached>,
    /// The sections chosen for `nearest` contexts in this tab (REF-9).
    pub reference_choices: Vec<crate::ref_preview::StoredChoice>,
    /// Expand repeats in the preview (REF-11), switched in the pane; `None` until then: as
    /// the rules file says.
    pub expand_repeats: Option<bool>,
    /// The sections around the selected element, and the element and revision they are for.
    pub reference_sections: Option<(NodeId, u64, Vec<references::Sections>)>,
    /// Where the arrow keys go: the tree or the grid, whichever was clicked last.
    pub keys: Pane,
    /// Grid row to scroll into view on the next frame.
    pub grid_scroll_to: Option<usize>,
    /// Scroll the marked cell (or, with none, the row's start) into view sideways on the
    /// next frame: Left or Right moved it.
    pub grid_reveal_cell: bool,
    /// The marked grid cell of the marked row: an index into the grid's columns. `None`
    /// marks the whole row.
    pub grid_cell: Option<usize>,
    /// The grid cell being edited (GRID-10).
    pub grid_edit: Option<crate::grid::CellEditor>,
    /// What the last edit command did, or why it could not; the app shows it.
    pub note: Option<Note>,
    /// What the pane beside the grid shows for the clicked cell (GRID-16).
    pub cell_pane: Option<crate::cell_pane::CellPane>,
    /// The text cursor and selection (TEXT-16); `None` until the text is clicked.
    pub text_sel: Option<xml_atlas_engine::text::Selection>,
    /// The display column Up and Down keep, from where they started.
    pub text_goal: Option<usize>,
    /// Scroll the cursor into view on the next frame.
    pub text_reveal: bool,
    /// Give the text view the keyboard on the next frame: an Edit menu command acts there.
    pub text_focus: bool,
    /// Text an input method is composing at the cursor; shown, not yet in the document
    /// (TEXT-19).
    pub text_preedit: Option<crate::text_view::Preedit>,
    /// The document being read again on a worker thread after typing broke it (D19).
    reparse: Option<Receiver<Reparsed>>,
    /// Lines the text view showed last frame; rows with word wrap, for Page Up and Down.
    pub text_rows: std::ops::Range<usize>,
    /// How far the text view is scrolled down, by whole lines (D16, TEXT-8).
    pub text_scroll: crate::scroll::RowScroll,
    /// How far the text view is scrolled sideways, in pixels; `f64`, as a line of 100 MiB is
    /// about 800 million pixels wide (TEXT-5).
    pub text_left: f64,
    /// The widest line the text view drew last frame, gutter included.
    pub text_width: f64,
    /// Columns of the long lines the text view shows (TEXT-5).
    pub text_long: crate::text_view::LongLines,
    /// Where the lines break into rows with word wrap (TEXT-13).
    pub text_wrap: xml_atlas_engine::wrap::Wrap,
    /// Whether the text view wrapped lines last frame.
    pub text_wrapped: bool,
    /// Word wrap for this tab alone, over the setting: a minified file opens wrapped
    /// (TEXT-23).
    pub wrap: Option<bool>,
    /// The file is minified, and the notice that says so waits for Format or Dismiss
    /// (TEXT-23).
    pub minified_notice: bool,
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
    /// `paths` came on the command line; `opened` brings more while the app runs (D26).
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        paths: Vec<PathBuf>,
        opened: Option<Receiver<PathBuf>>,
    ) -> Self {
        stop_at_the_edge(&cc.egui_ctx);
        no_animations(&cc.egui_ctx);
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
            applied_light: None,
            confirm: None,
            quitting: false,
            find,
            xpath: Default::default(),
            loading: Vec::new(),
            opened,
            help: Default::default(),
            font_dialog: None,
            settings_window: Default::default(),
            updates: Default::default(),
            update_notice: None,
            agent: None,
            message: None,
            notice: None,
            format_dialog: None,
            unencodable: None,
            disk_watch: DiskWatch::start(cc.egui_ctx.clone()),
        };
        if let Err(e) = crate::fonts::apply(&cc.egui_ctx, &mut app.settings.text_font) {
            log::warn!("{e}; the text view uses the built-in font");
            app.settings.text_font.family = None;
        }
        for path in paths {
            app.open_file(&cc.egui_ctx, path);
        }
        #[cfg(target_os = "macos")]
        crate::macos_open::wake(&cc.egui_ctx);
        app.open_requested(&cc.egui_ctx);
        app
    }

    /// Opens the files the system asked for since the last frame, each in its own tab.
    fn open_requested(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = self
            .opened
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for path in paths {
            self.open_file(ctx, path);
        }
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
        crate::agent::add_tab(
            &mut self.docs,
            &mut self.next_id,
            self.settings.tree_attributes,
            doc,
        );
    }

    /// Lets agents in while View > Allow AI agents (MCP) is on, and runs their calls on the
    /// tabs (D28).
    fn serve_agents(&mut self, ctx: &egui::Context) {
        if self.settings.agents != self.agent.is_some() {
            self.agent = None;
            if self.settings.agents {
                let started = xml_atlas_mcp::bridge::endpoint_file()
                    .ok_or_else(|| "there is no folder for the settings".to_string())
                    .and_then(|file| {
                        let ctx = ctx.clone();
                        xml_atlas_mcp::bridge::Listener::start(file, move || ctx.request_repaint())
                            .map_err(|e| e.to_string())
                    });
                match started {
                    Ok(listener) => self.agent = Some(listener),
                    Err(e) => {
                        self.message = Some(format!("Cannot let agents in: {e}"));
                        self.settings.agents = false;
                    }
                }
            }
        }
        let Some(agent) = &self.agent else {
            return;
        };
        let calls: Vec<_> = agent.requests.try_iter().collect();
        for request in calls {
            // Typing may have left an index behind its text (TEXT-22).
            for open in self.docs.iter_mut() {
                open.settle();
            }
            let mut docs = crate::agent::WindowDocs {
                docs: &mut self.docs,
                next_id: &mut self.next_id,
                tree_attributes: self.settings.tree_attributes,
            };
            let result = xml_atlas_mcp::call(&mut docs, &request.tool, &request.arguments);
            let changes = xml_atlas_mcp::tools()
                .iter()
                .any(|t| t.name == request.tool && !t.read_only);
            let target = request.arguments.get("path").and_then(|p| p.as_str());
            let what = match target {
                Some(p) => format!("{} {p}", request.tool),
                None => request.tool.clone(),
            };
            match &result {
                Ok(_) if changes => self.notice = Some(Note::info(format!("Agent: {what}"))),
                Ok(_) => {}
                Err(e) => self.notice = Some(Note::refused(format!("Agent: {what}: {e}"))),
            }
            request.answer(result.map_err(|e| e.to_string()));
        }
    }

    /// Light, dark, or the desktop's preference (WIN-2), and the light style (WIN-8). On Linux the desktop's preference comes
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
        if self.applied_light != Some(self.settings.light_style) {
            let light = crate::theme::Theme::light_in(self.settings.light_style);
            crate::theme::install(ctx, light, crate::theme::Theme::dark());
            self.applied_light = Some(self.settings.light_style);
        }
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

    /// Saving was refused because the text does not fit the file's encoding (OPEN-8): says
    /// what and where, and offers to show it or to save as UTF-8 instead.
    fn encoding_dialog(&mut self, ui: &mut Ui) {
        let Some((path, error)) = self.unencodable.clone() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let Some(open) = self.docs.active() else {
            self.unencodable = None;
            return;
        };
        let text = match &error {
            Unencodable::Char {
                offset,
                ch,
                encoding,
            } => {
                let line = open.doc.line_of(*offset as u64);
                let column = xml_atlas_engine::text::column(&open.doc, *offset) + 1;
                format!(
                    "{ch} on line {}, column {column}, cannot be written in {encoding}, the \
                     file's encoding. Nothing was saved.",
                    line + 1
                )
            }
            Unencodable::Lossy { encoding } => format!(
                "The file has bytes that are not valid {encoding}; they are shown as \u{FFFD} \
                 and cannot be written back. Nothing was saved."
            ),
        };
        let (mut show, mut utf8, mut cancel) = (false, false, false);
        let modal = egui::Modal::new(egui::Id::unique("unencodable")).show(&ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("Cannot save in this encoding");
            ui.label(text);
            ui.label("Save as UTF-8 writes the file in UTF-8, and its declaration says so.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                utf8 = ui.button("Save as UTF-8").clicked();
                if matches!(error, Unencodable::Char { .. }) {
                    show = ui.button("Show it").clicked();
                }
                cancel = ui.button("Cancel").clicked();
            });
        });
        if utf8 {
            self.unencodable = None;
            self.save_as_utf8(Some(path));
        } else if show {
            self.unencodable = None;
            if let (Some(open), Unencodable::Char { offset, ch, .. }) =
                (self.docs.active_mut(), error)
            {
                open.text_sel = Some(xml_atlas_engine::text::Selection {
                    anchor: offset,
                    head: offset + ch.len_utf8(),
                });
                open.scroll_text_to = Some(offset as u64);
                open.text_reveal = true;
                open.text_focus = true;
            }
        } else if cancel || modal.should_close() {
            self.unencodable = None;
        }
    }

    /// File > Save as UTF-8 (OPEN-8): the document is saved in UTF-8 from now on, its
    /// declaration changed to say so, to `path` or to its own file.
    fn save_as_utf8(&mut self, path: Option<PathBuf>) {
        let Some(open) = self.docs.active_mut() else {
            return;
        };
        if let Err(e) = open.edit(|doc| Ok(doc.convert_to_utf8())) {
            self.message = Some(format!("Cannot convert to UTF-8: {e}"));
            return;
        }
        let path = path.or_else(|| open.doc.path.clone());
        match path {
            Some(p) => {
                if self.save_to(p) {
                    self.notice = Some(Note::info("Saved as UTF-8".to_string()));
                }
            }
            None => {
                self.save();
            }
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
        // Typing may have left the index behind the text: read it first (D19).
        if let Some(open) = self.docs.active_mut() {
            open.settle();
        }
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
        if let Some(open) = self.docs.active_mut() {
            open.formatted();
        }
    }

    /// View > Word wrap and Alt+Z (TEXT-13): for a tab with a choice of its own (a minified
    /// file, TEXT-23) only that tab, else the setting for every tab.
    fn toggle_wrap(&mut self) {
        let setting = self.settings.word_wrap;
        match self.docs.active_mut() {
            Some(open) if open.wrap.is_some() => open.wrap = Some(!open.wraps(setting)),
            _ => self.settings.word_wrap = !setting,
        }
    }

    /// Edit > Compact (FMT-14): the document on one line.
    fn compact_document(&mut self) {
        // Typing may have left the index behind the text: read it first (D19).
        if let Some(open) = self.docs.active_mut() {
            open.settle();
        }
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
            open.cursor_to_change();
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
            open.cursor_to_change();
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
            crate::settings_window::format_options(ui, &mut edit);
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
            Err(SaveError::Encoding(e)) => {
                self.unencodable = Some((path, e));
                false
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

    /// The notice above the views for a minified file (TEXT-23): it is shown wrapped, and
    /// Format spreads it over lines.
    fn minified_bar(&mut self, ui: &mut Ui) {
        let Some(open) = self.docs.active() else {
            return;
        };
        let name = open.doc.path.as_deref().map(file_name).unwrap_or_default();
        let broken = open.problems.iter().any(|p| p.severity == Severity::Error);
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "ℹ {name} is minified: its elements are on one long line, shown wrapped."
            ));
            if !broken
                && ui
                    .button("Format")
                    .on_hover_text(
                        "Put each element on its own indented line. One undo step; the file \
                         changes only when you save",
                    )
                    .clicked()
            {
                self.format_document();
            }
            if ui.button("Dismiss").clicked()
                && let Some(open) = self.docs.active_mut()
            {
                open.minified_notice = false;
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
                if ui.button("New    Ctrl+N").clicked() {
                    ui.close();
                    self.new_document();
                }
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
                if self
                    .docs
                    .active()
                    .is_some_and(|o| o.doc.encoding().is_some())
                    && ui.button("Save as UTF-8").clicked()
                {
                    ui.close();
                    self.save_as_utf8(None);
                }
                if ui
                    .add_enabled(!self.docs.is_empty(), egui::Button::new("Close    Ctrl+W"))
                    .clicked()
                {
                    ui.close();
                    self.close_tab(&ctx);
                }
                ui.separator();
                if ui.button("Settings…    Ctrl+,").clicked() {
                    ui.close();
                    self.settings_window.open = true;
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
                self.clipboard_menu(ui);
                ui.separator();
                if ui
                    .add_enabled(!self.docs.is_empty(), egui::Button::new("Find…    Ctrl+F"))
                    .clicked()
                {
                    ui.close();
                    self.find.open(self.docs.active_mut());
                }
                let selected = self.docs.active().is_some_and(|o| o.selected.is_some());
                if ui
                    .add_enabled(
                        selected,
                        egui::Button::new("Find in element…    Ctrl+Shift+F"),
                    )
                    .on_hover_text("Find only inside the element selected in tree or grid")
                    .clicked()
                {
                    ui.close();
                    if let Some(open) = self.docs.active_mut() {
                        self.find.open_within(open);
                    }
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
                    .add_enabled(
                        !self.docs.is_empty(),
                        egui::Button::new("XPath…    Ctrl+Shift+X"),
                    )
                    .on_hover_text("Select nodes with an XPath 1.0 expression")
                    .clicked()
                {
                    ui.close();
                    self.xpath.open();
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
                    if ui.button("Collapse the tree").clicked() {
                        ui.close();
                        if let Some(open) = self.docs.active_mut() {
                            open.tree.collapse_all(&open.doc);
                        }
                    }
                });
                ui.add_enabled(
                    !self.settings.diagram,
                    egui::Checkbox::new(&mut self.settings.grid, "Grid"),
                )
                .on_hover_text("Hidden, the text and the reference preview take its place");
                ui.checkbox(&mut self.settings.diagram, "Diagram    Ctrl+Shift+D")
                    .on_hover_text(
                        "The structure of the document in the grid's place: one box per path \
                         of element names, with how many there are (D36)",
                    );
                ui.add_enabled(
                    self.settings.grid && !self.settings.diagram,
                    egui::Checkbox::new(
                        &mut self.settings.cell_pane,
                        "Cell pane beside the grid (experiment)",
                    ),
                );
                ui.checkbox(&mut self.settings.toolbar, "Toolbar");
                ui.checkbox(
                    &mut self.settings.reference_preview,
                    "Preview with references resolved",
                )
                .on_hover_text(
                    "A pane with the selected element as it reads with its references \
                     resolved, from the rules in xml-atlas-references.json (REF-5)",
                );
                ui.add_enabled(
                    self.settings.reference_preview,
                    egui::Checkbox::new(&mut self.settings.preview_tree, "Preview tree"),
                )
                .on_hover_text("The preview as a tree, right of it (REF-10)");
                if ui.button("Font…").clicked() {
                    ui.close();
                    self.font_dialog =
                        Some(crate::fonts::FontDialog::new(&self.settings.text_font));
                }
                let setting = self.settings.word_wrap;
                let mut wrap = self.docs.active().map_or(setting, |o| o.wraps(setting));
                if ui.checkbox(&mut wrap, "Word wrap    Alt+Z").changed() {
                    self.toggle_wrap();
                }
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
            ui.menu_button("Help", |ui| {
                if ui.button("XML Atlas Help    F1").clicked() {
                    ui.close();
                    self.help.guide = true;
                }
                if ui
                    .add_enabled(
                        !self.updates.running(),
                        egui::Button::new("Check for updates now"),
                    )
                    .clicked()
                {
                    ui.close();
                    self.check_for_updates_now(ui.ctx());
                }
                if ui.button("About XML Atlas").clicked() {
                    ui.close();
                    self.help.about = true;
                }
            });
        });
    }

    /// Edit > Cut, Copy, Paste and Select All (TEXT-17, TEXT-18). They act in the text view:
    /// it gets the keyboard back, and egui hands it the same events the keys make, on the
    /// next frame.
    fn clipboard_menu(&mut self, ui: &mut Ui) {
        let sel = self.docs.active().and_then(|o| o.text_sel);
        let selected = sel.is_some_and(|s| !s.is_empty());
        let open = !self.docs.is_empty();
        let items = [
            (
                "Cut    Ctrl+X",
                selected,
                Some(egui::ViewportCommand::RequestCut),
            ),
            (
                "Copy    Ctrl+C",
                selected,
                Some(egui::ViewportCommand::RequestCopy),
            ),
            (
                "Paste    Ctrl+V",
                open,
                Some(egui::ViewportCommand::RequestPaste),
            ),
            ("Select All    Ctrl+A", open, None),
        ];
        for (label, enabled, command) in items {
            if !ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                continue;
            }
            ui.close();
            self.clipboard(ui.ctx(), command);
        }
    }

    /// Cut, Copy, Paste (`command`) or, without one, Select All, in the text view: it gets
    /// the keyboard back, and egui hands it the same events the keys make, on the next frame.
    fn clipboard(&mut self, ctx: &egui::Context, command: Option<egui::ViewportCommand>) {
        let Some(open) = self.docs.active_mut() else {
            return;
        };
        open.text_focus = true;
        match command {
            Some(command) => ctx.send_viewport_cmd(command),
            None => {
                open.text_sel = Some(xml_atlas_engine::text::select_all(&open.doc));
                open.text_goal = None;
                open.doc.history.seal();
            }
        }
    }

    /// File > Settings… (WIN-11). The window changes the settings; what a change means beyond
    /// the setting is applied here.
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_window.open {
            return;
        }
        let before = self.settings.clone();
        let request = self
            .settings_window
            .show(ctx, &mut self.settings, self.updates.running());
        if self.settings.tree_attributes != before.tree_attributes {
            // A view setting: for every tab, as View > Attributes in the tree does.
            for open in self.docs.iter_mut() {
                open.tree.set_attributes(self.settings.tree_attributes);
            }
        }
        if self.settings.text_font != before.text_font {
            // Only Reset to defaults changes it here; the font dialog applies its own.
            let mut font = self.settings.text_font.clone();
            match crate::fonts::apply(ctx, &mut font) {
                Ok(()) => self.settings.text_font = font,
                Err(e) => {
                    self.message = Some(format!("Cannot use the font: {e}"));
                    self.settings.text_font = before.text_font;
                }
            }
        }
        match request {
            Some(crate::settings_window::Request::ChooseFont) => {
                self.font_dialog = Some(crate::fonts::FontDialog::new(&self.settings.text_font));
            }
            Some(crate::settings_window::Request::CheckForUpdates) => {
                self.check_for_updates_now(ctx);
            }
            None => {}
        }
    }

    /// The update check (PKG-7, D30): takes its answer when it has come, and starts the daily
    /// check when it is due. Called every frame; it costs a look at the clock.
    fn check_for_updates(&mut self, ctx: &egui::Context) {
        if let Some((asked, answer)) = self.updates.poll() {
            let current =
                update::Version::parse(update::CURRENT).unwrap_or(update::Version(0, 0, 0));
            let notice = update::notice(
                asked,
                answer,
                current,
                self.settings.update_shown,
                update::Install::detect(),
            );
            if let Some(Notice::Newer(r, _)) = &notice {
                // Told once: the next start does not tell again.
                self.settings.update_shown = Some(r.version);
            }
            if notice.is_some() {
                self.update_notice = notice;
            }
        }
        let now = update::now();
        if !self.updates.running()
            && update::due(self.settings.update_check, self.settings.update_last, now)
        {
            // Noted when asking, so a check that fails waits a day too.
            self.settings.update_last = Some(now);
            self.updates.start(ctx.clone(), false);
        }
    }

    /// Help > Check for updates now, and *Check now* in Settings: asks at once, also when the
    /// daily check is off, and says what it found.
    fn check_for_updates_now(&mut self, ctx: &egui::Context) {
        self.settings.update_last = Some(update::now());
        self.update_notice = None;
        self.updates.start(ctx.clone(), true);
    }

    /// View > Font… (TEXT-25): the text view takes the font chosen with OK.
    fn font_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.font_dialog else {
            return;
        };
        match dialog.show(ctx) {
            None => {}
            Some(crate::fonts::Answer::Cancel) => self.font_dialog = None,
            Some(crate::fonts::Answer::Use(mut font)) => {
                self.font_dialog = None;
                match crate::fonts::apply(ctx, &mut font) {
                    Ok(()) => self.settings.text_font = font,
                    Err(e) => self.message = Some(format!("Cannot use the font: {e}")),
                }
            }
        }
    }

    /// File > New (OPEN-22): an untitled document in a new tab.
    fn new_document(&mut self) {
        self.add_document(Document::untitled());
        self.message = None;
    }

    /// The toolbar under the menu (WIN-5): the commands used most, as in XML Marker's (XM-58).
    fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let active = self.docs.active();
        let open = active.is_some();
        let modified = active.is_some_and(|o| o.doc.is_modified());
        let selected = active
            .and_then(|o| o.text_sel)
            .is_some_and(|s| !s.is_empty());
        let history = active.map(|o| &o.doc.history);
        let undo = history.is_some_and(|h| h.undo_label().is_some());
        let redo = history.is_some_and(|h| h.redo_label().is_some());
        ui.horizontal(|ui| {
            if tool_icons::button(ui, true, Tool::New, "New    Ctrl+N") {
                self.new_document();
            }
            if tool_icons::button(ui, true, Tool::Open, "Open    Ctrl+O") {
                self.pick_file(&ctx);
            }
            if tool_icons::button(ui, modified, Tool::Save, "Save    Ctrl+S") {
                self.save();
            }
            ui.separator();
            if tool_icons::button(ui, selected, Tool::Cut, "Cut    Ctrl+X") {
                self.clipboard(&ctx, Some(egui::ViewportCommand::RequestCut));
            }
            if tool_icons::button(ui, selected, Tool::Copy, "Copy    Ctrl+C") {
                self.clipboard(&ctx, Some(egui::ViewportCommand::RequestCopy));
            }
            if tool_icons::button(ui, open, Tool::Paste, "Paste    Ctrl+V") {
                self.clipboard(&ctx, Some(egui::ViewportCommand::RequestPaste));
            }
            ui.separator();
            if tool_icons::button(ui, undo, Tool::Undo, "Undo    Ctrl+Z") {
                self.undo();
            }
            if tool_icons::button(ui, redo, Tool::Redo, "Redo    Ctrl+Y") {
                self.redo();
            }
            if tool_icons::button(ui, open, Tool::Find, "Find    Ctrl+F") {
                self.find.open(self.docs.active_mut());
            }
            if tool_icons::button(ui, open, Tool::XPath, "XPath    Ctrl+Shift+X") {
                self.xpath.open();
            }
            ui.separator();
            if tool_icons::button(ui, open, Tool::DrillUp, "Drill Up    Alt+Up")
                && let Some(open) = self.docs.active_mut()
            {
                crate::nav::drill_up(open);
            }
            if tool_icons::button(ui, open, Tool::DrillDown, "Drill Down    Alt+Down")
                && let Some(open) = self.docs.active_mut()
            {
                crate::nav::drill_down(open);
            }
            if tool_icons::button(ui, open, Tool::Format, "Format") {
                self.format_document();
            }
            ui.separator();
            if tool_icons::button(ui, open, Tool::Diagram, "Diagram    Ctrl+Shift+D") {
                self.settings.diagram = !self.settings.diagram;
            }
        });
    }

    /// The status bar: the file and the last command on the left, the update notice (PKG-7)
    /// on the right.
    fn status_bar(&mut self, ui: &mut Ui) {
        if self.update_notice.is_none() {
            self.status_text(ui);
            return;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            self.update_notice(ui);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                self.status_text(ui);
            });
        });
    }

    /// The update notice, right to left: its close button, then its text.
    fn update_notice(&mut self, ui: &mut Ui) {
        let Some(notice) = &self.update_notice else {
            return;
        };
        let close = ui
            .small_button("×")
            .on_hover_text("Close this notice")
            .clicked();
        match notice.link() {
            Some(url) => {
                ui.hyperlink_to(notice.text(), url).on_hover_text(url);
            }
            None if matches!(notice, Notice::Failed(_)) => {
                ui.colored_label(ui.visuals().warn_fg_color, notice.text());
            }
            None => {
                ui.label(notice.text());
            }
        }
        if close {
            self.update_notice = None;
        }
    }

    fn status_text(&mut self, ui: &mut Ui) {
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
            // The file's encoding, when it is not UTF-8 (OPEN-8).
            let encoding = open
                .doc
                .encoding()
                .map(|e| format!("  ·  {}", e.label))
                .unwrap_or_default();
            ui.label(format!(
                "{name}{}{encoding}  ·  {:.1} MiB  ·  {} lines  ·  {} elements  ·  opened in {:.0} ms",
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
            if self.agent.as_ref().is_some_and(|a| a.connected() > 0) {
                ui.label(RichText::new("  ·  Agent connected").strong());
            }
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
        self.check_for_updates(&ctx);
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
        self.open_requested(&ctx);
        self.serve_agents(&ctx);
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O)))
        {
            self.pick_file(&ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::N)))
        {
            self.new_document();
        }
        if ctx.input_mut(|i| {
            let pressed = i.consume_shortcut(&KeyboardShortcut::new(Modifiers::ALT, Key::Z));
            if pressed {
                // On Linux the key also comes as the letter, which would be typed.
                i.events
                    .retain(|e| !matches!(e, egui::Event::Text(t) if t.eq_ignore_ascii_case("z")));
            }
            pressed
        }) {
            // View > Word wrap (TEXT-13).
            self.toggle_wrap();
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
        self.encoding_dialog(ui);
        self.format_dialog(&ctx);
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F1)) {
            self.help.guide = true;
        }
        crate::help::show(&ctx, &mut self.help);
        self.font_dialog(&ctx);
        if ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma))
        }) {
            self.settings_window.open = true;
        }
        self.settings_window(&ctx);
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

        // Typing broke the text of a document: read it again after a pause (D19).
        for open in self.docs.iter_mut() {
            open.reparse_when_due(&ctx);
        }
        if let Some(open) = self.docs.active_mut() {
            open.scan_grid(&ctx);
        }
        // Before Ctrl+D (Duplicate), which would match it too.
        if !self.docs.is_empty() && ctx.input_mut(|i| i.consume_shortcut(&crate::diagram::TOGGLE)) {
            self.settings.diagram = !self.settings.diagram;
        }
        // Before Find's: F3 steps through the XPath results while their bar is open, and
        // through a diagram box's elements while the find bar is closed.
        self.xpath.shortcuts(&ctx, self.docs.active_mut());
        if self.settings.diagram
            && !self.find.visible
            && let Some(open) = self.docs.active_mut()
        {
            crate::diagram::shortcuts(&ctx, open);
        }
        self.find.shortcuts(&ctx, self.docs.active_mut());
        if self.confirm.is_none()
            && let Some(open) = self.docs.active_mut()
        {
            // A hidden grid takes no keys: they go to the tree.
            let grid_shown = self.settings.grid && !self.settings.diagram;
            if !grid_shown && open.keys == Pane::Grid {
                open.keys = Pane::Tree;
            }
            crate::nav::handle(&ctx, open);
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        if self.settings.toolbar {
            egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        }
        if !self.docs.is_empty() {
            egui::Panel::top("tabs").show(ui, |ui| self.tab_bar(ui));
        }
        if self.docs.active().is_some_and(|o| o.disk.is_some()) {
            egui::Panel::top("disk").show(ui, |ui| self.disk_bar(ui));
        }
        if self.docs.active().is_some_and(|o| o.minified_notice) {
            egui::Panel::top("minified").show(ui, |ui| self.minified_bar(ui));
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

        // The font chosen in View > Font… for tree, grid and cell pane too (TEXT-25).
        let view_font = self.settings.text_font.view_font_id();
        let theme = crate::theme::current(ui);
        egui::Panel::left("tree")
            .frame(theme.view_frame(ui.style(), false))
            .resizable(true)
            .default_size(320.0)
            .show(ui, |ui| {
                let id = open.id;
                crate::fonts::use_in_views(ui, view_font.as_ref());
                ui.push_id(id, |ui| tree::show(ui, open, view_font.as_ref()));
            });
        if std::mem::take(&mut open.find_requested) {
            self.find.open_within(open);
        }
        let layout = self.settings.layout;
        let format = self.settings.format;
        let cell_pane = self.settings.cell_pane;
        let reference_preview = self.settings.reference_preview;
        let preview_tree = reference_preview && self.settings.preview_tree;
        let show_diagram = self.settings.diagram;
        let show_grid = self.settings.grid && !show_diagram;
        let wrap = open.wraps(self.settings.word_wrap);
        let text_font = self.settings.text_font.font_id();
        egui::CentralPanel::default().show(ui, |ui| {
            let total = ui.available_height();
            if show_diagram {
                // In the grid's place (D36), with a height of its own per layout.
                let diagram = match layout {
                    Layout::GridAbove => egui::Panel::top("diagram"),
                    Layout::TextAbove => egui::Panel::bottom("diagram-below"),
                };
                diagram
                    .frame(theme.view_frame(ui.style(), false))
                    .resizable(true)
                    .default_size(total * 0.45)
                    .show(ui, |ui| {
                        crate::fonts::use_in_views(ui, view_font.as_ref());
                        ui.push_id(open.id, |ui| crate::diagram::show(ui, open));
                    });
            }
            if show_grid {
                // Each layout has its own panel, so each remembers its own height.
                let grid = match layout {
                    Layout::GridAbove => egui::Panel::top("grid"),
                    Layout::TextAbove => egui::Panel::bottom("grid-below"),
                };
                grid.frame(theme.view_frame(ui.style(), false))
                    .resizable(true)
                    .default_size(total * 0.45)
                    .show(ui, |ui| {
                        crate::fonts::use_in_views(ui, view_font.as_ref());
                        ui.push_id(open.id, |ui| {
                            if cell_pane {
                                let width = ui.available_width() * 0.35;
                                egui::Panel::right("cell-pane")
                                    .frame(theme.view_frame(ui.style(), false))
                                    .resizable(true)
                                    .default_size(width)
                                    .show(ui, |ui| {
                                        ui.push_id("cell-pane", |ui| {
                                            crate::cell_pane::show(ui, open)
                                        });
                                    });
                            }
                            egui::CentralPanel::default()
                                .frame(theme.view_frame(ui.style(), true))
                                .show(ui, |ui| crate::grid::show(ui, open));
                        });
                    });
            }
            if !open.problems.is_empty() {
                egui::Panel::bottom("problems")
                    .resizable(true)
                    .show(ui, |ui| {
                        ui.push_id(open.id, |ui| problems_panel::show(ui, open));
                    });
            }
            if self.xpath.visible && crate::xpath_bar::has_results(open) {
                // The views' background, as tree and text have: a list to click in.
                egui::Panel::bottom("xpath-results")
                    .frame(theme.view_frame(ui.style(), false))
                    .resizable(true)
                    .show(ui, |ui| {
                        ui.push_id(open.id, |ui| crate::xpath_bar::show_results(ui, open));
                    });
            }
            if self.find.visible {
                egui::Panel::top("find").show(ui, |ui| self.find.show(ui, open));
            }
            if self.xpath.visible {
                egui::Panel::top("xpath").show(ui, |ui| self.xpath.show(ui, open));
            }
            if preview_tree {
                // Added first, so it is the rightmost: main tree | text | preview | its tree.
                let width = ui.available_width() * 0.2;
                egui::Panel::right("reference-tree")
                    .frame(theme.view_frame(ui.style(), false))
                    .resizable(true)
                    .default_size(width)
                    .show(ui, |ui| {
                        crate::fonts::use_in_views(ui, view_font.as_ref());
                        ui.push_id(open.id, |ui| {
                            crate::ref_preview::show_tree(ui, open, view_font.as_ref())
                        });
                    });
            }
            if reference_preview {
                let width = ui.available_width() * 0.45;
                egui::Panel::right("reference-preview")
                    .frame(theme.view_frame(ui.style(), false))
                    .resizable(true)
                    .default_size(width)
                    .show(ui, |ui| {
                        ui.push_id(open.id, |ui| crate::ref_preview::show(ui, open, &text_font));
                    });
            }
            egui::CentralPanel::default()
                .frame(theme.view_frame(ui.style(), true))
                .show(ui, |ui| {
                    ui.push_id(open.id, |ui| {
                        crate::text_view::show(ui, open, &format, wrap, &text_font)
                    });
                });
        });
        if let Some(note) = open.note.take() {
            self.notice = Some(note);
        }
    }
}

impl Open {
    /// A tab for `doc`, with nothing selected yet.
    pub fn new(doc: Document, id: egui::Id) -> Self {
        let started = std::time::Instant::now();
        let minified = doc.is_minified();
        log::debug!("minified check: {minified} in {:?}", started.elapsed());
        let mut open = Open {
            tree: tree::TreeState::new(&doc),
            number: 0,
            doc,
            id,
            discard: false,
            disk: None,
            selected: None,
            focus: None,
            mark: None,
            find_within: None,
            find_requested: false,
            xpath: None,
            diagram: Default::default(),
            grid: None,
            scroll_text_to: None,
            problems: Vec::new(),
            references: None,
            reference_problems: Vec::new(),
            references_due: None,
            reference_preview: None,
            reference_choices: Vec::new(),
            expand_repeats: None,
            reference_sections: None,
            keys: Pane::Tree,
            grid_scroll_to: None,
            grid_reveal_cell: false,
            grid_cell: None,
            grid_edit: None,
            note: None,
            cell_pane: None,
            text_sel: None,
            text_goal: None,
            text_reveal: false,
            text_focus: false,
            text_scroll: crate::scroll::RowScroll::default(),
            text_left: 0.0,
            text_width: 0.0,
            text_long: Default::default(),
            text_wrap: Default::default(),
            text_wrapped: false,
            text_preedit: None,
            reparse: None,
            text_rows: 0..0,
            wrap: minified.then_some(true),
            minified_notice: minified,
        };
        open.references = open.doc.path.as_deref().and_then(load_rules);
        open.check_references();
        open
    }

    /// The rules, when there are any and they could be read.
    pub fn rules(&self) -> Option<&Arc<Rules>> {
        match &self.references {
            Some(Ok((_, rules))) => Some(rules),
            _ => None,
        }
    }

    /// Checks the references again (REF-4) and rebuilds the problems list. Waits while the
    /// index lags behind typing (D19).
    pub(crate) fn check_references(&mut self) {
        if self.doc.stale().is_some() {
            self.references_due = Some(Instant::now() + xml_atlas_engine::REPARSE_PAUSE);
            return;
        }
        self.references_due = None;
        self.reference_problems = match self.rules() {
            Some(rules) => {
                let t = Instant::now();
                let p = references::problems(&self.doc, rules);
                log::debug!("references checked in {:?}", t.elapsed());
                p
            }
            None => Vec::new(),
        };
        self.update_problems();
    }

    /// The problems list: the document's own, and the references that find nothing.
    fn update_problems(&mut self) {
        let mut all = self.doc.problems();
        all.extend(self.reference_problems.iter().cloned());
        all.sort_by_key(|p| p.offset);
        self.problems = all;
    }

    /// Whether this tab wraps lines: its own choice when it has one, else the `setting`.
    pub fn wraps(&self, setting: bool) -> bool {
        self.wrap.unwrap_or(setting)
    }

    /// After Format (TEXT-23): a file that is no longer minified loses its notice and wraps
    /// as the setting says.
    pub fn formatted(&mut self) {
        if (self.wrap.is_some() || self.minified_notice) && !self.doc.is_minified() {
            self.wrap = None;
            self.minified_notice = false;
        }
    }

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
        // Typing may have left the index behind the text: read it first (D19).
        self.settle();
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
        let find_within = self
            .find_within
            .and_then(|id| shift.remap(&idx.element_position(id)));
        let choices: Vec<_> = self
            .reference_choices
            .iter()
            .filter_map(|c| c.remap(shift))
            .collect();
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
        // Gone when the element was deleted.
        self.find_within = find_within.and_then(|p| idx.element_at_position(&p));
        self.reference_choices = choices;
        self.check_references();
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

    /// Applies what a key typed, cut or pasted in the text view (D19). The index changes only
    /// around it, and tree, grid and problems list follow the replaced nodes instead of being
    /// built again, so a key costs what the change touches, not the size of the file. The
    /// selection stays; when its element was replaced, it goes to the element that starts
    /// where it started.
    pub fn type_edit(&mut self, edit: Edit) -> Result<(), EditError> {
        let selected_start = self.selected.map(|id| self.doc.index.node(id).start);
        let within_start = self.find_within.map(|id| self.doc.index.node(id).start);
        let choices = crate::ref_preview::chosen(self);
        self.doc.apply_typed(edit)?;
        let Some(ns) = self.doc.node_splice() else {
            // Not expected for a typed change; build the views again.
            self.refresh(|_| true);
            return Ok(());
        };
        self.tree.follow(&self.doc, &ns);
        let choices: Vec<_> = choices
            .iter()
            .filter_map(|c| {
                Some(references::Choice {
                    scope: ns.map(c.scope)?,
                    chosen: ns.map(c.chosen)?,
                })
            })
            .collect();
        crate::ref_preview::store(self, &choices);
        let moved = |start: u64| match self.doc.last_change() {
            Some(c) if start >= (c.at + c.removed) as u64 => {
                start + c.inserted as u64 - c.removed as u64
            }
            _ => start,
        };
        self.find_within = self
            .find_within
            .and_then(|id| ns.map(id))
            .or_else(|| within_start.and_then(|s| self.element_at(moved(s))));
        let selected = self
            .selected
            .and_then(|id| ns.map(id))
            .or_else(|| selected_start.and_then(|s| self.element_at(moved(s))))
            .or_else(|| self.doc.index.root_element());
        self.selected = selected;
        self.focus = selected;
        self.mark = None;
        if let Some(id) = selected
            && self.tree.selected().is_none()
        {
            self.tree.mark(xml_atlas_engine::tree::Row::Node(id));
        }
        let followed = match &mut self.grid {
            Some(grid) => grid.follow(&self.doc, &ns),
            None => true,
        };
        if !followed {
            let view = self.grid.as_ref().map(|g| g.view().clone());
            self.grid = selected.map(|id| {
                let mut grid = Grid::start(&self.doc, id);
                if let Some(view) = view {
                    grid.set_view(&self.doc, view);
                }
                grid
            });
            self.grid_cell = None;
        }
        self.grid_edit = None;
        // Checking every reference per key costs too much on a large file: after a pause.
        self.reference_problems.clear();
        self.update_problems();
        if self.rules().is_some() {
            self.references_due = Some(Instant::now() + xml_atlas_engine::REPARSE_PAUSE);
        }
        Ok(())
    }

    /// Reads the document again when typing broke it and the pause is over (TEXT-22, D19):
    /// on this thread up to [`REPARSE_INLINE_BYTES`], else on a worker thread with a copy of
    /// the source. Call once per frame for every open document.
    pub fn reparse_when_due(&mut self, ctx: &egui::Context) {
        if let Some(due) = self.references_due {
            let now = Instant::now();
            if now >= due {
                self.check_references();
            } else {
                ctx.request_repaint_after(due - now);
            }
        }
        if let Some(rx) = &self.reparse {
            match rx.try_recv() {
                Ok(done) => {
                    self.reparse = None;
                    let t = Instant::now();
                    self.refresh(|doc| doc.finish_reparse(done));
                    log::debug!("worker result taken in: {:?}", t.elapsed());
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => self.reparse = None,
            }
        }
        let Some(wait) = self.doc.reparse_wait(Instant::now()) else {
            return;
        };
        if !wait.is_zero() {
            ctx.request_repaint_after(wait);
        } else if self.doc.src.len() <= REPARSE_INLINE_BYTES {
            let t = Instant::now();
            self.refresh(Document::reparse);
            log::debug!("read again on the UI thread: {:?}", t.elapsed());
        } else if let Some(job) = {
            let t = Instant::now();
            let job = self.doc.reparse_job();
            log::debug!(
                "read again on a worker thread; copying the text: {:?}",
                t.elapsed()
            );
            job
        } {
            let (tx, rx) = channel();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                // The document may have been closed meanwhile; then nobody listens.
                let _ = tx.send(job.run());
                ctx.request_repaint();
            });
            self.reparse = Some(rx);
        }
    }

    /// Looks at more of the grid's rows for columns (GRID-3), for at most [`GRID_SCAN_TIME`]
    /// a frame, until all have been. Call once per frame for the document shown.
    pub fn scan_grid(&mut self, ctx: &egui::Context) {
        let Some(grid) = &mut self.grid else { return };
        if grid.scanned() {
            return;
        }
        let t = Instant::now();
        let children = GRID_SCAN_CHILDREN * xml_atlas_engine::grid::scan_threads();
        while !grid.scan(&self.doc, children) {
            if t.elapsed() >= GRID_SCAN_TIME {
                ctx.request_repaint();
                return;
            }
        }
        log::debug!("grid columns: last step {:?}", t.elapsed());
    }

    /// Reads the document again now if typing left the index behind, before a command that
    /// works from the index.
    pub fn settle(&mut self) {
        self.reparse = None;
        self.refresh(Document::reparse);
    }

    /// Rebuilds tree, grid and problems when `change` changed the index but not the text,
    /// keeping what is selected and expanded; nothing scrolls.
    fn refresh(&mut self, change: impl FnOnce(&mut Document) -> bool) {
        let text_scroll = self.scroll_text_to;
        if let Ok(true) = self.edit(|doc| Ok(change(doc))) {
            self.scroll_text_to = text_scroll;
            self.tree.keep_scroll();
        }
    }

    /// After undo or redo, the text cursor goes where the change was, behind what it put back
    /// (TEXT-20). Only when the text has a cursor already.
    pub fn cursor_to_change(&mut self) {
        if self.text_sel.is_none() {
            return;
        }
        if let Some(c) = self.doc.last_change() {
            self.text_sel = Some(xml_atlas_engine::text::Selection::cursor(c.at + c.inserted));
            self.text_goal = None;
            self.text_reveal = true;
            self.scroll_text_to = None;
        }
    }

    /// Makes `id` the selected element: grid shows its children, tree reveals it, text scrolls.
    pub fn select(&mut self, id: NodeId) {
        let id = self.element_or_parent(id);
        self.selected = Some(id);
        self.focus = Some(id);
        self.mark = None;
        self.grid = Some(Grid::start(&self.doc, id));
        self.grid_cell = None;
        self.grid_edit = None;
        self.tree.reveal(&self.doc, id);
        self.scroll_text_to = Some(self.doc.index.node(id).start);
    }

    /// Marks `id` in the grid and tree and highlights it in the text view, without changing the
    /// grid.
    pub fn focus(&mut self, id: NodeId) {
        self.focus = Some(id);
        let n = self.doc.index.node(id);
        self.mark = (self.selected != Some(id)).then_some(n.start..n.end);
        self.scroll_text_to = Some(n.start);
    }

    /// Highlights the `index`th attribute of `element` in the text view.
    pub fn mark_attribute(&mut self, element: NodeId, index: u32) {
        let spans = self.doc.index.attribute_spans(&self.doc.src, element);
        if let Some(span) = spans.get(index as usize) {
            self.mark = Some(span.start as u64..span.end as u64);
            self.scroll_text_to = Some(span.start as u64);
        }
    }

    /// Highlights the text that the index keeps inside `element` (D11), like a text node.
    pub fn mark_text(&mut self, element: NodeId) {
        if let Some(span) = self.doc.index.folded_span(&self.doc.src, element) {
            self.focus = Some(element);
            self.mark = Some(span.start as u64..span.end as u64);
            self.scroll_text_to = Some(span.start as u64);
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
pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || std::fs::canonicalize(a)
            .ok()
            .zip(std::fs::canonicalize(b).ok())
            .is_some_and(|(a, b)| a == b)
}

/// What happened to the file, after its name.
/// The reference rules for a document at `path` (REF-1): the rules file next to it or above
/// it, if there is one.
fn load_rules(path: &Path) -> Option<Result<(PathBuf, Arc<Rules>), String>> {
    let file = Rules::find(path)?;
    Some(match Rules::read(&file) {
        Ok(rules) => {
            log::info!("reference rules from {}", file.display());
            Ok((file, Arc::new(rules)))
        }
        Err(e) => {
            log::warn!("reference rules in {}: {e}", file.display());
            Err(format!("{}: {e}", file.display()))
        }
    })
}

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

/// Scroll areas stop at the edge instead of springing back from past it.
///
/// The tree and the grid scroll past the end when they show one of the last rows, and egui
/// springs back with an `f32` offset. Past 2^23 points (the tree of a 300 MiB file) an `f32`
/// moves in whole points, so the spring never gets closer than one point to the edge and
/// asks for a new frame forever: a core stays busy (docs/benchmarks.md, Windows).
fn stop_at_the_edge(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| style.spacing.scroll.kinetic.rubber_band = false);
}

/// Changes show at once, without egui's animations.
///
/// The grid's scroll areas belong to the element shown, so every new selection starts with
/// hidden scroll bars that egui faded in over about 15 frames: 220 ms of frames after every
/// tree key, click or Page Down, at any file size (docs/benchmarks.md, Windows). Expanding
/// sections and menus open at once too.
fn no_animations(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
}

/// A toolbar button: `icon`, with `tip` when the pointer rests on it, also while disabled.
#[cfg(test)]
mod tests {
    use super::*;

    /// A minified file opens wrapped in its own tab, with the notice (TEXT-23); after Format
    /// both go, and the tab wraps as the setting says. Other files follow the setting.
    #[test]
    fn minified_file_opens_wrapped_until_formatted() {
        let items = "<a x=\"ü\">Grüße</a>".repeat(400);
        let src = format!("\u{feff}<r>{items}</r>\r\n");
        let mut open = Open::new(
            Document::from_bytes(src.into_bytes()),
            egui::Id::unique("min"),
        );
        assert!(open.minified_notice);
        assert!(open.wraps(false));
        let out = open.doc.format(&FormatOptions::default()).unwrap();
        open.edit(|doc| {
            doc.apply(Edit::whole("Format", doc.src.len(), out))
                .map(|()| true)
        })
        .unwrap();
        open.formatted();
        assert!(!open.minified_notice);
        assert!(!open.wraps(false));
        assert!(open.wraps(true));
        // The byte order mark and CRLF are kept by Format.
        assert!(open.doc.src.starts_with("\u{feff}<r>\r\n".as_bytes()));

        let plain = Open::new(
            Document::from_bytes(b"<r>\n<a/>\n</r>\n".to_vec()),
            egui::Id::unique("plain"),
        );
        assert!(!plain.minified_notice);
        assert!(!plain.wraps(false));
    }

    /// A scroll bar that appears, as on every new selection in the grid, is there at once and
    /// asks for no further frames.
    #[test]
    fn scroll_bars_appear_without_animation() {
        let ctx = egui::Context::default();
        no_animations(&ctx);
        // (shown, delay before the next frame) for a bar that should be `show`n.
        let frame = |show: bool| {
            let mut shown = 0.0;
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                shown = ui
                    .ctx()
                    .animate_bool_responsive(ui.scope_id().with("bar"), show);
            });
            // No renderer here takes the font atlas.
            out.textures_delta.clear();
            (
                shown,
                out.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
            )
        };
        // Hidden first, as in a new grid; a new context also asks for a second frame.
        frame(false);
        frame(false);
        let (shown, delay) = frame(true);
        assert_eq!(shown, 1.0);
        assert!(!delay.is_zero(), "asked for another frame");
    }

    /// A list as tall as the tree of a 300 MiB file (633,000 orders), scrolled past its end
    /// as revealing one of the last rows does, stops at the end in the same frame. Left past
    /// it, egui's spring back may never arrive (see [`stop_at_the_edge`]); whether it does
    /// depends on rounding, so the test checks for the overscroll itself.
    #[test]
    fn scrolling_past_the_end_of_a_tall_list_stops_at_the_end() {
        let ctx = egui::Context::default();
        stop_at_the_edge(&ctx);
        let rows = 633_000;
        let row_h = 21.0;
        // (offset, largest valid offset) after a frame.
        let frame = |offset: Option<f32>| {
            let mut result = (0.0, 0.0);
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                // Rows touch, as in the tree.
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut area = egui::ScrollArea::vertical().auto_shrink(false);
                if let Some(o) = offset {
                    area = area.vertical_scroll_offset(o);
                }
                let shown = area.show_rows(ui, row_h, rows, |ui, range| {
                    for _ in range {
                        ui.allocate_exact_size(egui::vec2(100.0, row_h), egui::Sense::hover());
                    }
                });
                let max = shown.content_size.y - shown.inner_rect.height();
                result = (shown.state.unclamped_offset().y, max);
            });
            // No renderer here takes the font atlas.
            out.textures_delta.clear();
            result
        };
        // The first frame learns the size; the second scrolls past the end.
        frame(None);
        let (offset, max) = frame(Some((rows - 3) as f32 * row_h));
        assert!(
            max > 2f32.powi(23),
            "not tall enough to need whole points: {max}"
        );
        assert!(
            offset <= max,
            "scrolled {} points past the end",
            offset - max
        );
    }

    /// The cell pane beside the grid (GRID-16) showing an element as deep as those in AUTOSAR
    /// files fits the width it is given. An egui side panel is never narrower than its
    /// content: the pane's path line made it as wide as the path, the grid beside it was
    /// squeezed to a few pixels, and the border could not be dragged back. Reported with an
    /// `.arxml` file.
    #[test]
    fn a_deep_path_fits_the_cell_pane() {
        let names: Vec<String> = (0..14).map(|i| format!("ÄR-PACKAGE-ELEMENT-{i}")).collect();
        let mut src = String::from("\u{feff}<?xml version=\"1.0\"?>\r\n");
        for n in &names {
            src.push_str(&format!("<{n}>\r\n"));
        }
        src.push_str("<FIBEX-ELEMENT-REF DEST=\"CAN-FRAME\">/Frame/Grüße</FIBEX-ELEMENT-REF>\r\n");
        for n in names.iter().rev() {
            src.push_str(&format!("</{n}>\r\n"));
        }
        let doc = Document::from_bytes(src.into_bytes());
        let deepest = (0..doc.index.len() as NodeId)
            .rev()
            .find(|&id| doc.index.node(id).kind == NodeKind::Element)
            .unwrap();
        let mut open = Open::new(doc, egui::Id::unique("deep"));
        // A leaf is the grid's one row (GRID-5); marked, the pane shows it with its path.
        open.select(deepest);
        assert!(open.grid.as_ref().unwrap().path.len() > 300);

        let ctx = egui::Context::default();
        let width = 300.0;
        let mut used = 0.0;
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.allocate_ui(egui::vec2(width, 400.0), |ui| {
                    crate::cell_pane::show(ui, &mut open);
                    used = ui.min_rect().width();
                });
            });
            // No renderer here takes the font atlas.
            out.textures_delta.clear();
        }
        assert!(
            open.cell_pane.is_some(),
            "the pane did not show the marked row"
        );
        assert!(
            used <= width,
            "the pane's content needs {used} points of the {width} it has"
        );
    }

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
