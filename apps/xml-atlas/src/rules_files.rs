//! Settings > References: the rules files of the configuration folder (REF-13; D41), and
//! what the tabs get from them. The folder's rules and commands are the engine's
//! (`xml_atlas_engine::rules_folder`).
//!
//! The folder is read when references are switched on or the list is shown, and again when
//! it changes on disk; never per frame.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{RichText, Ui};
use xml_atlas_engine::rules_folder::{Entry, FolderError, Library, RulesFolder, applies, resolved};

/// Typing in Use for is written after this pause, or when the field is left.
const PAUSE: Duration = Duration::from_millis(700);
/// Width of a Use for field.
const USE_FOR_WIDTH: f32 = 250.0;
/// Width of the list, for the names and errors that wrap in it.
const LIST_WIDTH: f32 = 470.0;

const NOTE: &str = "XML Atlas uses only the files in this list, never one beside an XML file. \
                    A file is used for a document when it is on and Use for matches; of \
                    several, the upper one wins.";
const USE_FOR_HELP: &str = "Patterns separated by ;\n\
                            * stands for any characters, folders included, ? for one.\n\
                            Without / a pattern is compared with the file's name: catalog*.xml\n\
                            With / with the end of its path: */XML/*catalog.xml\n\
                            * alone is every document; empty is none.";

/// The folder, as last read, and the list's state in the window.
#[derive(Default)]
pub struct RulesFiles {
    folder: Option<RulesFolder>,
    library: Option<Arc<Library>>,
    /// What the disk watch looks at for it.
    watched: Vec<PathBuf>,
    use_for: Option<Draft>,
    asked: Option<Asked>,
    /// The last command that failed.
    error: Option<String>,
    /// The shown document's path, and where it really is.
    shown: Option<(PathBuf, PathBuf)>,
}

/// A Use for being typed.
struct Draft {
    name: String,
    text: String,
    changed: Instant,
}

/// What the list asks about a file.
enum Asked {
    Rename { name: String, to: String },
    Delete { name: String },
}

/// A command of the list.
enum Command {
    On(String, bool),
    Move(String, bool),
    New,
    Import,
    Duplicate(String),
    Rename(String, String),
    Delete(String),
    OpenFolder,
}

impl RulesFiles {
    /// The files in the configuration folder `config`; `None` where there is none.
    pub fn new(config: Option<&Path>) -> Self {
        Self {
            folder: config.map(RulesFolder::new),
            ..Self::default()
        }
    }

    /// Reads the folder when it is first `needed`, forgets it when it is not (REF-12), and
    /// writes a Use for whose typing has paused.
    pub fn update(&mut self, ctx: &egui::Context, needed: bool) {
        if let Some(draft) = &self.use_for {
            let waited = draft.changed.elapsed();
            if !needed || waited >= PAUSE {
                self.write_use_for();
            } else {
                ctx.request_repaint_after(PAUSE - waited);
            }
        }
        if !needed {
            self.library = None;
            self.watched.clear();
            self.shown = None;
        } else if self.library.is_none() {
            self.reload();
        }
    }

    /// Reads the folder again, if it was read.
    pub fn refresh(&mut self) {
        if self.library.is_some() {
            self.reload();
        }
    }

    pub fn library(&self) -> Option<&Arc<Library>> {
        self.library.as_ref()
    }

    pub fn watched(&self) -> &[PathBuf] {
        &self.watched
    }

    /// Writes what is typed and not yet written, when the window closes.
    pub fn flush(&mut self) {
        self.write_use_for();
    }

