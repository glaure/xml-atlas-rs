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
use crate::installer::{self, Installer};
use crate::keys;
use crate::lang::Lang;
use crate::settings::{Layout, LightStyle, Settings, ThemeChoice};
use crate::sizes::Split;
use crate::tabs::Tabs;
use crate::tool_icons::{self, Tool};
use crate::update::{self, Notice};
use crate::{problems_panel, tree};
use xml_atlas_engine::grid::Grid;
use xml_atlas_engine::problems::{Problem, Severity};
use xml_atlas_engine::references::{self, Rules};
use xml_atlas_engine::rules_folder::Library;

mod menus;

/// The rules a tab uses, with their file, or that file and why it cannot be used.
pub type TabRules = Result<(PathBuf, Arc<Rules>), (PathBuf, String)>;

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
    /// The system's language, such as `de-DE`, as read at the start (LANG-7).
    system_language: Option<String>,
    /// Follows the window's size for the settings (WIN-3).
    window: crate::sizes::Tracker,
    /// The panes as last shown, to notice Reset layout whenever it comes.
    shown_panes: crate::sizes::Panes,
    /// X11 ignores a maximize asked for before the window shows; it is asked again then.
    maximize: Option<u8>,
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
    /// Alt tapped on its own opens the menu bar for the keyboard (#167).
    menu_keys: crate::menu_keys::MenuKeys,
    find: crate::find_bar::FindBar,
    /// Tools > XPath… (D35).
    xpath: crate::xpath_bar::XPathBar,
    goto: crate::goto_bar::GoToBar,
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
    /// The rules files in the configuration folder (REF-13).
    rules_files: crate::rules_files::RulesFiles,
    /// The update check under way (PKG-7, D30).
    updates: update::Checker,
    /// What the last update check found, in the status bar until it is closed.
    update_notice: Option<Notice>,
    /// The newer version on its way to the system's installer (PKG-8, D43).
    installer: Installer,
    /// Agents' calls, while Settings > Allow AI agents (MCP) is on (D28).
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
    /// The window's language, for what a command says in the status bar; the app sets it
    /// every frame.
    pub lang: Lang,
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
    /// The cells of the grid's rows on screen, kept from frame to frame (GRID-9).
    pub grid_cells: xml_atlas_engine::grid_cells::Cells,
    /// Where the text view jumps on the next frame: a byte of the source. Its line, or with
    /// word wrap its row, goes near the top, and a long line scrolls sideways to it.
    pub scroll_text_to: Option<u64>,
    /// Warnings and errors, in document order (OPEN-13), with the references that find
    /// nothing (REF-4).
    pub problems: Vec<Problem>,
    /// References are resolved in this tab (REF-12), as the setting says.
    resolve_references: bool,
    /// The rules files and the path the rules below were taken for, to see a change.
    references_from: Option<(Arc<Library>, Option<PathBuf>)>,
    /// The reference rules for this file (REF-13), or why they could not be read; `None`
    /// while references are off or no file is for it.
    pub references: Option<TabRules>,
    /// The text of their file, and its check for the preview's header (REF-16).
    pub rules_text: Option<Arc<str>>,
    pub rules_status: crate::rules_status::RulesStatus,
    /// The preview asked to edit this rules file; the app takes it.
    pub edit_rules: Option<PathBuf>,
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
    /// A click in the text expands the tree to the clicked element (TREE-12); from the
    /// settings, every frame.
    pub text_click_expands: bool,
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

/// A structured edit, as a refusal names it.
#[derive(Clone, Copy, Debug)]
pub enum What {
    EditCell,
    RemoveAttribute,
    DeleteText,
    Delete,
    Duplicate,
    Move,
    Rename,
}

