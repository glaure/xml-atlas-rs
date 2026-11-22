//! File > Settings… (`docs/spec/window.md`, WIN-11; `docs/decisions.md`, D31).
//!
//! Every remembered setting (WIN-3) in one window, in sections. A change applies and is saved
//! at once, as the View menu's options are; there is no OK or Cancel. The app applies what a
//! change means beyond the setting itself (the tree's attribute rows, the font) after the
//! window is drawn, so the window only edits [`Settings`].

use egui::{Context, RichText, Ui};
use xml_atlas_engine::FormatOptions;

use crate::rules_files::RulesFiles;
use crate::settings::{Layout, LightStyle, Settings, ThemeChoice};
use crate::theme::Theme;
use crate::theme_preview::choice;

/// Under the Views section.
const VIEWS_NOTE: &str = "The View menu switches these too. Alt+Z switches word wrap for the \
                          open tab alone.";
/// Under the Format section.
const FORMAT_NOTE: &str = "Edit > Format options… changes the same settings. A file's own \
                           settings comment comes first.";

/// Under the Updates section: what the check sends, and what it does not do.
const UPDATES_NOTE: &str = "XML Atlas asks GitHub for the latest release and tells you in the \
                            status bar when it is newer. It sends nothing but that request, \
                            with the program's version, and downloads nothing unless you \
                            click Download and install in that notice.";
/// Width of the list of sections on the left.
const SECTIONS_WIDTH: f32 = 120.0;
/// Height kept free around a section for the window's title, margins and the program's own
/// bars; a section taller than the rest of the screen scrolls.
const SECTION_MARGIN: f32 = 120.0;
/// The window's id.
const WINDOW_ID: &str = "settings-window";
/// The least height a section scrolls in.
const SECTION_MIN_HEIGHT: f32 = 120.0;

/// A part of the Settings window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Appearance,
    Views,
    References,
    Format,
    Updates,
    Agents,
}

/// What the window asks the app to do, beyond changing a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Open the font dialog (TEXT-25).
    ChooseFont,
    /// Ask for a newer version now (PKG-7).
    CheckForUpdates,
}

/// What Settings > References shows: the rules files, and the document they may be for.
pub struct RulesList<'a> {
    pub files: &'a mut RulesFiles,
    pub document: Option<&'a std::path::Path>,
}

/// Whether the window is open, and the section shown.
#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    section: Section,
}

impl Section {
    /// Every section, in the order the window lists them.
    pub const ALL: [Section; 6] = [
        Section::Appearance,
        Section::Views,
        Section::References,
        Section::Format,
        Section::Updates,
        Section::Agents,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Views => "Views",
            Section::References => "References",
            Section::Format => "Format",
            Section::Updates => "Updates",
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
                settings.panes = d.panes;
                settings.layout = d.layout;
                settings.tree_attributes = d.tree_attributes;
                settings.grid = d.grid;
                settings.cell_pane = d.cell_pane;
                settings.diagram = d.diagram;
                settings.word_wrap = d.word_wrap;
            }
            // The switches only: the files and their list are the user's.
            Section::References => {
                settings.references = d.references;
                settings.reference_preview = d.reference_preview;
                settings.preview_tree = d.preview_tree;
            }
            Section::Format => settings.format = d.format,
            // When it last asked, and what it told, are not settings to reset.
            Section::Updates => settings.update_check = d.update_check,
            Section::Agents => settings.agents = d.agents,
        }
    }
}

impl SettingsWindow {
    /// Whether the list of rules files is in view.
    pub fn shows_references(&self) -> bool {
        self.open && self.section == Section::References
    }