    /// The list. `document`: the path of the document shown, to say which file it gets.
    pub fn show(&mut self, ui: &mut Ui, document: Option<&Path>) {
        ui.set_max_width(LIST_WIDTH);
        ui.label(RichText::new(NOTE).weak());
        ui.add_space(6.0);
        if self.library.is_none() {
            self.reload();
        }
        let Some(library) = self.library.clone() else {
            return;
        };
        let red = ui.visuals().error_fg_color;
        for e in [&library.error, &self.error].into_iter().flatten() {
            ui.colored_label(red, e.as_str());
        }
        let resolved = self.resolved_document(document);
        let resolved = resolved.as_deref();
        let used = library
            .entries
            .iter()
            .position(|e| e.on && applies(&e.use_for, resolved));
        let mut command = None;
        ui.add_enabled_ui(library.error.is_none(), |ui| {
            if library.entries.is_empty() {
                ui.label("No rules files yet: import one, or make a new one.");
            }
            for (i, entry) in library.entries.iter().enumerate() {
                let last = i + 1 == library.entries.len();
                ui.push_id(&entry.name, |ui| {
                    self.file(ui, entry, (i == 0, last), resolved, &mut command);
                    if document.is_some() && used == Some(i) {
                        ui.label(RichText::new("In use for the document shown").weak());
                    } else if document.is_some()
                        && entry.on
                        && let Some(upper) = used
                        && upper < i
                        && applies(&entry.use_for, resolved)
                    {
                        let upper = &library.entries[upper].name;
                        let text = format!("Also for the document shown, but {upper} is above it");
                        ui.label(RichText::new(text).weak());
                    }
                });
                ui.separator();
            }
            ui.horizontal(|ui| {
                if ui.button("New").clicked() {
                    command = Some(Command::New);
                }
                if ui
                    .button("Import…")
                    .on_hover_text("Copies a rules file into the list; the original stays")
                    .clicked()
                {
                    command = Some(Command::Import);
                }
                if ui.button("Open folder").clicked() {
                    command = Some(Command::OpenFolder);
                }
            });
        });
        if let Some(folder) = &self.folder {
            let path = folder.path().display().to_string();
            ui.add(egui::Label::new(RichText::new(path).weak()).truncate());
        }
        if let Some(command) = command {
            self.run(command);
        }
    }

    /// One file's lines.
    fn file(
        &mut self,
        ui: &mut Ui,
        entry: &Entry,
        (first, last): (bool, bool),
        document: Option<&Path>,
        command: &mut Option<Command>,
    ) {
        let name = &entry.name;
        ui.horizontal(|ui| {
            let mut on = entry.on;
            if ui
                .checkbox(&mut on, RichText::new(name).strong())
                .on_hover_text(entry.path.display().to_string())
                .changed()
            {
                *command = Some(Command::On(name.clone(), on));
            }
        });
        ui.horizontal(|ui| {
            let label = ui.label("Use for");
            let mut text = match &self.use_for {
                Some(draft) if draft.name == *name => draft.text.clone(),
                _ => entry.use_for.clone(),
            };
            let field = ui
                .add(
                    egui::TextEdit::singleline(&mut text)
                        .id_salt("use-for")
                        .desired_width(USE_FOR_WIDTH)
                        .hint_text("no document"),
                )
                .labelled_by(label.id)
                .on_hover_text(USE_FOR_HELP);
            if field.changed() {
                self.type_use_for(name, text.clone());
            }
            if field.lost_focus() {
                self.write_use_for();
            }
            let add = document
                .and_then(Path::file_name)
                .and_then(|n| n.to_str())
                .filter(|_| !applies(&text, document));
            if let Some(shown) = add
                && ui
                    .small_button("+ shown")
                    .on_hover_text(format!("Add {shown}, the document shown"))
                    .clicked()
            {
                let sep = if text.trim().is_empty() { "" } else { "; " };
                self.type_use_for(name, format!("{}{sep}{shown}", text.trim()));
                self.write_use_for();
            }
        });
        ui.horizontal(|ui| {
            let mut button = |ui: &mut Ui, enabled: bool, text: &str, what: Command| {
                if ui
                    .add_enabled(enabled, egui::Button::new(text).small())
                    .clicked()
                {
                    *command = Some(what);
                }
            };
            button(ui, !first, "Up", Command::Move(name.clone(), true));
            button(ui, !last, "Down", Command::Move(name.clone(), false));
            button(ui, true, "Duplicate", Command::Duplicate(name.clone()));
            if ui.small_button("Rename…").clicked() {
                self.asked = Some(Asked::Rename {
                    name: name.clone(),
                    to: name.clone(),
                });
            }
            if ui.small_button("Delete…").clicked() {
                self.asked = Some(Asked::Delete { name: name.clone() });
            }
        });
        if let Err(e) = &entry.rules {
            ui.colored_label(ui.visuals().error_fg_color, e.as_str());
        } else if entry.on && entry.use_for.is_empty() {
            let text = "Used for no document until Use for says which";
            ui.colored_label(ui.visuals().warn_fg_color, text);
        }
        let mut answered = false;
        match &mut self.asked {
            Some(Asked::Rename { name: of, to }) if of == name => {
                ui.horizontal(|ui| {
                    let label = ui.label("New name");
                    ui.text_edit_singleline(to).labelled_by(label.id);
                    if ui.button("Rename").clicked() {
                        *command = Some(Command::Rename(of.clone(), to.clone()));
                        answered = true;
                    }
                    answered |= ui.button("Cancel").clicked();
                });
            }
            Some(Asked::Delete { name: of }) if of == name => {
                ui.label(format!("Delete {of} from the folder? It cannot be undone."));
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        *command = Some(Command::Delete(of.clone()));
                        answered = true;
                    }
                    answered |= ui.button("Cancel").clicked();
                });
            }
            _ => {}
        }
        if answered {
            self.asked = None;
        }
    }

    fn type_use_for(&mut self, name: &str, text: String) {
        if self.use_for.as_ref().is_some_and(|d| d.name != name) {
            self.write_use_for();
        }
        self.use_for = Some(Draft {
            name: name.to_owned(),
            text,
            changed: Instant::now(),
        });
    }

    fn write_use_for(&mut self) {
        let Some(draft) = self.use_for.take() else {
            return;
        };
        let Some(folder) = &self.folder else { return };
        let done = folder.set_use_for(&draft.name, &draft.text);
        self.done(done);
    }

    fn run(&mut self, command: Command) {
        self.write_use_for();
        let Some(folder) = &self.folder else { return };
        let done: Result<(), FolderError> = match command {
            Command::On(name, on) => folder.set_on(&name, on),
            Command::Move(name, up) => folder.move_by(&name, up),
            Command::New => folder.create().map(|_| ()),
            Command::Import => {
                let picked = rfd::FileDialog::new()
                    .add_filter("Rules", &["json"])
                    .set_title("Import a rules file")
                    .pick_file();
                match picked {
                    Some(file) => folder.import(&file).map(|_| ()),
                    None => return,
                }
            }
            Command::Duplicate(name) => folder.duplicate(&name).map(|_| ()),
            Command::Rename(name, to) => folder.rename(&name, &to).map(|_| ()),
            Command::Delete(name) => folder.delete(&name),
            Command::OpenFolder => folder.create_folder().and_then(|path| {
                show_folder(path).map_err(|error| FolderError::Io {
                    path: path.to_owned(),
                    error,
                })
            }),
        };
        self.done(done);
    }

    fn done(&mut self, done: Result<(), FolderError>) {
        self.error = done.err().map(|e| e.to_string());
        self.reload();
    }

    fn reload(&mut self) {
        let (new, mut watched) = match &self.folder {
            Some(folder) => (folder.load(), folder.watched().map(Path::to_owned).to_vec()),
            None => (
                Library {
                    error: Some("there is no configuration folder".into()),
                    ..Library::default()
                },
                Vec::new(),
            ),
        };
        watched.extend(new.entries.iter().map(|e| e.path.clone()));
        self.watched = watched;
        // The same, kept: the tabs compare the pointer.
        if self.library.as_deref() != Some(&new) {
            self.library = Some(Arc::new(new));
        }
    }

    /// Where the shown document is, worked out once per document and not per frame.
    fn resolved_document(&mut self, document: Option<&Path>) -> Option<PathBuf> {
        let document = document?;
        if self
            .shown
            .as_ref()
            .is_none_or(|(given, _)| given != document)
        {
            self.shown = Some((document.to_owned(), resolved(document)));
        }
        self.shown.as_ref().map(|(_, real)| real.clone())
    }
}

