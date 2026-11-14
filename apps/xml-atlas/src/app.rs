//! Window layout, file loading and selection state.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use egui::{Key, KeyboardShortcut, Modifiers, RichText, Ui};
use xml_atlas_engine::Document;
use xml_atlas_engine::xml::{NodeId, NodeKind, ROOT};

use crate::grid::GridModel;
use crate::tree;

pub struct App {
    open: Option<Open>,
    loading: Option<Loading>,
    message: Option<String>,
}

struct Loading {
    path: PathBuf,
    rx: Receiver<std::io::Result<Document>>,
}

/// A document plus everything the views remember about it.
pub struct Open {
    pub doc: Document,
    pub modified: bool,
    /// Element shown in the grid and selected in the tree.
    pub selected: Option<NodeId>,
    /// Node highlighted in the text view (a grid row, or the selection).
    pub focus: Option<NodeId>,
    pub tree: tree::TreeState,
    pub grid: Option<GridModel>,
    pub scroll_text_to: Option<usize>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let mut app = Self {
            open: None,
            loading: None,
            message: None,
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
                self.set_document(doc, false);
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                    "{} — XML Atlas",
                    file_name(&path)
                )));
            }
            Err(e) => self.message = Some(format!("Could not open {}: {e}", path.display())),
        }
    }

    fn set_document(&mut self, doc: Document, modified: bool) {
        let mut open = Open {
            tree: tree::TreeState::new(&doc),
            doc,
            modified,
            selected: None,
            focus: None,
            grid: None,
            scroll_text_to: None,
        };
        if let Some(root) = open.doc.index.root_element() {
            open.select(root);
        }
        self.open = Some(open);
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
        match open.doc.format(2) {
            Ok(out) => {
                let path = open.doc.path.clone();
                let mut doc = Document::from_bytes(out);
                doc.path = path;
                self.set_document(doc, true);
            }
            Err(e) => self.message = Some(format!("Cannot format: {e}")),
        }
    }

    fn save(&mut self) {
        let Some(open) = &mut self.open else { return };
        let path = match open.doc.path.clone() {
            Some(p) => p,
            None => match rfd::FileDialog::new().save_file() {
                Some(p) => p,
                None => return,
            },
        };
        match open.doc.save(&path) {
            Ok(()) => open.modified = false,
            Err(e) => self.message = Some(format!("Could not save {}: {e}", path.display())),
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
                let can_save = self.open.as_ref().is_some_and(|o| o.modified);
                if ui
                    .add_enabled(can_save, egui::Button::new("Save     Ctrl+S"))
                    .clicked()
                {
                    ui.close();
                    self.save();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui
                    .add_enabled(self.open.is_some(), egui::Button::new("Format (indent)"))
                    .clicked()
                {
                    ui.close();
                    self.format_document();
                }
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
                if open.modified { " (modified)" } else { "" },
                s.bytes as f64 / 1048576.0,
                s.lines,
                s.elements,
                s.timings.total().as_secs_f64() * 1000.0,
            ));
            if let Some(err) = &open.doc.index.error {
                let text = RichText::new(format!("  ⚠ {}", err)).color(ui.visuals().error_fg_color);
                if ui.link(text).on_hover_text("Go to the error").clicked() {
                    open.scroll_text_to = Some(open.doc.line_of(err.offset));
                }
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_loading(&ctx);

        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .find(|p| !p.as_os_str().is_empty())
        });
        if let Some(path) = dropped {
            self.start_loading(&ctx, path);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O)))
        {
            self.pick_file(&ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S)))
        {
            self.save();
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
        egui::CentralPanel::default().show(ui, |ui| {
            let total = ui.available_height();
            egui::Panel::top("grid")
                .resizable(true)
                .default_size(total * 0.45)
                .show(ui, |ui| {
                    crate::grid::show(ui, open);
                });
            egui::CentralPanel::default().show(ui, |ui| {
                crate::text_view::show(ui, open);
            });
        });
    }
}

impl Open {
    /// Makes `id` the selected element: grid shows its children, tree reveals it, text scrolls.
    pub fn select(&mut self, id: NodeId) {
        let id = self.element_or_parent(id);
        self.selected = Some(id);
        self.focus = Some(id);
        self.grid = Some(GridModel::build(&self.doc, id));
        self.tree.reveal(&self.doc, id);
        self.scroll_text_to = Some(self.doc.line_of(self.doc.index.node(id).start));
    }

    /// Highlights `id` in the text view without changing the grid.
    pub fn focus(&mut self, id: NodeId) {
        self.focus = Some(id);
        self.scroll_text_to = Some(self.doc.line_of(self.doc.index.node(id).start));
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

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
