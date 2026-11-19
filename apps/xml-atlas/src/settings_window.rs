//! File > Settings… (`docs/spec/window.md`, WIN-11; `docs/decisions.md`, D31).
//!
//! Every remembered setting (WIN-3) in one window, in sections. A change applies and is saved
//! at once, as the View menu's options are; there is no OK or Cancel. The app applies what a
//! change means beyond the setting itself (the tree's attribute rows, the font) after the
//! window is drawn, so the window only edits [`Settings`].

use egui::{Context, RichText, Ui};
use xml_atlas_engine::FormatOptions;

use crate::settings::{Layout, LightStyle, Settings, ThemeChoice};

/// Under the Views section.
const VIEWS_NOTE: &str = "The View menu switches these too. Alt+Z switches word wrap for the \
                          open tab alone.";
/// Under the Format section.
const FORMAT_NOTE: &str = "Edit > Format options… changes the same settings. A file's own \
                           settings comment comes first.";

/// Width of the list of sections on the left.
const SECTIONS_WIDTH: f32 = 120.0;

/// A part of the Settings window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Appearance,
    Views,
    Format,
    Agents,
}

/// What the window asks the app to do, beyond changing a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Open the font dialog (TEXT-25).
    ChooseFont,
}

/// Whether the window is open, and the section shown.
#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    section: Section,
}

impl Section {
    /// Every section, in the order the window lists them.
    pub const ALL: [Section; 4] = [
        Section::Appearance,
        Section::Views,
        Section::Format,
        Section::Agents,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Views => "Views",
            Section::Format => "Format",
            Section::Agents => "AI agents",
        }
    }

    /// Puts this section's settings back to their defaults, and leaves the others.
    pub fn reset(self, settings: &mut Settings) {
        let d = Settings::default();
        match self {
            Section::Appearance => {
                settings.theme = d.theme;
                settings.light_style = d.light_style;
                settings.text_font = d.text_font;
                settings.toolbar = d.toolbar;
            }
            Section::Views => {
                settings.layout = d.layout;
                settings.tree_attributes = d.tree_attributes;
                settings.grid = d.grid;
                settings.cell_pane = d.cell_pane;
                settings.reference_preview = d.reference_preview;
                settings.preview_tree = d.preview_tree;
                settings.word_wrap = d.word_wrap;
            }
            Section::Format => settings.format = d.format,
            Section::Agents => settings.agents = d.agents,
        }
    }
}

impl SettingsWindow {
    /// Shows the window while it is open; changes go straight into `settings`.
    pub fn show(&mut self, ctx: &Context, settings: &mut Settings) -> Option<Request> {
        let mut open = self.open;
        let mut request = None;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .default_size([560.0, 360.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(SECTIONS_WIDTH);
                        for s in Section::ALL {
                            ui.selectable_value(&mut self.section, s, s.title());
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.heading(self.section.title());
                        ui.add_space(6.0);
                        request = section(ui, self.section, settings);
                        ui.add_space(12.0);
                        if ui.button("Reset to defaults").clicked() {
                            self.section.reset(settings);
                        }
                    });
                });
            });
        self.open = open;
        request
    }
}

/// The Format settings (FMT-10 to FMT-13), shared by Settings and Edit > Format options….
pub fn format_options(ui: &mut Ui, edit: &mut FormatOptions) {
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
}

