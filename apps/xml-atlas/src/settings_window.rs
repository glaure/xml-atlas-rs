//! File > Settings… (`docs/spec/window.md`, WIN-11; `docs/decisions.md`, D31).
//!
//! Every remembered setting (WIN-3) in one window, in sections. A change applies and is saved
//! at once, as the View menu's options are; there is no OK or Cancel. The app applies what a
//! change means beyond the setting itself (the tree's attribute rows, the font) after the
//! window is drawn, so the window only edits [`Settings`].

use crate::lang::Lang;
use egui::{Context, RichText, Ui};
use xml_atlas_engine::FormatOptions;

use crate::keys;
use crate::lang::LanguageChoice;
use crate::rules_files::RulesFiles;
use crate::settings::{Layout, LightStyle, Settings, ThemeChoice};
use crate::theme::Theme;
use crate::theme_preview::choice;

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
    Localization,
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
    pub const ALL: [Section; 7] = [
        Section::Appearance,
        Section::Localization,
        Section::Views,
        Section::References,
        Section::Format,
        Section::Updates,
        Section::Agents,
    ];

    fn title(self, lang: Lang) -> &'static str {
        match self {
            Section::Appearance => lang.settings_appearance(),
            Section::Localization => lang.settings_localization(),
            Section::Views => lang.settings_views(),
            Section::References => lang.settings_references(),
            Section::Format => lang.settings_format(),
            Section::Updates => lang.settings_updates(),
            Section::Agents => lang.settings_agents(),
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
            Section::Localization => settings.language = d.language,
            Section::Views => {
                settings.panes = d.panes;
                settings.layout = d.layout;
                settings.tree_attributes = d.tree_attributes;
                settings.expand_on_text_click = d.expand_on_text_click;
                settings.grid = d.grid;
                settings.cell_pane = d.cell_pane;
                settings.diagram = d.diagram;
                settings.grid_behind_diagram = d.grid_behind_diagram;
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
    /// Opens the window on `section`, as Tools > References > Rules files… does.
    pub fn show_section(&mut self, section: Section) {
        self.open = true;
        self.section = section;
    }

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
        lang: Lang,
    ) -> Option<Request> {
        let mut open = self.open;
        let mut request = None;
        let height = (ctx.content_rect().height() - SECTION_MARGIN).max(SECTION_MIN_HEIGHT);
        egui::Window::new(lang.settings_title())
            .id(egui::Id::unique(WINDOW_ID))
            .open(&mut open)
            .collapsible(false)
            .default_size([560.0, 360.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(SECTIONS_WIDTH);
                        for s in Section::ALL {
                            ui.selectable_value(&mut self.section, s, s.title(lang));
                        }
                    });
                    ui.separator();
                    // Vertical first: a scroll area lays out as the row it is in would.
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(("settings-section", self.section.title(Lang::En)))
                            .max_height(height)
                            // Without it a section would scroll at the window's default
                            // height, also where the screen has room.
                            .min_scrolled_height(height)
                            .show(ui, |ui| {
                                ui.heading(self.section.title(lang));
                                ui.add_space(6.0);
                                request =
                                    section(ui, lang, self.section, settings, rules, checking);
                                ui.add_space(12.0);
                                if ui.button(lang.settings_reset()).clicked() {
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

/// The Format settings (FMT-10 to FMT-13), shared by Settings and Tools > Format options….
pub fn format_options(ui: &mut Ui, lang: Lang, edit: &mut FormatOptions) {
    egui::Grid::new("format-options-grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(lang.settings_format_indent());
            ui.horizontal(|ui| {
                ui.radio_value(&mut edit.tabs, false, lang.settings_format_spaces());
                ui.add_enabled(
                    !edit.tabs,
                    egui::DragValue::new(&mut edit.indent_size).range(0..=16),
                );
                ui.radio_value(&mut edit.tabs, true, lang.settings_format_tabs());
            });
            ui.end_row();
            ui.label(lang.settings_format_attributes());
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut edit.max_attributes).range(0..=99));
                ui.label(RichText::new(lang.settings_format_attributes_hint()).weak());
            });
            ui.end_row();
            ui.label(lang.settings_format_also());
            ui.vertical(|ui| {
                ui.checkbox(
                    &mut edit.remove_comments,
                    lang.settings_format_remove_comments(),
                );
                ui.checkbox(&mut edit.trim_text, lang.settings_format_trim_text());
                ui.label(RichText::new(lang.settings_format_content_note()).weak());
            });
            ui.end_row();
        });
}