    /// Shows the window while it is open; changes go straight into `settings`. `checking`: an
    /// update check is under way.
    pub fn show(
        &mut self,
        ctx: &Context,
        settings: &mut Settings,
        rules: RulesList<'_>,
        checking: bool,
    ) -> Option<Request> {
        let mut open = self.open;
        let mut request = None;
        let height = (ctx.content_rect().height() - SECTION_MARGIN).max(SECTION_MIN_HEIGHT);
        egui::Window::new("Settings")
            .id(egui::Id::unique(WINDOW_ID))
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
                    // Vertical first: a scroll area lays out as the row it is in would.
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(("settings-section", self.section.title()))
                            .max_height(height)
                            // Without it a section would scroll at the window's default
                            // height, also where the screen has room.
                            .min_scrolled_height(height)
                            .show(ui, |ui| {
                                ui.heading(self.section.title());
                                ui.add_space(6.0);
                                request = section(ui, self.section, settings, rules, checking);
                                ui.add_space(12.0);
                                if ui.button("Reset to defaults").clicked() {
                                    self.section.reset(settings);
                                }
                            });
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
fn section(
    ui: &mut Ui,
    section: Section,
    s: &mut Settings,
    rules: RulesList<'_>,
    checking: bool,
) -> Option<Request> {
    let mut request = None;
    match section {
        Section::Appearance => {
            theme_choices(ui, s);
            ui.add_space(8.0);
            egui::Grid::new("settings-appearance")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
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
                        ui.checkbox(&mut s.diagram, "Diagram, in the grid's place");
                        ui.checkbox(&mut s.word_wrap, "Word wrap");
                    });
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.label(RichText::new(VIEWS_NOTE).weak());
        }
        Section::References => {
            crate::ref_preview::switches(ui, s);
            ui.add_space(8.0);
            rules.files.show(ui, rules.document);
        }
        Section::Format => {
            format_options(ui, &mut s.format);
            ui.add_space(6.0);
            ui.label(RichText::new(FORMAT_NOTE).weak());
        }
        Section::Updates => {
            ui.checkbox(&mut s.update_check, "Check for newer versions once a day");
            ui.horizontal(|ui| {
                let last = crate::update::ago(s.update_last, crate::update::now());
                ui.label(format!("Last checked: {last}"));
                if checking {
                    ui.spinner();
                } else if ui.button("Check now").clicked() {
                    request = Some(Request::CheckForUpdates);
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new(UPDATES_NOTE).weak());
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

/// The theme mode and the light style, each choice under an example of how it looks
/// (WIN-12). Light, and the light half of Follow system, are in the chosen light style.
fn theme_choices(ui: &mut Ui, s: &mut Settings) {
    let (soft, classic, dark) = (Theme::light(), Theme::light_classic(), Theme::dark());
    let light = match s.light_style {
        LightStyle::Soft => &soft,
        LightStyle::Classic => &classic,
    };
    ui.label("Theme");
    ui.horizontal_top(|ui| {
        let (theme, system) = (&mut s.theme, ThemeChoice::System);
        choice(ui, theme, system, "Follow system", &[light, &dark]);
        choice(ui, theme, ThemeChoice::Light, "Light", &[light]);
        choice(ui, theme, ThemeChoice::Dark, "Dark", &[&dark]);
    });
    ui.add_space(8.0);
    ui.label("Light style");
    ui.horizontal_top(|ui| {
        let style = &mut s.light_style;
        choice(ui, style, LightStyle::Soft, "Soft grey", &[&soft]);
        choice(
            ui,
            style,
            LightStyle::Classic,
            "Classic, as in 1.0",
            &[&classic],
        );
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use xml_atlas_engine::find::FindOptions;

    use super::*;

    /// Every setting but the window's size, the recent files, the find options and what the update check
    /// remembers is in a section, and every
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
            diagram: true,
            word_wrap: true,
            toolbar: false,
            references: true,
            reference_preview: true,
            preview_tree: true,
            text_font: crate::fonts::TextFont {
                family: Some("Größe Mono".into()),
                face: crate::fonts::Face::Bold,
                size: 15,
                file: Some((PathBuf::from("C:\\Fonts\\Größe.ttf"), 1)),
            },
            agents: true,
            update_check: false,
            update_last: Some(1_800_000_000),
            update_shown: Some(crate::update::Version(1, 2, 0)),
            find: FindOptions {
                match_case: true,
                whole_word: true,
            },
            recent: vec![PathBuf::from("/home/me/Größe.xml")],
            window: crate::sizes::Window {
                maximized: true,
                ..Default::default()
            },
            panes: {
                let mut panes = crate::sizes::Panes::default();
                assert!(panes.read("tree", "0.3"));
                panes
            },
        };
        let mut reset = changed.clone();
        for section in Section::ALL {
            let before = reset.clone();
            section.reset(&mut reset);
            assert_ne!(before, reset, "{section:?} resets nothing");
        }
        let expected = Settings {
            window: changed.window,
            update_last: changed.update_last,
            update_shown: changed.update_shown,
            find: changed.find,
            recent: changed.recent.clone(),
            ..Settings::default()
        };
        assert_eq!(reset, expected);
    }

    /// With the theme examples, Appearance still fits: whole in the program's own window
    /// size, and inside a small screen, where it scrolls.
    #[test]
    fn appearance_fits_the_screen() {
        for (size, whole) in [
            (egui::vec2(1280.0, 820.0), true),
            (egui::vec2(800.0, 420.0), false),
        ] {
            let ctx = Context::default();
            let mut window = SettingsWindow {
                open: true,
                ..Default::default()
            };
            let mut settings = Settings::default();
            let mut files = RulesFiles::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut reset_shown = false;
            // A window learns its size in its first frames.
            for _ in 0..4 {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    window.show(ui.ctx(), &mut settings, no_rules(&mut files), false);
                });
                out.textures_delta.clear();
                // The last widget: not painted at all when scrolled out, or clipped.
                reset_shown = out.shapes.iter().any(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == "Reset to defaults" => s
                        .clip_rect
                        .contains_rect(t.galley.rect.translate(t.pos.to_vec2())),
                    _ => false,
                });
            }
            let rect = ctx
                .memory(|m| m.area_rect(egui::Id::unique(WINDOW_ID)))
                .expect("the window was drawn");
            assert!(
                screen.expand(1.0).contains_rect(rect),
                "{rect:?} in {screen:?}"
            );
            assert_eq!(reset_shown, whole, "in {screen:?}");
        }
    }

    fn no_rules(files: &mut RulesFiles) -> RulesList<'_> {
        RulesList {
            files,
            document: None,
        }
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
        let mut files = RulesFiles::default();
        for section in Section::ALL {
            window.section = section;
            let mut out = ctx.run_ui(Default::default(), |ui| {
                assert_eq!(
                    window.show(ui.ctx(), &mut settings, no_rules(&mut files), false),
                    None
                );
            });
            out.textures_delta.clear();
            assert!(window.open);
        }
        assert_eq!(settings, Settings::default());
    }
}
