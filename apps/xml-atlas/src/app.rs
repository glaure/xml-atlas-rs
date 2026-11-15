//! Window layout, file loading and selection state.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use egui::{Key, KeyboardShortcut, Modifiers, RichText, ThemePreference, Ui};
use xml_atlas_engine::xml::{NodeId, NodeKind, ROOT};
use xml_atlas_engine::{Document, Edit, EditError, FormatError, FormatOptions};

use crate::settings::{Layout, Settings, ThemeChoice};
use crate::{problems_panel, tree};
use xml_atlas_engine::grid::Grid;
use xml_atlas_engine::problems::{Problem, Severity};

pub struct App {
    open: Option<Open>,
    /// Settings remembered between sessions (WIN-3), and what was last written to disk.
    settings: Settings,
    saved: Settings,
    /// The desktop's light or dark preference as it arrives (WIN-2); `None` until known.
    desktop_theme: Option<egui::Theme>,
    desktop_theme_rx: Option<Receiver<egui::Theme>>,
    /// The theme last handed to egui, so it is set only when it changes.
    applied_theme: Option<ThemePreference>,
    /// An action waiting for the answer to "save changes?" (OPEN-11).
    confirm: Option<Action>,
    /// Set once the user agreed to close, so the close request is not asked about again.
    quitting: bool,
    find: crate::find_bar::FindBar,
    loading: Option<Loading>,
    message: Option<String>,
    /// A note about the last command, shown in the status bar until the next one.
    notice: Option<String>,
    /// The Format options dialog, with the values being edited.
    format_dialog: Option<FormatOptions>,
}

/// The answers of the "save changes?" dialog.
enum Answer {
    Save,
    Discard,
    Cancel,
}

/// Something that replaces or closes the open document, and so may lose unsaved changes.
#[derive(Clone, Debug)]
enum Action {
    Open(PathBuf),
    PickFile,
    Quit,
}

struct Loading {
    path: PathBuf,
    rx: Receiver<std::io::Result<Document>>,
}

/// A document plus everything the views remember about it.
pub struct Open {
    pub doc: Document,
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
}