impl What {
    /// "Cannot rename", before the reason.
    fn cannot(self, lang: Lang) -> &'static str {
        match self {
            What::EditCell => lang.status_cannot_edit_cell(),
            What::RemoveAttribute => lang.status_cannot_remove_attribute(),
            What::DeleteText => lang.status_cannot_delete_text(),
            What::Delete => lang.status_cannot_delete(),
            What::Duplicate => lang.status_cannot_duplicate(),
            What::Move => lang.status_cannot_move(),
            What::Rename => lang.status_cannot_rename(),
        }
    }
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
            system_language: sys_locale::get_locale(),
            docs: Tabs::default(),
            next_id: 0,
            title: String::new(),
            window: crate::sizes::Tracker::new(std::time::Instant::now()),
            shown_panes: settings.panes,
            maximize: settings.window.maximized.then_some(0),
            settings: settings.clone(),
            saved: settings,
            desktop_theme: None,
            desktop_theme_rx: crate::system_theme::watch(cc.egui_ctx.clone()),
            applied_theme: None,
            applied_light: None,
            confirm: None,
            quitting: false,
            find,
            menu_keys: Default::default(),
            xpath: Default::default(),
            goto: Default::default(),
            loading: Vec::new(),
            opened,
            help: Default::default(),
            font_dialog: None,
            settings_window: Default::default(),
            rules_files: crate::rules_files::RulesFiles::new(
                crate::settings::path().as_deref().and_then(Path::parent),
            ),
            updates: Default::default(),
            update_notice: None,
            installer: Default::default(),
            agent: None,
            message: None,
            notice: None,
            format_dialog: None,
            unencodable: None,
            disk_watch: DiskWatch::start(cc.egui_ctx.clone()),
        };
        if let Err(e) = crate::fonts::apply(&cc.egui_ctx, &mut app.settings.text_font) {
            // Logs are in English (LANG-3).
            log::warn!("{}; the text view uses the built-in font", e.text(Lang::En));
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
            let note = self.lang().status_open_already(&file_name(&path));
            self.notice = Some(Note::info(note));
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
                Err(TryRecvError::Disconnected) => {
                    Err(std::io::Error::other(self.lang().status_loading_stopped()))
                }
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
            .chain(self.rules_files.watched().iter().cloned())
            .collect();
        let new_list = self.disk_watch.watch(paths);
        if !(self.disk_watch.take_changed() || new_list) {
            return;
        }
        self.rules_files.refresh();
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
        let lang = self.lang();
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
                self.notice = Some(Note::info(lang.status_reloaded(&file_name(path))));
            }
            Err(e) => {
                let path = path.display().to_string();
                self.message = Some(lang.status_cannot_reload(&path, &e.to_string()));
            }
        }
    }

    fn loaded(&mut self, path: PathBuf, result: std::io::Result<Document>) {
        let lang = self.lang();
        let cannot = |e: &std::io::Error| {
            lang.status_cannot_open(&path.display().to_string(), &e.to_string())
        };
        match result {
            Ok(doc) => {
                self.add_document(doc);
                self.settings.add_recent(&path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // A file that is gone leaves the recent list (OPEN-12).
                let listed = self.settings.recent.len();
                self.settings.recent.retain(|p| p != &path);
                let message = cannot(&e);
                self.message = Some(if self.settings.recent.len() < listed {
                    lang.status_removed_recent(&message)
                } else {
                    message
                });
            }
            Err(e) => self.message = Some(cannot(&e)),
        }
    }

    /// Shows `doc` in a new tab.
    fn add_document(&mut self, doc: Document) {
        let rules = self.rules_in_use();
        crate::agent::add_tab(
            &mut self.docs,
            &mut self.next_id,
            self.settings.tree_attributes,
            rules.as_ref(),
            doc,
        );
    }

    /// The rules files while references are on (REF-12).
    fn rules_in_use(&self) -> Option<Arc<Library>> {
        self.settings
            .references
            .then(|| self.rules_files.library().cloned().unwrap_or_default())
    }

    /// Lets agents in while Settings > Allow AI agents (MCP) is on, and runs their calls on the
    /// tabs (D28).
    fn serve_agents(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        if self.settings.agents != self.agent.is_some() {
            self.agent = None;
            if self.settings.agents {
                let started = xml_atlas_mcp::bridge::endpoint_file()
                    .ok_or_else(|| lang.status_no_settings_folder().to_string())
                    .and_then(|file| {
                        let ctx = ctx.clone();
                        xml_atlas_mcp::bridge::Listener::start(file, move || ctx.request_repaint())
                            .map_err(|e| e.to_string())
                    });
                match started {
                    Ok(listener) => self.agent = Some(listener),
                    Err(e) => {
                        self.message = Some(lang.status_cannot_agents(&e));
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
                references: self
                    .settings
                    .references
                    .then(|| self.rules_files.library().cloned().unwrap_or_default()),
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
    /// Notes the window's size for the settings (WIN-3).
    fn follow_window(&mut self, ctx: &egui::Context) {
        if let Some(frames) = self.maximize {
            // The window shows after the first frame.
            if frames == 0 {
                self.maximize = Some(1);
                ctx.request_repaint();
            } else {
                self.maximize = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
        }
        let now = std::time::Instant::now();
        let seen = crate::sizes::Seen::now(ctx);
        if let Some(wait) = self.window.observe(seen, &mut self.settings.window, now) {
            ctx.request_repaint_after(wait);
        }
    }

    /// The language the window's texts are in (LANG-6).
    fn lang(&self) -> Lang {
        self.settings
            .language
            .language(self.system_language.as_deref())
    }

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
                // The rules editor asks about its changes first (REF-15).
                if !self.rules_files.may_quit() {
                    return;
                }
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
        let lang = self.lang();
        let name = self
            .docs
            .active()
            .and_then(|o| o.doc.path.as_deref())
            .map(file_name)
            .unwrap_or_else(|| lang.unsaved_the_document().into());
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("save-changes")).show(&ctx, |ui| {
            ui.set_width(360.0);
            ui.heading(lang.unsaved_title());
            ui.label(lang.unsaved_question(&name));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(lang.unsaved_save()).clicked() {
                    answer = Some(Answer::Save);
                }
                if ui.button(lang.unsaved_discard()).clicked() {
                    answer = Some(Answer::Discard);
                }
                if ui.button(lang.dialog_cancel()).clicked() {
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
        let lang = self.lang();
        let text = match &error {
            Unencodable::Char {
                offset,
                ch,
                encoding,
            } => {
                let line = open.doc.line_of(*offset as u64);
                let column = xml_atlas_engine::text::column(&open.doc, *offset) + 1;
                lang.encoding_char(
                    &ch.to_string(),
                    &(line + 1).to_string(),
                    &column.to_string(),
                    encoding,
                )
            }
            Unencodable::Lossy { encoding } => lang.encoding_lossy(encoding, "\u{FFFD}"),
        };
        let (mut show, mut utf8, mut cancel) = (false, false, false);
        let modal = egui::Modal::new(egui::Id::unique("unencodable")).show(&ctx, |ui| {
            ui.set_width(420.0);
            ui.heading(lang.encoding_title());
            ui.label(text);
            ui.label(lang.encoding_note());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                utf8 = ui.button(lang.encoding_save_utf8()).clicked();
                if matches!(error, Unencodable::Char { .. }) {
                    show = ui.button(lang.encoding_show()).clicked();
                }
                cancel = ui.button(lang.dialog_cancel()).clicked();
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
            let message = self.lang().status_cannot_utf8(&e.to_string());
            self.message = Some(message);
            return;
        }
        let path = path.or_else(|| open.doc.path.clone());
        match path {
            Some(p) => {
                if self.save_to(p) {
                    let note = self.lang().status_saved_utf8();
                    self.notice = Some(Note::info(note));
                }
            }
            None => {
                self.save();
            }
        }
    }

    /// Quitting stopped: a later quit asks about every modified document again.
    fn cancel_quit(&mut self) {
        self.installer.quit_cancelled();
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
        let lang = self.lang();
        let mut show = None;
        let mut close = None;
        egui::ScrollArea::horizontal()
            .id_salt("tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, open) in self.docs.iter().enumerate() {
                        let path = open.doc.path.as_deref();
                        let mut name = path
                            .map(file_name)
                            .unwrap_or_else(|| lang.tabs_untitled().into());
                        if open.doc.is_modified() {
                            name.push_str(" *");
                        }
                        let active = i == self.docs.active_index();
                        let mut hover = path.map(|p| p.display().to_string()).unwrap_or_default();
                        let name = match open.disk {
                            Some(change) => {
                                hover.push_str(&format!("\n{}", disk_text(lang, change)));
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
                            .on_hover_text(keys::tip(
                                ui.ctx(),
                                lang,
                                lang.tool_close_tab(),
                                &keys::CLOSE,
                            ));
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
    /// The next tab, or the previous one.
    fn switch_tab(&mut self, back: bool) {
        self.docs.cycle(back);
        self.notice = None;
    }

    fn tab_shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, key)))
        };
        // Back first: Ctrl+Tab would match Ctrl+Shift+Tab too.
        if keys::consume(ctx, &keys::CLOSE) {
            self.close_tab(ctx);
        } else if keys::consume(ctx, &keys::PREVIOUS_TAB) || pressed(Key::PageUp) {
            self.switch_tab(true);
        } else if keys::consume(ctx, &keys::NEXT_TAB) || pressed(Key::PageDown) {
            self.switch_tab(false);
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
            .add_filter(self.lang().files_all(), &["*"]);
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
        let lang = self.lang();
        self.replace_with(
            "Format",
            open.doc.format(&chosen.options),
            |options| {
                let done = lang.status_formatted(&options.to_string());
                if chosen.from_document.is_empty() {
                    done
                } else {
                    lang.status_from_comment(&done, &chosen.from_document.join(", "))
                }
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

    /// Tools > Compact (FMT-14): the document on one line.
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
        let lang = self.lang();
        self.replace_with(
            "Compact",
            result,
            |o| {
                let mut s = lang.status_compacted().to_string();
                if o.remove_comments {
                    s = lang.status_comments_removed(&s);
                }
                if o.trim_text {
                    s = lang.status_text_trimmed(&s);
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
        let lang = self.lang();
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
                    Err(e) => self.message = Some(lang.status_cannot_step(label, &e.to_string())),
                }
            }
            Err(e) => self.message = Some(lang.status_cannot_format(&e.to_string())),
        }
    }

    /// Edit > Undo.
    fn undo(&mut self) {
        let lang = self.lang();
        let Some(open) = self.docs.active_mut() else {
            return;
        };
        let mut label = None;
        if let Ok(true) = open.edit(|doc| {
            label = doc.undo();
            Ok(label.is_some())
        }) {
            open.cursor_to_change();
            self.notice = label.map(|l| Note::info(lang.status_undid(&l)));
        }
    }

    /// Edit > Redo.
    fn redo(&mut self) {
        let lang = self.lang();
        let Some(open) = self.docs.active_mut() else {
            return;
        };
        let mut label = None;
        if let Ok(true) = open.edit(|doc| {
            label = doc.redo();
            Ok(label.is_some())
        }) {
            open.cursor_to_change();
            self.notice = label.map(|l| Note::info(lang.status_redid(&l)));
        }
    }

    /// Tools > Format options: indent and attribute wrapping for this session.
    fn format_dialog(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
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
            ui.heading(lang.format_title());
            ui.add_space(6.0);
            crate::settings_window::format_options(ui, lang, &mut edit);
            if !overrides.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(lang.format_overrides(&overrides.join(", ")))
                        .color(ui.visuals().warn_fg_color),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(lang.dialog_ok()).clicked() {
                    close = Some(true);
                }
                if ui.button(lang.dialog_cancel()).clicked() {
                    close = Some(false);
                }
                if ui.button(lang.format_defaults()).clicked() {
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
                let path = path.display().to_string();
                let message = self.lang().status_cannot_save(&path, &e.to_string());
                self.message = Some(message);
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
        let lang = self.lang();
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("⚠ {name} {}.", disk_text(lang, change)),
            );
            if change == DiskChange::Changed {
                let (label, tip) = if modified {
                    (lang.disk_reload_discard(), lang.disk_reload_discard_tip())
                } else {
                    (lang.disk_reload(), lang.disk_reload_tip())
                };
                if ui.button(label).on_hover_text(tip).clicked() {
                    self.reload(&ctx);
                }
            }
            if ui
                .button(lang.disk_save())
                .on_hover_text(lang.disk_save_tip())
                .clicked()
            {
                self.save();
            }
            if ui.button(lang.disk_save_as()).clicked() {
                self.save_as();
            }
            if ui
                .button(lang.disk_keep())
                .on_hover_text(lang.disk_keep_tip())
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
        let lang = self.lang();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("ℹ {}", lang.minified_notice(&name)));
            if !broken
                && ui
                    .button(lang.minified_format())
                    .on_hover_text(lang.minified_format_tip())
                    .clicked()
            {
                self.format_document();
            }
            if ui.button(lang.minified_dismiss()).clicked()
                && let Some(open) = self.docs.active_mut()
            {
                open.minified_notice = false;
            }
        });
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
    /// The rules editor (REF-15), before the keys that act on the document.
    fn rules_editor(&mut self, ctx: &egui::Context) {
        let asked = self.docs.active_mut().and_then(|o| o.edit_rules.take());
        if let Some(path) = asked {
            self.rules_files.edit(&path);
        }
        self.rules_files
            .show_editor(ctx, self.lang(), self.docs.active());
        if self.rules_files.take_quit() {
            self.request(ctx, Action::Quit);
        } else if self.rules_files.take_quit_cancelled() {
            self.cancel_quit();
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        if !self.settings_window.open {
            return;
        }
        let before = self.settings.clone();
        let rules = crate::settings_window::RulesList {
            files: &mut self.rules_files,
            document: self.docs.active().and_then(|o| o.doc.path.as_deref()),
        };
        let request =
            self.settings_window
                .show(ctx, &mut self.settings, rules, self.updates.running(), lang);
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
                    let lang = self.lang();
                    self.message = Some(lang.status_cannot_font(&e.text(lang)));
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
        if let Some(page) = self.installer.poll() {
            ctx.open_url(egui::OpenUrl::new_tab(page));
        }
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
                self.installer.offer(match &notice {
                    Some(Notice::Newer(r, update::Install::Download)) => Some(r),
                    _ => None,
                });
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

    /// Help > Check for updates now, *Check now* in Settings and About's button: asks at once, also when the
    /// daily check is off, and says what it found.
    fn check_for_updates_now(&mut self, ctx: &egui::Context) {
        self.settings.update_last = Some(update::now());
        self.update_notice = None;
        self.updates.start(ctx.clone(), true);
    }

    /// View > Font… (TEXT-25): the text view takes the font chosen with OK.
    fn font_dialog(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        let Some(dialog) = &mut self.font_dialog else {
            return;
        };
        match dialog.show(ctx, lang) {
            None => {}
            Some(crate::fonts::Answer::Cancel) => self.font_dialog = None,
            Some(crate::fonts::Answer::Use(mut font)) => {
                self.font_dialog = None;
                match crate::fonts::apply(ctx, &mut font) {
                    Ok(()) => self.settings.text_font = font,
                    Err(e) => {
                        self.message = Some(lang.status_cannot_font(&e.text(lang)));
                    }
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
        let lang = self.lang();
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
            if tool_icons::button(
                ui,
                true,
                Tool::New,
                &keys::tip(&ctx, lang, lang.tool_new(), &keys::NEW),
            ) {
                self.new_document();
            }
            if tool_icons::button(
                ui,
                true,
                Tool::Open,
                &keys::tip(&ctx, lang, lang.tool_open(), &keys::OPEN),
            ) {
                self.pick_file(&ctx);
            }
            if tool_icons::button(
                ui,
                modified,
                Tool::Save,
                &keys::tip(&ctx, lang, lang.tool_save(), &keys::SAVE),
            ) {
                self.save();
            }
            ui.separator();
            if tool_icons::button(
                ui,
                selected,
                Tool::Cut,
                &keys::tip(&ctx, lang, lang.tool_cut(), &keys::CUT),
            ) {
                self.clipboard(&ctx, Some(egui::ViewportCommand::RequestCut));
            }
            if tool_icons::button(
                ui,
                selected,
                Tool::Copy,
                &keys::tip(&ctx, lang, lang.tool_copy(), &keys::COPY),
            ) {
                self.clipboard(&ctx, Some(egui::ViewportCommand::RequestCopy));
            }
            if tool_icons::button(
                ui,
                open,
                Tool::Paste,
                &keys::tip(&ctx, lang, lang.tool_paste(), &keys::PASTE),
            ) {
                self.clipboard(&ctx, Some(egui::ViewportCommand::RequestPaste));
            }
            ui.separator();
            if tool_icons::button(
                ui,
                undo,
                Tool::Undo,
                &keys::tip(&ctx, lang, lang.tool_undo(), &keys::UNDO),
            ) {
                self.undo();
            }
            if tool_icons::button(
                ui,
                redo,
                Tool::Redo,
                &keys::tip(&ctx, lang, lang.tool_redo(), &keys::REDO),
            ) {
                self.redo();
            }
            if tool_icons::button(
                ui,
                open,
                Tool::Find,
                &keys::tip(&ctx, lang, lang.tool_find(), &crate::find_bar::OPEN),
            ) {
                self.find.open(self.docs.active_mut());
            }
            if tool_icons::button(
                ui,
                open,
                Tool::XPath,
                &keys::tip(&ctx, lang, lang.tool_xpath(), &crate::xpath_bar::OPEN),
            ) {
                self.xpath.open();
            }
            ui.separator();
            if tool_icons::button(
                ui,
                open,
                Tool::DrillUp,
                &keys::tip(&ctx, lang, lang.tool_drill_up(), &keys::DRILL_UP),
            ) && let Some(open) = self.docs.active_mut()
            {
                crate::nav::drill_up(open);
            }
            if tool_icons::button(
                ui,
                open,
                Tool::DrillDown,
                &keys::tip(&ctx, lang, lang.tool_drill_down(), &keys::DRILL_DOWN),
            ) && let Some(open) = self.docs.active_mut()
            {
                crate::nav::drill_down(open);
            }
            if tool_icons::button(
                ui,
                open,
                Tool::Format,
                &keys::tip(&ctx, lang, lang.tool_format(), &keys::FORMAT),
            ) {
                self.format_document();
            }
            ui.separator();
            if tool_icons::button(
                ui,
                open,
                Tool::Diagram,
                &keys::tip(&ctx, lang, lang.tool_diagram(), &crate::diagram::TOGGLE),
            ) {
                self.settings.show_diagram(!self.settings.diagram);
            }
            let expand = self.settings.expand_on_text_click;
            let tip = if expand {
                lang.tool_expand_on()
            } else {
                lang.tool_expand_off()
            };
            if tool_icons::toggle(ui, true, Tool::FollowText, expand, tip) {
                self.settings.expand_on_text_click = !expand;
            }
        });
    }

    /// The status bar: the file and the last command on the left, the update notice (PKG-7)
    /// on the right.
    fn status_bar(&mut self, ui: &mut Ui) {
        if self.update_notice.is_none() && !self.installer.is_active() {
            self.status_text(ui);
            return;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            self.update_notice(ui);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                // It has what the notice leaves, and is cut off there.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                self.status_text(ui);
            });
        });
    }

    /// The update notice, right to left: its close button, the installer's, then its text.
    fn update_notice(&mut self, ui: &mut Ui) {
        let lang = self.lang();
        let close = ui
            .small_button("×")
            .on_hover_text(lang.update_close())
            .clicked();
        let request = self.installer.show(ui, self.update_notice.as_ref(), lang);
        if close {
            self.update_notice = None;
            self.installer.dismiss();
        } else if let Some(request) = request {
            self.install_request(ui.ctx(), request);
        }
    }

    /// A click on the installer's part of the notice (PKG-8).
    fn install_request(&mut self, ctx: &egui::Context, request: installer::Request) {
        match request {
            installer::Request::Start => self.installer.start(ctx),
            installer::Request::Cancel => self.installer.cancel(),
            installer::Request::CloseAndInstall => {
                self.installer.close_and_install();
                self.request(ctx, Action::Quit);
            }
        }
    }

    fn status_text(&mut self, ui: &mut Ui) {
        let lang = self.lang();
        ui.horizontal(|ui| {
            if let Some(l) = self.loading.first() {
                ui.spinner();
                let path = l.path.display().to_string();
                ui.label(if l.reload.is_some() {
                    lang.status_reloading(&path)
                } else {
                    lang.status_opening(&path)
                });
                return;
            }
            if let Some(m) = &self.message {
                ui.colored_label(ui.visuals().error_fg_color, m);
                return;
            }
            let Some(open) = self.docs.active_mut() else {
                let key = keys::text(ui.ctx(), lang, &keys::OPEN);
                ui.label(lang.status_empty(&key));
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
            let name = if open.doc.is_modified() {
                lang.status_modified(&name)
            } else {
                name
            };
            let size = lang.decimal(s.bytes as f64 / 1048576.0, 1);
            let time = lang.decimal(s.timings.total().as_secs_f64() * 1000.0, 0);
            ui.label(format!(
                "{name}{encoding}  ·  {size} MiB  ·  {}  ·  {}  ·  {}",
                lang.status_lines(s.lines),
                lang.status_elements(s.elements),
                lang.status_opened_in(&time),
            ));
            if self.agent.as_ref().is_some_and(|a| a.connected() > 0) {
                let connected = format!("  ·  {}", lang.status_agent_connected());
                ui.label(RichText::new(connected).strong());
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
                if ui
                    .link(text)
                    .on_hover_text(lang.status_go_to_error())
                    .clicked()
                {
                    problems_panel::go_to(open, i);
                }
            }
        });
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.window
            .finish(&mut self.settings.window, std::time::Instant::now());
        self.save_settings();
        // Last: it replaces this program (PKG-8).
        self.installer.on_exit(self.quitting);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let lang = self.lang();
        let ctx = ui.ctx().clone();
        self.poll_loading();
        for open in self.docs.iter_mut() {
            open.lang = lang;
        }
        self.check_disk();
        self.apply_theme(&ctx);
        self.check_for_updates(&ctx);
        self.follow_window(&ctx);
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
        if keys::consume(&ctx, &keys::OPEN) {
            self.pick_file(&ctx);
        }
        if keys::consume(&ctx, &keys::NEW) {
            self.new_document();
        }
        if ctx.input_mut(|i| {
            let pressed = i.consume_shortcut(&keys::WORD_WRAP);
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
            && (self.rules_files.has_unsaved() || self.docs.iter().any(|o| o.doc.is_modified()))
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
        if keys::consume(&ctx, &keys::HELP) {
            self.help.guide = true;
        }
        crate::help::show(&ctx, &mut self.help, lang);
        let checking = self.updates.running();
        if let Some(crate::about::Request::CheckForUpdates) =
            self.help.about.show(&ctx, lang, checking)
        {
            self.check_for_updates_now(&ctx);
        }
        self.font_dialog(&ctx);
        if keys::consume(&ctx, &keys::SETTINGS) {
            self.settings_window.open = true;
        }
        self.settings_window(&ctx);
        self.rules_editor(&ctx);
        // Ctrl+Shift+S first: Ctrl+S would match it too.
        if keys::consume(&ctx, &keys::SAVE_AS) {
            self.save_as();
        } else if keys::consume(&ctx, &keys::SAVE) {
            self.save();
        }
        // A focused text field (Find) has its own undo.
        if !ctx.text_edit_focused() && self.confirm.is_none() {
            // Redo first: Ctrl+Z would match Ctrl+Shift+Z too.
            if keys::consume(&ctx, &keys::REDO_ALSO) || keys::consume(&ctx, &keys::REDO) {
                self.redo();
            } else if keys::consume(&ctx, &keys::UNDO) {
                self.undo();
            } else if !self.docs.is_empty() && keys::consume(&ctx, &keys::FORMAT) {
                self.format_document();
            }
        }
        if self.confirm.is_none() && keys::consume(&ctx, &keys::QUIT) {
            self.request(&ctx, Action::Quit);
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
            self.settings.show_diagram(!self.settings.diagram);
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
        self.goto.shortcuts(&ctx, !self.docs.is_empty());
        if self.confirm.is_none()
            && let Some(open) = self.docs.active_mut()
        {
            // A hidden grid takes no keys: they go to the tree.
            let grid_shown = self.settings.grid;
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
        // After the menu and Settings, where it is switched, and before the views.
        let needed = self.settings.references
            || self.settings_window.shows_references()
            || self.rules_files.is_editing();
        self.rules_files.update(&ctx, needed);
        if ctx.input(|i| i.viewport().close_requested()) {
            self.rules_files.flush();
        }
        let library = self.rules_in_use();
        for open in self.docs.iter_mut() {
            open.set_references(library.as_ref());
        }

        let Some(open) = self.docs.active_mut() else {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    let key = keys::text(ui.ctx(), self.lang(), &keys::OPEN);
                    ui.heading(self.lang().welcome_drop(&key));
                });
            });
            return;
        };

        // The font chosen in View > Font… for tree, grid and cell pane too (TEXT-25).
        let view_font = self.settings.text_font.view_font_id();
        let theme = crate::theme::current(ui);
        // Reset layout, here or in Settings: what egui holds for this session goes too.
        crate::sizes::follow_reset(&ctx, &mut self.shown_panes, self.settings.panes);
        let frame = theme.view_frame(ui.style(), false);
        let panes = &mut self.settings.panes;
        panes.show(ui, Split::Tree, Some(frame), |ui, _| {
            let id = open.id;
            crate::fonts::use_in_views(ui, view_font.as_ref());
            ui.push_id(id, |ui| tree::show(ui, open, view_font.as_ref(), lang));
        });
        if std::mem::take(&mut open.find_requested) {
            self.find.open_within(open);
        }
        let layout = self.settings.layout;
        let format = self.settings.format;
        let cell_pane = self.settings.cell_pane;
        let reference_preview = self.settings.shows_preview();
        let preview_tree = self.settings.shows_preview_tree();
        let show_diagram = self.settings.diagram;
        let show_grid = self.settings.grid;
        let wrap = open.wraps(self.settings.word_wrap);
        let expand_on_click = self.settings.expand_on_text_click;
        let text_font = self.settings.text_font.font_id();
        egui::CentralPanel::default().show(ui, |ui| {
            let panes = &mut self.settings.panes;
            if show_diagram {
                // In the grid's place (D36), with a height of its own per layout.
                let split = match layout {
                    Layout::GridAbove => Split::DiagramAbove,
                    Layout::TextAbove => Split::DiagramBelow,
                };
                let frame = theme.view_frame(ui.style(), false);
                panes.show(ui, split, Some(frame), |ui, _| {
                    crate::fonts::use_in_views(ui, view_font.as_ref());
                    ui.push_id(open.id, |ui| crate::diagram::show(ui, open, lang));
                });
            }
            if show_grid {
                // Each layout has its own panel, so each remembers its own height.
                let split = match layout {
                    Layout::GridAbove => Split::GridAbove,
                    Layout::TextAbove => Split::GridBelow,
                };
                let frame = theme.view_frame(ui.style(), false);
                panes.show(ui, split, Some(frame), |ui, panes| {
                    crate::fonts::use_in_views(ui, view_font.as_ref());
                    ui.push_id(open.id, |ui| {
                        if cell_pane {
                            let frame = theme.view_frame(ui.style(), false);
                            panes.show(ui, Split::CellPane, Some(frame), |ui, _| {
                                ui.push_id("cell-pane", |ui| crate::cell_pane::show(ui, open));
                            });
                        }
                        egui::CentralPanel::default()
                            .frame(theme.view_frame(ui.style(), true))
                            .show(ui, |ui| crate::grid::show(ui, open, lang));
                    });
                });
            }
            if !open.problems.is_empty() {
                panes.show(ui, Split::Problems, None, |ui, _| {
                    ui.push_id(open.id, |ui| problems_panel::show(ui, open));
                });
            }
            if self.xpath.visible && crate::xpath_bar::has_results(open) {
                // The views' background, as tree and text have: a list to click in.
                let frame = theme.view_frame(ui.style(), false);
                panes.show(ui, Split::XPathResults, Some(frame), |ui, _| {
                    ui.push_id(open.id, |ui| crate::xpath_bar::show_results(ui, open));
                });
            }
            if self.find.visible {
                egui::Panel::top("find").show(ui, |ui| self.find.show(ui, open, lang));
            }
            if self.xpath.visible {
                egui::Panel::top("xpath").show(ui, |ui| self.xpath.show(ui, open, lang));
            }
            if self.goto.visible {
                egui::Panel::top("goto").show(ui, |ui| self.goto.show(ui, open, lang));
            }
            if preview_tree {
                // Added first, so it is the rightmost: main tree | text | preview | its tree.
                let frame = theme.view_frame(ui.style(), false);
                panes.show(ui, Split::PreviewTree, Some(frame), |ui, _| {
                    crate::fonts::use_in_views(ui, view_font.as_ref());
                    ui.push_id(open.id, |ui| {
                        crate::ref_preview::show_tree(ui, open, view_font.as_ref())
                    });
                });
            }
            if reference_preview {
                let frame = theme.view_frame(ui.style(), false);
                panes.show(ui, Split::Preview, Some(frame), |ui, _| {
                    ui.push_id(open.id, |ui| crate::ref_preview::show(ui, open, &text_font));
                });
            }
            egui::CentralPanel::default()
                .frame(theme.view_frame(ui.style(), true))
                .show(ui, |ui| {
                    ui.push_id(open.id, |ui| {
                        open.text_click_expands = expand_on_click;
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
            lang: Lang::En,
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
            grid_cells: Default::default(),
            scroll_text_to: None,
            problems: Vec::new(),
            resolve_references: false,
            references_from: None,
            references: None,
            rules_text: None,
            rules_status: Default::default(),
            edit_rules: None,
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
            text_click_expands: true,
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
        open.update_problems();
        open
    }

    /// Whether references are resolved in this tab (REF-12).
    pub fn resolves_references(&self) -> bool {
        self.resolve_references
    }

    /// Takes the rules for this tab from `library`; `None` while references are off
    /// (REF-12). Cheap when neither it nor the path changed. Off, what came from the rules
    /// goes: the problems, the preview and the sections chosen in it.
    pub fn set_references(&mut self, library: Option<&Arc<Library>>) {
        let same = match (library, &self.references_from) {
            (None, None) => true,
            (Some(new), Some((from, path))) => Arc::ptr_eq(new, from) && *path == self.doc.path,
            _ => false,
        };
        if same {
            return;
        }
        self.references_from = library.map(|l| (Arc::clone(l), self.doc.path.clone()));
        let entry = library.and_then(|l| l.for_document(self.doc.path.as_deref()));
        // Before the rules are compared: a misspelt key changes the text and not the rules.
        let text = entry.and_then(|entry| entry.text.clone());
        // Kept when it is the same: the header compares the pointer each frame.
        if text != self.rules_text {
            self.rules_text = text;
        }
        if self.rules_text.is_none() {
            self.rules_status = Default::default();
        }
        let rules = entry.map(|entry| match &entry.rules {
            Ok(rules) => Ok((entry.path.clone(), Arc::clone(rules))),
            Err(e) => Err((entry.path.clone(), e.clone())),
        });
        let on = library.is_some();
        if on == self.resolve_references && rules == self.references {
            return;
        }
        let file = |rules: &Option<TabRules>| match rules {
            Some(Ok((file, _)) | Err((file, _))) => Some(file.clone()),
            None => None,
        };
        // Sections chosen under one file's rules mean nothing under another's.
        if !on || file(&rules) != file(&self.references) {
            self.reference_choices.clear();
            self.expand_repeats = None;
        }
        self.resolve_references = on;
        self.references = rules;
        self.reference_preview = None;
        self.reference_sections = None;
        self.check_references();
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
        what: What,
        make: impl FnOnce(&Document) -> Result<Option<Edit>, CommandError>,
        shift: Shift,
    ) {
        // Typing may have left the index behind the text: read it first (D19).
        self.settle();
        let lang = self.lang;
        let refused = |why: &str| Note::refused(lang.status_refused(what.cannot(lang), why));
        if let Some(e) = &self.doc.index.error {
            self.note = Some(refused(&lang.status_not_well_formed(&e.to_string())));
            return;
        }
        let edit = match make(&self.doc) {
            Ok(Some(edit)) => edit,
            Ok(None) => return,
            Err(e) => {
                self.note = Some(refused(&e.to_string()));
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
                    Note::info(lang.status_nothing_to_change(&label))
                }
                Err(e) => refused(&e.to_string()),
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
        // A move replaces both elements in the text, so their marks are found again by
        // position (TREE-15); other changes move the marks with the text.
        let marked: Option<Vec<Vec<u32>>> = matches!(shift, Shift::Swap(..)).then(|| {
            self.doc
                .bookmarks
                .elements(idx)
                .filter_map(|id| shift.remap(&idx.element_position(id)))
                .collect()
        });
        let expanded: Vec<Vec<u32>> = self
            .tree
            .expanded_ids()
            .filter_map(|id| shift.remap(&idx.element_position(id)))
            .collect();
        if !change(&mut self.doc)? {
            return Ok(false);
        }
        if let Some(marked) = marked {
            let idx = &self.doc.index;
            self.doc.bookmarks.set(
                marked
                    .iter()
                    .filter_map(|p| idx.element_at_position(p))
                    .map(|id| idx.node(id).start as usize),
            );
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
        self.select_in(id, true);
    }

    /// As [`Self::select`]; with `expand` off the tree does not open up to `id` but selects the
    /// closest ancestor that is visible (TREE-12).
    pub fn select_in(&mut self, id: NodeId, expand: bool) {
        let id = self.element_or_parent(id);
        self.selected = Some(id);
        self.focus = Some(id);
        self.mark = None;
        self.grid = Some(Grid::start(&self.doc, id));
        self.grid_cell = None;
        self.grid_edit = None;
        if expand {
            self.tree.reveal(&self.doc, id);
        } else {
            self.tree.select_visible(&self.doc, id);
        }
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

    pub fn element_or_parent(&self, mut id: NodeId) -> NodeId {
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
fn disk_text(lang: Lang, change: DiskChange) -> &'static str {
    match change {
        DiskChange::Changed => lang.disk_changed(),
        DiskChange::Removed => lang.disk_removed(),
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

    /// TREE-12: a click in the text with expansion off leaves the tree closed and selects the
    /// closest element it shows; the grid still follows the clicked element. With expansion on
    /// the tree opens up to it. Non-ASCII text and CRLF, as real files have.
    #[test]
    fn a_text_click_expands_the_tree_only_when_asked() {
        let src = "<r>\r\n  <a>\r\n    <b>Grüße</b>\r\n  </a>\r\n</r>\r\n";
        let doc = Document::from_bytes(src.as_bytes().to_vec());
        let at = src.find("<b>").unwrap() as u64;
        let mut open = Open::new(doc, egui::Id::unique("text-click"));
        let b = open.element_at(at).unwrap();
        let a = open.doc.index.node(b).parent;
        open.tree.collapse_all(&open.doc);

        open.select_in(b, false);
        assert_eq!(open.selected, Some(b), "the grid follows the click");
        assert_eq!(
            open.tree.selected(),
            Some(xml_atlas_engine::tree::Row::Node(a))
        );
        assert!(!open.tree.is_expanded(a), "nothing was opened");

        open.select_in(b, true);
        assert_eq!(
            open.tree.selected(),
            Some(xml_atlas_engine::tree::Row::Node(b))
        );
        assert!(open.tree.is_expanded(a));
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

    /// A library with one file, on, for `use_for`.
    fn library_of(rules: &str, use_for: &str) -> Arc<Library> {
        Arc::new(Library {
            entries: vec![xml_atlas_engine::rules_folder::Entry {
                name: "rules.json".into(),
                path: "rules.json".into(),
                on: true,
                use_for: use_for.into(),
                rules: Rules::parse(rules).map(Arc::new).map_err(|e| e.to_string()),
                text: Some(rules.into()),
            }],
            error: None,
        })
    }

    /// REF-12: a file opens without its rules. Turned on, its references are checked and F12
    /// works; turned off, all that goes again, and typing does not bring it back.
    #[test]
    fn references_resolve_only_while_switched_on() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-ref-12-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let library = library_of(
            r#"{"includes": [{"element": "Include", "attribute": "src"}],
                "contexts": {"Shared": {"path": "/Doc/Shared"}}}"#,
            "größe.xml",
        );
        let src = "\u{feff}<Doc>\r\n<Shared><Größe/></Shared>\r\n\
                   <Include src=\"Shared:Größe\"/>\r\n<Include src=\"Shared:Fehlt\"/>\r\n</Doc>";
        let file = dir.join("größe.xml");
        std::fs::write(&file, src).unwrap();
        let mut open = Open::new(Document::open(&file).unwrap(), egui::Id::unique("ref-12"));
        let include = |open: &Open| {
            (0..open.doc.index.len() as NodeId)
                .find(|&id| {
                    open.doc.index.node(id).kind == NodeKind::Element
                        && open.doc.index.name(&open.doc.src, id) == b"Include"
                })
                .unwrap()
        };

        assert!(open.references.is_none() && open.problems.is_empty());
        let site = include(&open);
        open.select(site);
        crate::ref_preview::go_to_target(&mut open);
        let note = open.note.take().unwrap();
        assert!(note.refused && note.text.contains("References are off"));
        assert_eq!(open.selected, Some(site));

        open.set_references(Some(&library));
        assert!(open.rules().is_some());
        assert_eq!(open.problems.len(), 1);
        crate::ref_preview::go_to_target(&mut open);
        let target = open.selected.unwrap();
        assert_eq!(
            open.doc.index.path(&open.doc.src, target),
            "/Doc/Shared/Größe"
        );

        open.set_references(None);
        assert!(open.references.is_none() && open.problems.is_empty());
        assert!(open.reference_preview.is_none());
        let at = src.find("</Doc>").unwrap();
        open.type_edit(Edit::new("Type").replace(at..at, " "))
            .unwrap();
        assert!(open.references_due.is_none() && open.problems.is_empty());
        open.settle();
        assert!(open.problems.is_empty());

        // Changed rules are taken at once; for another document's, none are (REF-13).
        open.set_references(Some(&library));
        assert_eq!(open.problems.len(), 1);
        let changed = library_of(r#"{"includes": []}"#, "größe.xml");
        open.set_references(Some(&changed));
        assert!(open.rules().is_some_and(|r| r.is_empty()));
        assert!(open.problems.is_empty());
        open.set_references(Some(&library));
        assert_eq!(open.problems.len(), 1);
        open.set_references(Some(&library_of("{}", "other.xml")));
        assert!(open.references.is_none() && open.problems.is_empty());
        assert!(open.resolves_references());
        // A rules file beside the document is no longer used (D41).
        std::fs::write(dir.join("xml-atlas-references.json"), "{}").unwrap();
        open.set_references(Some(&Arc::default()));
        assert!(open.references.is_none());
        // Saved under another name, the rules follow it.
        open.set_references(Some(&library));
        open.doc.path = Some(dir.join("other.xml"));
        open.set_references(Some(&library));
        assert!(open.references.is_none() && open.problems.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
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

    /// OPEN-11, REF-15: quitting given up at the rules editor's question forgets "Don't save"
    /// for the documents, so the next quit asks again about what was typed since.
    #[test]
    fn cancel_at_the_rules_question_stops_the_whole_quit() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-quit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rules = dir.join("Größe.json");
        std::fs::write(&rules, "{}").unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = App::new(&cc, Vec::new(), None);
        app.new_document();
        app.docs
            .active_mut()
            .unwrap()
            .type_edit(Edit::new("Type").replace(0..0, " "))
            .unwrap();
        app.rules_files.edit(&rules);
        app.rules_files.set_editor_text("{ }");
        app.installer = Installer::ready_for_test();

        app.install_request(&ctx, installer::Request::CloseAndInstall);
        assert!(app.installer.will_install());
        assert!(app.confirm.take().is_some(), "asks about the document");
        // Don't save: on to the rules editor, which asks.
        app.docs.active_mut().unwrap().discard = true;
        app.request(&ctx, Action::Quit);
        assert!(!app.quitting);
        let escape = egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        // Cancel there.
        for events in [vec![], vec![], vec![escape], vec![]] {
            let input = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 700.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |_| app.rules_editor(&ctx));
            out.textures_delta.clear();
        }
        assert!(
            !app.installer.will_install(),
            "a later quit installs nothing (PKG-8)"
        );
        app.rules_files.set_editor_text("{}");
        assert!(!app.rules_files.has_unsaved());
        app.request(&ctx, Action::Quit);
        assert!(!app.quitting, "quit and lost the document's changes");
        assert!(app.confirm.is_some(), "asks about the document again");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// PKG-7, PKG-8: the status text gives way to the notice and its button, never under them.
    #[test]
    fn the_status_text_stays_clear_of_the_update_notice() {
        let offered = Installer::offered_for_test;
        let ready = Installer::ready_for_test;
        use crate::lang::LanguageChoice::{English, German};
        let installers = [Installer::default, offered, ready];
        for (language, installer) in installers.iter().flat_map(|i| [(English, i), (German, i)]) {
            for width in [700.0, 900.0, 1280.0] {
                let ctx = egui::Context::default();
                let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                let mut app = App::new(&cc, Vec::new(), None);
                app.settings = Settings {
                    language,
                    ..Default::default()
                };
                let mut doc = Document::untitled();
                let name = "Bestellungen-Größe-und-Lieferungen-2026.xml";
                doc.path = Some(std::env::temp_dir().join(name));
                app.add_document(doc);
                let release = update::Release {
                    version: update::Version(1, 4, 0),
                    url: update::RELEASES.into(),
                    files: Vec::new(),
                };
                app.update_notice = Some(Notice::Newer(release, update::Install::Download));
                app.installer = installer();
                let mut texts = Vec::new();
                // A panel has its size from the second frame on.
                for _ in 0..3 {
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 300.0),
                        )),
                        ..Default::default()
                    };
                    let mut out = ctx.run_ui(input, |ui| {
                        egui::Panel::bottom("status").show(ui, |ui| app.status_bar(ui));
                    });
                    out.textures_delta.clear();
                    texts = out
                        .shapes
                        .into_iter()
                        .filter_map(|s| match s.shape {
                            egui::Shape::Text(t) => Some(t.galley.rect.translate(t.pos.to_vec2())),
                            _ => None,
                        })
                        .collect();
                }
                texts.sort_by(|a, b| a.left().total_cmp(&b.left()));
                assert!(texts.len() >= 3, "{texts:?}");
                for pair in texts.windows(2) {
                    assert!(
                        pair[0].right() <= pair[1].left(),
                        "{language:?} {width}: {pair:?}"
                    );
                }
            }
        }
    }

    /// The status bar of an app set to German, as the texts it draws.
    fn german_status(app: &mut App, ctx: &egui::Context) -> Vec<String> {
        let mut texts = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 300.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                egui::Panel::bottom("status").show(ui, |ui| app.status_bar(ui));
            });
            out.textures_delta.clear();
            texts = out
                .shapes
                .into_iter()
                .filter_map(|s| match s.shape {
                    egui::Shape::Text(t) => {
                        let right = t.pos.x + t.galley.rect.right();
                        assert!(right <= 1280.0, "{:?} ends at {right}", t.galley.text());
                        Some(t.galley.text().to_owned())
                    }
                    _ => None,
                })
                .collect();
        }
        texts
    }

    /// LANG-1 and LANG-17: the status bar and what the commands put in it, in German, for a
    /// file with a byte order mark, CRLF line endings and non-ASCII text.
    #[test]
    fn the_status_bar_is_german() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = App::new(&cc, Vec::new(), None);
        app.settings = Settings {
            language: crate::lang::LanguageChoice::German,
            ..Default::default()
        };
        let shows = |texts: &[String], part: &str| texts.iter().any(|t| t.contains(part));
        let texts = german_status(&mut app, &ctx);
        assert!(shows(&texts, "oder auf das Fenster ziehen"), "{texts:?}");

        let src = "\u{feff}<r>\r\n  <a>Größe</a>\r\n</r>\r\n";
        app.add_document(Document::from_bytes(src.as_bytes().to_vec()));
        app.docs.active_mut().unwrap().lang = app.lang();
        app.format_document();
        let texts = german_status(&mut app, &ctx);
        for part in [
            "0,0 MiB",
            "Zeilen",
            "2 Elemente",
            "geöffnet in",
            "Formatiert: ",
        ] {
            assert!(shows(&texts, part), "{part}: {texts:?}");
        }

        app.docs
            .active_mut()
            .unwrap()
            .type_edit(Edit::new("Type").replace(3..3, " "))
            .unwrap();
        app.undo();
        app.redo();
        let texts = german_status(&mut app, &ctx);
        assert!(shows(&texts, "(geändert)"), "{texts:?}");
        assert!(shows(&texts, "Wiederholt: Type"), "{texts:?}");

        let gone = std::env::temp_dir().join("xml-atlas-nicht-da-Größe.xml");
        let error = std::io::Error::from(std::io::ErrorKind::NotFound);
        app.loaded(gone, Err(error));
        let texts = german_status(&mut app, &ctx);
        assert!(shows(&texts, "konnte nicht geöffnet werden: "), "{texts:?}");
    }

    /// What element commands and bookmarks say takes the tab's language.
    #[test]
    fn commands_report_in_the_tabs_language() {
        let doc = Document::from_bytes("\u{feff}<r>\r\n  <a>Größe</a>\r\n</r>".as_bytes().to_vec());
        let mut open = Open::new(doc, egui::Id::unique("german-notes"));
        open.lang = Lang::De;
        let note = |open: &mut Open| open.note.take().unwrap().text;
        let root = xml_atlas_engine::command::resolve(&open.doc, "/r").unwrap();
        open.select(root);
        crate::bookmarks::toggle(&mut open);
        assert_eq!(note(&mut open), "Lesezeichen gesetzt (1 insgesamt)");
        crate::bookmarks::clear(&mut open);
        assert_eq!(note(&mut open), "1 Lesezeichen entfernt");
        open.command(
            What::Rename,
            |d| xml_atlas_engine::command::rename(d, root, "a b").map(Some),
            Shift::None,
        );
        let text = note(&mut open);
        // The reason is the engine's, in English until its messages are translated.
        assert!(text.starts_with("Umbenennen nicht möglich: "), "{text}");
        let end = open.doc.src.len();
        open.type_edit(Edit::new("Type").replace(end - 4..end, ""))
            .unwrap();
        open.command(What::Delete, |_| Ok(None), Shift::None);
        let text = note(&mut open);
        assert!(
            text.starts_with("Löschen nicht möglich: das Dokument ist nicht wohlgeformt ("),
            "{text}"
        );
    }

    /// Draws what `show` draws until it has its size, in the program's own window size, and
    /// checks that nothing is cut off or lies over another text. Gives the texts. With
    /// `front`, the title of a dialog that lies over another window, only that dialog's.
    fn whole_in_the_window(
        app: &mut App,
        front: Option<&str>,
        show: fn(&mut App, &mut Ui),
    ) -> Vec<String> {
        let ctx = egui::Context::default();
        ctx.set_os(egui::os::OperatingSystem::Nix);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
        let mut texts = Vec::new();
        for _ in 0..6 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| show(app, ui));
            out.textures_delta.clear();
            texts = out
                .shapes
                .into_iter()
                .filter_map(|s| match s.shape {
                    egui::Shape::Text(t) => {
                        let rect = t.galley.rect.translate(t.pos.to_vec2());
                        Some((t.galley.text().to_owned(), rect, s.clip_rect))
                    }
                    _ => None,
                })
                .collect();
        }
        if let Some(title) = front {
            // Drawn last, so its texts come after all of the other window's.
            let first = texts.iter().position(|(t, ..)| t == title);
            texts.drain(..first.unwrap_or_else(|| panic!("no {title}: {texts:?}")));
        }
        for (i, (text, rect, clip)) in texts.iter().enumerate() {
            let shown = clip.intersect(screen).expand(1.0);
            // Sideways: the rules editor's text and check scroll downwards.
            assert!(
                shown.left() <= rect.left() && rect.right() <= shown.right(),
                "{text:?} is cut: {rect:?} in {shown:?}"
            );
            for (other, o, _) in &texts[i + 1..] {
                let shared = rect.intersect(*o);
                assert!(
                    shared.width() <= 1.0 || shared.height() <= 1.0,
                    "{text:?} lies over {other:?}"
                );
            }
        }
        texts.into_iter().map(|(text, ..)| text).collect()
    }

    /// LANG-1 and LANG-15: every dialog in German, each alone.
    #[test]
    fn the_dialogs_are_german_and_whole() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-dialogs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = App::new(&cc, Vec::new(), None);
        app.settings = Settings {
            language: crate::lang::LanguageChoice::German,
            ..Default::default()
        };
        let src = "\u{feff}<r>\r\n  <a>Größe €</a>\r\n</r>\r\n";
        let mut doc = Document::from_bytes(src.as_bytes().to_vec());
        doc.path = Some(dir.join("Bestellungen-Größe-und-Lieferungen-2026.xml"));
        app.add_document(doc);
        let check = |texts: Vec<String>, wanted: &[&str]| {
            for part in wanted {
                assert!(texts.iter().any(|t| t.contains(part)), "{part}: {texts:?}");
            }
        };

        app.confirm = Some(Action::Quit);
        check(
            whole_in_the_window(&mut app, None, |app, ui| app.confirm_dialog(ui)),
            &[
                "Nicht gespeicherte Änderungen",
                "Änderungen an Bestellungen-Größe",
                "Speichern",
                "Nicht speichern",
                "Abbrechen",
            ],
        );
        app.confirm = None;

        let error = Unencodable::Char {
            offset: src.find('€').unwrap(),
            ch: '€',
            encoding: "ISO-8859-1".into(),
        };
        app.unencodable = Some((dir.join("a.xml"), error));
        check(
            whole_in_the_window(&mut app, None, |app, ui| app.encoding_dialog(ui)),
            &[
                "Speichern in dieser Kodierung nicht möglich",
                "in Zeile 2",
                "Als UTF-8 speichern",
                "Anzeigen",
                "Abbrechen",
            ],
        );
        app.unencodable = Some((
            dir.join("a.xml"),
            Unencodable::Lossy {
                encoding: "UTF-16".into(),
            },
        ));
        check(
            whole_in_the_window(&mut app, None, |app, ui| app.encoding_dialog(ui)),
            &["ungültig sind"],
        );
        app.unencodable = None;

        app.format_dialog = Some(FormatOptions::default());
        check(
            whole_in_the_window(&mut app, None, |app, ui| {
                app.format_dialog(&ui.ctx().clone())
            }),
            &["Formatierungsoptionen", "OK", "Abbrechen", "Standardwerte"],
        );
        app.format_dialog = None;

        app.font_dialog = Some(crate::fonts::FontDialog::new(&app.settings.text_font));
        check(
            whole_in_the_window(&mut app, None, |app, ui| app.font_dialog(&ui.ctx().clone())),
            &[
                "Schriftart",
                "Hack (integriert)",
                "Fett kursiv",
                "Größe",
                "Vorschau",
                "Standard",
            ],
        );
        app.font_dialog = None;

        let rules = dir.join("Größe.json");
        std::fs::write(&rules, "{}").unwrap();
        app.rules_files.edit(&rules);
        app.rules_files.set_editor_text("{\"nein\": 1}");
        // Changed behind its back, so that the editor offers what to do.
        std::fs::write(&rules, "{ \"anders\": true }").unwrap();
        app.rules_files.refresh();
        check(
            whole_in_the_window(&mut app, None, |app, ui| {
                app.rules_editor(&ui.ctx().clone())
            }),
            &[
                "Regeln bearbeiten",
                "Speichern",
                "Zurücksetzen",
                "Einfügen",
                "Schließen",
                "Die Datei wurde auf dem Datenträger geändert.",
                "Überschreiben",
                "Neu laden und meine Änderungen verwerfen",
                "Fehler",
                "Zeile 1, Spalte",
            ],
        );
        assert!(!app.rules_files.may_quit());
        let title = "Nicht gespeicherte Regeln";
        check(
            whole_in_the_window(&mut app, Some(title), |app, ui| {
                app.rules_editor(&ui.ctx().clone())
            }),
            &[title, "Regeln speichern", "Nicht speichern", "Abbrechen"],
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// LANG-1 and LANG-15: the notices above the views, the tab of a new file, the grid with
    /// its attributes and cell pane, the problems list and the reference preview in German.
    #[test]
    fn the_panes_and_notices_are_german_and_whole() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = App::new(&cc, Vec::new(), None);
        app.settings = Settings {
            language: crate::lang::LanguageChoice::German,
            ..Default::default()
        };
        let check = |texts: Vec<String>, wanted: &[&str]| {
            for part in wanted {
                assert!(texts.iter().any(|t| t.contains(part)), "{part}: {texts:?}");
            }
        };

        app.new_document();
        check(
            whole_in_the_window(&mut app, None, |app, ui| app.tab_bar(ui)),
            &["unbenannt"],
        );

        // One long line, so the file counts as minified.
        let src = format!(
            "\u{feff}<r a=\"Größe\">{}</r>",
            "<o><n>ü</n></o>".repeat(400)
        );
        let mut doc = Document::from_bytes(src.into_bytes());
        doc.path = Some(std::env::temp_dir().join("Bestellungen-Größe-und-Lieferungen.xml"));
        app.add_document(doc);
        let open = app.docs.active_mut().unwrap();
        open.lang = Lang::De;
        open.disk = Some(DiskChange::Changed);
        check(
            whole_in_the_window(&mut app, None, |app, ui| app.minified_bar(ui)),
            &["ist minifiziert", "Formatieren", "Ausblenden"],
        );
        check(
            whole_in_the_window(&mut app, None, |app, ui| app.disk_bar(ui)),
            &[
                "wurde auf dem Datenträger geändert.",
                "Neu laden",
                "Speichern unter…",
                "Behalten",
            ],
        );
        app.docs.active_mut().unwrap().disk = Some(DiskChange::Removed);
        check(
            whole_in_the_window(&mut app, None, |app, ui| app.disk_bar(ui)),
            &["gelöscht oder verschoben."],
        );

        let open = app.docs.active_mut().unwrap();
        let root = xml_atlas_engine::command::resolve(&open.doc, "/r").unwrap();
        open.select(root);
        check(
            whole_in_the_window(&mut app, None, |app, ui| {
                ui.set_max_width(960.0);
                let open = app.docs.active_mut().unwrap();
                crate::grid::show(ui, open, Lang::De);
            }),
            &["400 Zeilen", "1 Attribut", "Element"],
        );
        check(
            whole_in_the_window(&mut app, None, |app, ui| {
                ui.set_max_width(320.0);
                crate::cell_pane::show(ui, app.docs.active_mut().unwrap());
            }),
            &["Eine Zeile oder Zelle"],
        );
        check(
            whole_in_the_window(&mut app, None, |app, ui| {
                ui.set_max_width(420.0);
                let font = egui::FontId::monospace(13.0);
                crate::ref_preview::show(ui, app.docs.active_mut().unwrap(), &font);
            }),
            &["Keine Verweisregeln für diese Datei", "Regeln legen fest"],
        );

        let mut broken = Document::from_bytes("<r>\r\n<Größe>\r\n</r>".as_bytes().to_vec());
        broken.path = None;
        app.add_document(broken);
        app.docs.active_mut().unwrap().lang = Lang::De;
        check(
            whole_in_the_window(&mut app, None, |app, ui| {
                ui.set_max_width(960.0);
                crate::problems_panel::show(ui, app.docs.active_mut().unwrap());
            }),
            &["Probleme", "Fehler, 0 Warnungen", "Zeile "],
        );
    }
}