/// Opens `path` in the system's file manager.
fn show_folder(path: &Path) -> std::io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = std::process::Command::new(program).arg(path).spawn()?;
    // Waited for elsewhere, so that it does not stay behind as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REF-12, REF-13: read when needed and forgotten after; a change on disk is picked up,
    /// and the same folder keeps the same library, so the tabs do nothing.
    #[test]
    fn the_folder_is_read_only_while_needed() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let folder = RulesFolder::new(&dir);
        folder.create().unwrap();
        let ctx = egui::Context::default();
        let mut files = RulesFiles::new(Some(&dir));
        files.update(&ctx, false);
        assert!(files.library().is_none() && files.watched().is_empty());
        files.update(&ctx, true);
        let first = files.library().unwrap().clone();
        assert_eq!(first.entries.len(), 1);
        assert_eq!(files.watched().len(), 3);
        files.refresh();
        assert!(Arc::ptr_eq(&first, files.library().unwrap()));
        folder.create().unwrap();
        files.refresh();
        assert_eq!(files.library().unwrap().entries.len(), 2);
        files.update(&ctx, false);
        assert!(files.library().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A typed Use for is not lost when the list goes away before the pause is over.
    #[test]
    fn a_typed_pattern_is_written_when_the_list_goes() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-typed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let folder = RulesFolder::new(&dir);
        let name = folder.create().unwrap();
        let ctx = egui::Context::default();
        let mut files = RulesFiles::new(Some(&dir));
        files.update(&ctx, true);
        files.type_use_for(&name, "Größe.xml".into());
        assert_eq!(folder.load().entries[0].use_for, "", "not on every key");
        files.update(&ctx, false);
        assert_eq!(folder.load().entries[0].use_for, "Größe.xml");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