/// A view that takes arrow keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pane {
    #[default]
    Tree,
    Grid,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let settings = Settings::load();
        let mut find = crate::find_bar::FindBar::default();
        find.set_options(settings.find);
        let mut app = Self {
            open: None,
            settings: settings.clone(),
            saved: settings,
            desktop_theme: None,
            desktop_theme_rx: crate::system_theme::watch(cc.egui_ctx.clone()),
            applied_theme: None,
            confirm: None,
            quitting: false,
            find,
            loading: None,
            message: None,
            notice: None,
            format_dialog: None,
        };
        if let Some(path) = path {
            app.start_loading(&cc.egui_ctx, path);
        }
        app
    }

    fn start_loading(&mut self, ctx: &egui::Context, path: PathBuf) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        let p = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Document::open(&p));
            ctx.request_repaint();
        });
        self.loading = Some(Loading { path, rx });
        self.message = None;
        self.notice = None;
    }

    fn poll_loading(&mut self, ctx: &egui::Context) {
        let Some(loading) = &self.loading else { return };
        let Ok(result) = loading.rx.try_recv() else {
            return;
        };
        let path = loading.path.clone();
        self.loading = None;
        match result {
            Ok(doc) => {
                self.set_document(doc);
                self.settings.add_recent(&path);
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                    "{} — XML Atlas",
                    file_name(&path)
                )));
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

    fn set_document(&mut self, doc: Document) {
        let mut open = Open {
            tree: tree::TreeState::new(&doc),
            doc,
            selected: None,
            focus: None,
            mark: None,
            grid: None,
            scroll_text_to: None,
            problems: Vec::new(),
            keys: Pane::Tree,
            grid_scroll_to: None,
        };
        open.problems = open.doc.problems();
        open.tree.set_attributes(self.settings.tree_attributes);
        if let Some(root) = open.doc.index.root_element() {
            open.select(root);
        }
        self.open = Some(open);
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

    /// Runs `action`, or first asks whether to save when the document has unsaved changes.
    fn request(&mut self, ctx: &egui::Context, action: Action) {
        if self.open.as_ref().is_some_and(|o| o.doc.is_modified()) {
            self.confirm = Some(action);
        } else {
            self.perform(ctx, action);
        }
    }

    fn perform(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Open(path) => self.start_loading(ctx, path),
            Action::PickFile => self.pick_file(ctx),
            Action::Quit => {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// The "save changes?" dialog. Save runs the action only if saving worked.
    fn confirm_dialog(&mut self, ui: &mut Ui) {
        let Some(action) = self.confirm.clone() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let name = self
            .open
            .as_ref()
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
        match answer {
            Some(Answer::Save) => {
                self.confirm = None;
                if self.save() {
                    self.perform(&ctx, action);
                }
            }
            Some(Answer::Discard) => {
                self.confirm = None;
                self.perform(&ctx, action);
            }
            Some(Answer::Cancel) => self.confirm = None,
            None => {}
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
        if let Some(path) = dialog.pick_file() {
            self.start_loading(ctx, path);
        }
    }

    fn format_document(&mut self) {
        let Some(open) = &self.open else { return };
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
        let Some(open) = &self.open else { return };
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
        let Some(open) = &mut self.open else { return };
        match result {
            Ok(out) => {
                let edit = Edit::whole(label, open.doc.src.len(), out);
                match open.edit(|doc| doc.apply(edit).map(|()| true)) {
                    Ok(_) => self.notice = Some(notice(options)),
                    // Already formatted: the edit changes nothing.
                    Err(EditError::Empty) => self.notice = Some(notice(options)),
                    Err(e) => self.message = Some(format!("Cannot {label}: {e}")),
                }
            }
            Err(e) => self.message = Some(format!("Cannot format: {e}")),
        }
    }

    /// Edit > Undo.
    fn undo(&mut self) {
        let Some(open) = &mut self.open else { return };
        let mut label = None;
        if let Ok(true) = open.edit(|doc| {
            label = doc.undo();
            Ok(label.is_some())
        }) {
            self.notice = label.map(|l| format!("Undid {l}"));
        }
    }

    /// Edit > Redo.
    fn redo(&mut self) {
        let Some(open) = &mut self.open else { return };
        let mut label = None;
        if let Ok(true) = open.edit(|doc| {
            label = doc.redo();
            Ok(label.is_some())
        }) {
            self.notice = label.map(|l| format!("Redid {l}"));
        }
    }

    /// Edit > Format options: indent and attribute wrapping for this session.
    fn format_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut edit) = self.format_dialog else {
            return;
        };
        let overrides = self
            .open
            .as_ref()
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
        let Some(open) = &mut self.open else {
            return false;
        };
        let path = match open.doc.path.clone() {
            Some(p) => p,
            None => match rfd::FileDialog::new().save_file() {
                Some(p) => p,
                None => return false,
            },
        };
        match open.doc.save(&path) {
            Ok(()) => {
                self.settings.add_recent(&path);
                true
            }
            Err(e) => {
                self.message = Some(format!("Could not save {}: {e}", path.display()));
                false
            }
        }
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
            self.request(ctx, Action::Open(path));
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
                    self.request(&ctx, Action::PickFile);
                }
                self.recent_menu(ui, &ctx);
                let can_save = self.open.as_ref().is_some_and(|o| o.doc.is_modified());
                if ui
                    .add_enabled(can_save, egui::Button::new("Save     Ctrl+S"))
                    .clicked()
                {
                    ui.close();
                    self.save();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.close();
                    self.request(&ctx, Action::Quit);
                }
            });
            ui.menu_button("Edit", |ui| {
                let history = self.open.as_ref().map(|o| &o.doc.history);
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
                    .add_enabled(self.open.is_some(), egui::Button::new("Find…    Ctrl+F"))
                    .clicked()
                {
                    ui.close();
                    self.find.open();
                }
                if ui
                    .add_enabled(self.open.is_some(), egui::Button::new("Format"))
                    .clicked()
                {
                    ui.close();
                    self.format_document();
                }
                if ui
                    .add_enabled(self.open.is_some(), egui::Button::new("Compact"))
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
                ui.add_enabled_ui(self.open.is_some(), |ui| {
                    let mut show = self.open.as_ref().is_none_or(|o| o.tree.options.attributes);
                    if ui.checkbox(&mut show, "Attributes in the tree").changed()
                        && let Some(open) = &mut self.open
                    {
                        open.tree.set_attributes(show);
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
            if let Some(l) = &self.loading {
                ui.spinner();
                ui.label(format!("Opening {}…", l.path.display()));
                return;
            }
            if let Some(m) = &self.message {
                ui.colored_label(ui.visuals().error_fg_color, m);
                return;
            }
            let Some(open) = &mut self.open else {
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
                ui.label(RichText::new(format!("  ·  {n}")).weak());
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
        self.poll_loading(&ctx);
        self.apply_theme(&ctx);
        self.save_settings();

        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .find(|p| !p.as_os_str().is_empty())
        });
        if let Some(path) = dropped {
            self.request(&ctx, Action::Open(path));
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O)))
        {
            self.request(&ctx, Action::PickFile);
        }
        // The window's close button: hold the close until the user has answered.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quitting
            && self.open.as_ref().is_some_and(|o| o.doc.is_modified())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Action::Quit);
        }
        self.confirm_dialog(ui);
        self.format_dialog(&ctx);
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S)))
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

        self.find.shortcuts(&ctx, self.open.as_mut());
        if self.confirm.is_none()
            && let Some(open) = &mut self.open
        {
            crate::nav::handle(&ctx, open);
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        let Some(open) = &mut self.open else {
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
                tree::show(ui, open);
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
                    crate::grid::show(ui, open);
                });
            if !open.problems.is_empty() {
                egui::Panel::bottom("problems")
                    .resizable(true)
                    .show(ui, |ui| problems_panel::show(ui, open));
            }
            if self.find.visible {
                egui::Panel::top("find").show(ui, |ui| self.find.show(ui, open));
            }
            egui::CentralPanel::default().show(ui, |ui| {
                crate::text_view::show(ui, open);
            });
        });
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
        let idx = &self.doc.index;
        let selected = self.selected.map(|id| idx.element_position(id));
        let expanded: Vec<Vec<u32>> = self
            .tree
            .expanded_ids()
            .map(|id| idx.element_position(id))
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
            .and_then(|p| self.doc.index.element_at_position(&p))
            .or_else(|| self.doc.index.root_element());
        match selected {
            Some(id) => self.select(id),
            None => {
                self.selected = None;
                self.grid = None;
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

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