/// The controls of one section.
fn section(
    ui: &mut Ui,
    lang: Lang,
    section: Section,
    s: &mut Settings,
    rules: RulesList<'_>,
    checking: bool,
) -> Option<Request> {
    let mut request = None;
    match section {
        Section::Appearance => {
            theme_choices(ui, lang, s);
            ui.add_space(8.0);
            egui::Grid::new("settings-appearance")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label(lang.settings_font());
                    ui.horizontal(|ui| {
                        let f = &s.text_font;
                        let family = f.family.as_deref().unwrap_or(lang.settings_font_built_in());
                        ui.label(lang.settings_font_summary(family, &f.size.to_string()));
                        if ui.button(lang.settings_font_choose()).clicked() {
                            request = Some(Request::ChooseFont);
                        }
                    });
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut s.toolbar, lang.switch_toolbar());
                    ui.end_row();
                });
        }
        Section::Localization => language_row(ui, lang, s),
        Section::Views => {
            egui::Grid::new("settings-views")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label(lang.settings_views_layout());
                    ui.vertical(|ui| {
                        ui.radio_value(&mut s.layout, Layout::GridAbove, lang.switch_grid_above());
                        ui.radio_value(&mut s.layout, Layout::TextAbove, lang.switch_text_above());
                    });
                    ui.end_row();
                    ui.label(lang.settings_views_show());
                    ui.vertical(|ui| {
                        ui.checkbox(&mut s.tree_attributes, lang.switch_attributes());
                        ui.checkbox(&mut s.expand_on_text_click, lang.switch_expand_on_click());
                        let mut grid = s.grid;
                        if ui.checkbox(&mut grid, lang.switch_grid()).changed() {
                            s.show_grid(grid);
                        }
                        ui.add_enabled(
                            s.grid,
                            egui::Checkbox::new(&mut s.cell_pane, lang.switch_cell_pane()),
                        )
                        .on_disabled_hover_text(lang.why_no_grid());
                        let mut diagram = s.diagram;
                        if ui
                            .checkbox(&mut diagram, lang.settings_views_diagram())
                            .changed()
                        {
                            s.show_diagram(diagram);
                        }
                        ui.checkbox(&mut s.word_wrap, lang.switch_wrap_all());
                    });
                    ui.end_row();
                });
            ui.add_space(6.0);
            let wrap = keys::text(ui.ctx(), lang, &keys::WORD_WRAP);
            let note = lang.settings_views_note(&wrap);
            ui.label(RichText::new(note).weak());
        }
        Section::References => {
            crate::ref_preview::switches(ui, lang, s);
            ui.add_space(8.0);
            rules.files.show(ui, lang, rules.document);
        }
        Section::Format => {
            format_options(ui, lang, &mut s.format);
            ui.add_space(6.0);
            ui.label(RichText::new(lang.settings_format_note()).weak());
        }
        Section::Updates => {
            ui.checkbox(&mut s.update_check, lang.settings_updates_daily());
            ui.horizontal(|ui| {
                let last = crate::update::ago(lang, s.update_last, crate::update::now());
                ui.label(lang.settings_updates_last(&last));
                if checking {
                    ui.spinner();
                } else if ui.button(lang.settings_updates_check_now()).clicked() {
                    request = Some(Request::CheckForUpdates);
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new(lang.settings_updates_note()).weak());
        }
        Section::Agents => {
            ui.checkbox(&mut s.agents, lang.settings_agents_allow());
            ui.add_space(6.0);
            ui.label(RichText::new(lang.settings_agents_note()).weak());
        }
    }
    request
}

/// The row of Localization (LANG-6): its label is the same in every language, and each
/// language is named in itself.
fn language_row(ui: &mut Ui, lang: Lang, s: &mut Settings) {
    ui.horizontal(|ui| {
        ui.label(lang.settings_language());
        for (value, name) in [
            (LanguageChoice::System, lang.settings_language_system()),
            (LanguageChoice::English, "English"),
            (LanguageChoice::German, "Deutsch"),
        ] {
            ui.radio_value(&mut s.language, value, name);
        }
    });
}