/// The controls of one section.
fn section(ui: &mut Ui, section: Section, s: &mut Settings) -> Option<Request> {
    let mut request = None;
    match section {
        Section::Appearance => {
            egui::Grid::new("settings-appearance")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Theme");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut s.theme, ThemeChoice::System, "Follow system");
                        ui.radio_value(&mut s.theme, ThemeChoice::Light, "Light");
                        ui.radio_value(&mut s.theme, ThemeChoice::Dark, "Dark");
                    });
                    ui.end_row();
                    ui.label("Light style");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut s.light_style, LightStyle::Soft, "Soft grey");
                        ui.radio_value(
                            &mut s.light_style,
                            LightStyle::Classic,
                            "Classic, as in 1.0",
                        );
                    });
                    ui.end_row();
                    ui.label("Font");
                    ui.horizontal(|ui| {
                        let f = &s.text_font;
                        let family = f.family.as_deref().unwrap_or("Built-in");
                        ui.label(format!("{family}, {} pt", f.size));
                        if ui.button("Choose…").clicked() {
                            request = Some(Request::ChooseFont);
                        }
                    });
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut s.toolbar, "Toolbar");
                    ui.end_row();
                });
        }
        Section::Views => {
            egui::Grid::new("settings-views")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Layout");
                    ui.vertical(|ui| {
                        ui.radio_value(&mut s.layout, Layout::GridAbove, "Grid above text");
                        ui.radio_value(
                            &mut s.layout,
                            Layout::TextAbove,
                            "Text above grid (as in XML Marker)",
                        );
                    });
                    ui.end_row();
                    ui.label("Show");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut s.tree_attributes, "Attributes in the tree");
                        ui.checkbox(&mut s.grid, "Grid");
                        ui.add_enabled(
                            s.grid,
                            egui::Checkbox::new(
                                &mut s.cell_pane,
                                "Cell pane beside the grid (experiment)",
                            ),
                        );
                        ui.checkbox(&mut s.reference_preview, "Preview with references resolved");
                        ui.add_enabled(
                            s.reference_preview,
                            egui::Checkbox::new(&mut s.preview_tree, "Preview tree"),
                        );
                        ui.checkbox(&mut s.word_wrap, "Word wrap");
                    });
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.label(RichText::new(VIEWS_NOTE).weak());
        }
        Section::Format => {
            format_options(ui, &mut s.format);
            ui.add_space(6.0);
            ui.label(RichText::new(FORMAT_NOTE).weak());
        }
        Section::Agents => {
            ui.checkbox(&mut s.agents, "Allow AI agents (MCP)");
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Agents such as Claude Code may read and edit the open documents through \
                     `xml-atlas-cli mcp --window`. Each change is one undo step. While this is \
                     on, the window listens on 127.0.0.1, for this computer only.",
                )
                .weak(),
            );
        }
    }
    request
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use xml_atlas_engine::find::FindOptions;

    use super::*;

    /// Every setting but the recent files and the find options is in a section, and every
    /// section resets something, so resetting all sections gives the defaults. Naming every field without `..` makes a new
    /// setting fail to compile here until it is given a section.
    #[test]
    fn every_setting_has_a_section() {
        let changed = Settings {
            format: FormatOptions {
                indent_size: 2,
                tabs: true,
                max_attributes: 0,
                remove_comments: true,
                trim_text: true,
            },
            layout: Layout::GridAbove,
            theme: ThemeChoice::Dark,
            light_style: LightStyle::Classic,
            tree_attributes: false,
            grid: false,
            cell_pane: false,
            word_wrap: true,
            toolbar: false,
            reference_preview: true,
            preview_tree: true,
            text_font: crate::fonts::TextFont {
                family: Some("Größe Mono".into()),
                face: crate::fonts::Face::Bold,
                size: 15,
                file: Some((PathBuf::from("C:\\Fonts\\Größe.ttf"), 1)),
            },
            agents: true,
            find: FindOptions {
                match_case: true,
                whole_word: true,
            },
            recent: vec![PathBuf::from("/home/me/Größe.xml")],
        };
        let mut reset = changed.clone();
        for section in Section::ALL {
            let before = reset.clone();
            section.reset(&mut reset);
            assert_ne!(before, reset, "{section:?} resets nothing");
        }
        let expected = Settings {
            find: changed.find,
            recent: changed.recent.clone(),
            ..Settings::default()
        };
        assert_eq!(reset, expected);
    }

    /// Drawing every section changes no setting and closes nothing.
    #[test]
    fn drawing_the_window_changes_nothing() {
        let ctx = Context::default();
        let mut window = SettingsWindow {
            open: true,
            ..Default::default()
        };
        let mut settings = Settings::default();
        for section in Section::ALL {
            window.section = section;
            let mut out = ctx.run_ui(Default::default(), |ui| {
                assert_eq!(window.show(ui.ctx(), &mut settings), None);
            });
            out.textures_delta.clear();
            assert!(window.open);
        }
        assert_eq!(settings, Settings::default());
    }
}