/// The theme mode and the light style, each choice under an example of how it looks
/// (WIN-12). Light, and the light half of Follow system, are in the chosen light style.
fn theme_choices(ui: &mut Ui, lang: Lang, s: &mut Settings) {
    let (soft, classic, dark) = (Theme::light(), Theme::light_classic(), Theme::dark());
    let light = match s.light_style {
        LightStyle::Soft => &soft,
        LightStyle::Classic => &classic,
    };
    ui.label(lang.theme_heading());
    ui.horizontal_top(|ui| {
        let (theme, system) = (&mut s.theme, ThemeChoice::System);
        choice(
            ui,
            lang,
            theme,
            system,
            lang.switch_theme_system(),
            &[light, &dark],
        );
        choice(
            ui,
            lang,
            theme,
            ThemeChoice::Light,
            lang.switch_theme_light(),
            &[light],
        );
        choice(
            ui,
            lang,
            theme,
            ThemeChoice::Dark,
            lang.switch_theme_dark(),
            &[&dark],
        );
    });
    ui.add_space(8.0);
    ui.label(lang.theme_light_style());
    ui.horizontal_top(|ui| {
        let style = &mut s.light_style;
        choice(
            ui,
            lang,
            style,
            LightStyle::Soft,
            lang.theme_soft(),
            &[&soft],
        );
        choice(
            ui,
            lang,
            style,
            LightStyle::Classic,
            lang.theme_classic(),
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
            language: LanguageChoice::German,
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
            expand_on_text_click: false,
            grid: false,
            cell_pane: false,
            diagram: true,
            grid_behind_diagram: true,
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
                    window.show(
                        ui.ctx(),
                        &mut settings,
                        no_rules(&mut files),
                        false,
                        Lang::En,
                    );
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
                    window.show(
                        ui.ctx(),
                        &mut settings,
                        no_rules(&mut files),
                        false,
                        crate::lang::Lang::En
                    ),
                    None
                );
            });
            out.textures_delta.clear();
            assert!(window.open);
        }
        assert_eq!(settings, Settings::default());
    }

    /// No part of Settings but the theme examples is narrower.
    const EXAMPLE_WIDTH: f32 = 110.0;

    /// LANG-15: every section of Settings in German, in the program's own window size, with
    /// a rules file in the list: nothing is cut off or lies over another text.
    #[test]
    fn every_section_fits_in_german() {
        let config = std::env::temp_dir().join(format!("xml-atlas-german-{}", std::process::id()));
        let folder = xml_atlas_engine::rules_folder::RulesFolder::new(&config);
        let name = folder.create().unwrap();
        folder
            .set_use_for(&name, "Bestellungen-Größe*.xml")
            .unwrap();
        for section in Section::ALL {
            let ctx = Context::default();
            ctx.set_os(egui::os::OperatingSystem::Nix);
            let mut window = SettingsWindow {
                open: true,
                section,
            };
            let mut settings = Settings {
                references: true,
                update_last: Some(1_800_000_000),
                ..Settings::default()
            };
            let mut files = RulesFiles::new(Some(&config));
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
            let mut texts = Vec::new();
            for _ in 0..6 {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    window.show(
                        ui.ctx(),
                        &mut settings,
                        no_rules(&mut files),
                        false,
                        Lang::De,
                    );
                });
                out.textures_delta.clear();
                texts.clear();
                for clipped in out.shapes {
                    collect(clipped.shape, clipped.clip_rect, &mut texts);
                }
            }
            let title = section.title(Lang::De);
            assert!(texts.iter().any(|(t, ..)| t == title), "{title}");
            // LANG-6: the language is chosen in its own section, and nowhere else.
            assert_eq!(
                texts.iter().any(|(t, ..)| t == "Language / Sprache"),
                section == Section::Localization,
                "{title}"
            );
            for (text, rect, clip) in &texts {
                // The two halves of Follow system's example cut its texts, in English too.
                if section == Section::Appearance && clip.width() < EXAMPLE_WIDTH {
                    continue;
                }
                // Sideways: a long section scrolls downwards, as in English.
                assert!(
                    clip.left() - 1.0 <= rect.left() && rect.right() <= clip.right() + 1.0,
                    "{title}: {text:?} is cut: {rect:?} in {clip:?}"
                );
            }
            // The theme examples are pictures with small texts of their own.
            if section != Section::Appearance {
                for (i, (a, rect, _)) in texts.iter().enumerate() {
                    for (b, other, _) in &texts[i + 1..] {
                        let shared = rect.intersect(*other);
                        assert!(
                            shared.width() <= 1.0 || shared.height() <= 1.0,
                            "{title}: {a:?} lies over {b:?}"
                        );
                    }
                }
            }
        }
        std::fs::remove_dir_all(&config).unwrap();
    }

    fn collect(
        shape: egui::Shape,
        clip: egui::Rect,
        out: &mut Vec<(String, egui::Rect, egui::Rect)>,
    ) {
        match shape {
            egui::Shape::Text(t) => {
                let rect = t.galley.rect.translate(t.pos.to_vec2());
                out.push((t.galley.text().to_owned(), rect, clip));
            }
            egui::Shape::Vec(shapes) => shapes.into_iter().for_each(|s| collect(s, clip, out)),
            _ => {}
        }
    }
}
