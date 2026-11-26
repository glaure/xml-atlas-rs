//! Help > XML Atlas Help (F1) (WIN-4), and the help windows' state.
//!
//! The guide is `assets/help.md`, and `help.de.md` in German (LANG-16), built into the program
//! so it works offline. The two have the same sections in the same order. It uses a small
//! part of Markdown: `#` and `##` headings, `- ` list items, paragraphs, and `code`; a line
//! that starts with spaces continues the item or paragraph above it.

use egui::text::LayoutJob;
use egui::{FontId, RichText, TextFormat, Ui};

use crate::about::About;
use crate::lang::Lang;

const GUIDE: &str = include_str!("../assets/help.md");
const GUIDE_DE: &str = include_str!("../assets/help.de.md");

/// The guide window's size when there is room for it.
const GUIDE_SIZE: egui::Vec2 = egui::vec2(560.0, 620.0);

/// Space kept free around the guide window, for its frame and the program's edges.
const MARGIN: f32 = 24.0;

/// Which help windows are open.
#[derive(Default)]
pub struct Help {
    pub guide: bool,
    pub about: About,
}

/// A piece of the guide.
#[derive(Debug, PartialEq, Eq)]
enum Block {
    Title(String),
    Heading(String),
    Item(String),
    Paragraph(String),
}

/// Draws the guide while it is open; [`About`] shows itself.
pub fn show(ctx: &egui::Context, help: &mut Help, lang: Lang) {
    // Never larger than the program's window, so the guide wraps at the window's width and
    // scrolls, however many displays the desktop spans (#178).
    let room = ctx.content_rect().size() - egui::vec2(MARGIN, MARGIN);
    let size = egui::vec2(GUIDE_SIZE.x.min(room.x), GUIDE_SIZE.y.min(room.y));
    egui::Window::new(lang.menu_help_guide())
        .id(guide_id())
        .open(&mut help.guide)
        .default_size(size)
        .max_size(room)
        .vscroll(true)
        .show(ctx, |ui| guide(ui, guide_text(lang)));
}

/// The guide in the window's language. Every language names its guide here, the English one
/// where it has none of its own.
fn guide_text(lang: Lang) -> &'static str {
    match lang {
        Lang::En => GUIDE,
        Lang::De => GUIDE_DE,
    }
}

fn guide_id() -> egui::Id {
    egui::Id::unique("help-guide")
}

fn guide(ui: &mut Ui, text: &str) {
    for block in blocks(text) {
        match block {
            Block::Title(t) => {
                ui.heading(t);
            }
            Block::Heading(t) => {
                ui.add_space(10.0);
                ui.label(RichText::new(t).strong().size(16.0));
                ui.add_space(2.0);
            }
            Block::Item(t) => {
                ui.horizontal_top(|ui| {
                    ui.label("•");
                    inline(ui, &t);
                });
            }
            Block::Paragraph(t) => {
                ui.add_space(4.0);
                inline(ui, &t);
            }
        }
    }
}

/// Text with `code` in the monospace font, wrapped at the window's width.
fn inline(ui: &mut Ui, text: &str) {
    let body = FontId::proportional(14.0);
    let code = FontId::monospace(13.0);
    let colour = ui.visuals().text_color();
    let mut job = LayoutJob::default();
    for (i, part) in text.split('`').enumerate() {
        let font = if i % 2 == 1 { &code } else { &body };
        job.append(part, 0.0, TextFormat::simple(font.clone(), colour));
    }
    // A window sizes itself with `Extend`, which lays text out on one line; wrap it instead.
    ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Wrap));
}

/// The guide's text as headings, list items and paragraphs.
fn blocks(text: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut open = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            open = false;
        } else if let Some(t) = line.strip_prefix("## ") {
            out.push(Block::Heading(t.to_string()));
            open = false;
        } else if let Some(t) = line.strip_prefix("# ") {
            out.push(Block::Title(t.to_string()));
            open = false;
        } else if let Some(t) = line.strip_prefix("- ") {
            out.push(Block::Item(t.to_string()));
            open = true;
        } else if open && let Some(Block::Item(t) | Block::Paragraph(t)) = out.last_mut() {
            t.push(' ');
            t.push_str(trimmed);
        } else {
            out.push(Block::Paragraph(trimmed.to_string()));
            open = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_join_continued_lines() {
        let text = "# Title\n\nIntro that\ngoes on.\n\n## Part\n\n- One `code`\n  continued\n- Two\n\nAfter.\n";
        assert_eq!(
            blocks(text),
            [
                Block::Title("Title".into()),
                Block::Paragraph("Intro that goes on.".into()),
                Block::Heading("Part".into()),
                Block::Item("One `code` continued".into()),
                Block::Item("Two".into()),
                Block::Paragraph("After.".into()),
            ]
        );
    }

    /// Each guide starts with its title, every section has something in it, and no word is
    /// lost: every non-empty line of the file is in a block.
    #[test]
    fn the_guides_read_whole() {
        for (lang, title) in [
            (Lang::En, "XML Atlas Help"),
            (Lang::De, "Hilfe zu XML Atlas"),
        ] {
            let guide = guide_text(lang);
            let b = blocks(guide);
            assert_eq!(b[0], Block::Title(title.into()));
            for pair in b.windows(2) {
                if let Block::Heading(h) = &pair[0] {
                    assert!(
                        !matches!(pair[1], Block::Heading(_)),
                        "section {h} is empty"
                    );
                }
            }
            let words = |s: &str| s.split_whitespace().count();
            let in_blocks: usize = b.iter().map(|b| words(text(b))).sum();
            let in_file: usize = guide
                .lines()
                .map(|l| {
                    let l = l.trim_start();
                    let l = l
                        .strip_prefix("## ")
                        .or_else(|| l.strip_prefix("# "))
                        .or_else(|| l.strip_prefix("- "))
                        .unwrap_or(l);
                    words(l)
                })
                .sum();
            assert_eq!(in_blocks, in_file, "{lang:?}");
        }
    }

    /// LANG-16: the same sections in the same order, each with the same headings, list items
    /// and paragraphs, and in each the same `code`: expressions, patterns and commands are
    /// not translated.
    #[test]
    fn the_german_guide_follows_the_english_one() {
        let shape = |guide: &str| -> Vec<(std::mem::Discriminant<Block>, Vec<String>)> {
            blocks(guide)
                .iter()
                .map(|b| {
                    // A line break inside `code` joins with a space, as in the window.
                    let mut code: Vec<String> = text(b)
                        .split('`')
                        .skip(1)
                        .step_by(2)
                        .map(str::to_owned)
                        .collect();
                    // A sentence may name them in another order.
                    code.sort();
                    (std::mem::discriminant(b), code)
                })
                .collect()
        };
        let (mut english, german) = (shape(GUIDE), shape(GUIDE_DE));
        // One is a word the diagram shows, which the window translates.
        for code in english.iter_mut().flat_map(|(_, code)| code) {
            if code == Lang::En.diagram_text() {
                *code = Lang::De.diagram_text().to_owned();
            }
        }
        for (_, code) in &mut english {
            code.sort();
        }
        for (i, (en, de)) in english.iter().zip(&german).enumerate() {
            assert_eq!(en, de, "block {i}: {:?}", blocks(GUIDE_DE)[i]);
        }
        assert_eq!(english.len(), german.len());
    }

    /// LANG-16: menus, entries and keys are named as the German window shows them.
    #[test]
    fn the_german_guide_uses_the_windows_names() {
        let guide: Vec<_> = blocks(GUIDE_DE)
            .iter()
            .map(|b| text(b).to_owned())
            .collect();
        let guide = guide.join("\n");
        let de = Lang::De;
        for name in [
            de.menu_file(),
            de.menu_file_new(),
            de.menu_file_open(),
            de.menu_file_recent(),
            de.menu_file_settings(),
            de.menu_edit(),
            de.menu_edit_element(),
            de.menu_element_rename(),
            de.menu_element_delete(),
            de.menu_element_duplicate(),
            de.menu_element_move_up(),
            de.menu_element_move_down(),
            de.menu_view(),
            de.menu_view_layout(),
            de.menu_view_reset_layout(),
            de.menu_view_font(),
            de.menu_view_theme(),
            de.menu_navigate(),
            de.menu_navigate_drill_up(),
            de.menu_navigate_drill_down(),
            de.menu_tools(),
            de.menu_tools_format(),
            de.menu_tools_compact(),
            de.menu_tools_format_options(),
            de.menu_tools_references(),
            de.menu_help(),
            de.menu_help_guide(),
            de.menu_help_updates(),
            de.switch_toolbar(),
            de.switch_diagram(),
            de.switch_attributes(),
            de.switch_references(),
            de.switch_preview(),
            de.switch_preview_tree(),
            de.settings_appearance(),
            de.settings_localization(),
            de.settings_views(),
            de.settings_references(),
            de.settings_format(),
            de.settings_updates(),
            de.settings_agents(),
            de.settings_agents_allow(),
            de.rules_use_for(),
            de.rules_edit(),
            de.preview_edit(),
            de.editor_insert(),
            de.cell_copy_whole(),
            de.find_match_case(),
            de.find_whole_word(),
            de.find_replace_all(),
            de.xpath_stop(),
            de.install_start(),
            de.install_close(),
        ] {
            // The guide leaves out an entry's ellipsis, but for the update notice's buttons.
            let name = name.trim_end_matches('…');
            assert!(guide.contains(name), "the guide does not name {name}");
        }
        for english in [
            "Ctrl",
            "Shift",
            "Page Up",
            "Home",
            "Backspace",
            "Delete",
            "Enter",
        ] {
            assert!(!guide.contains(english), "{english} in the German guide");
        }
    }

    fn text(block: &Block) -> &str {
        match block {
            Block::Title(t) | Block::Heading(t) | Block::Item(t) | Block::Paragraph(t) => t,
        }
    }

    /// The guide fits inside a main window that is smaller than its default size (#178).
    #[test]
    fn the_guide_fits_a_small_window() {
        for (lang, w, h) in [
            (Lang::En, 900.0, 600.0),
            (Lang::En, 480.0, 360.0),
            (Lang::De, 900.0, 600.0),
            (Lang::De, 480.0, 360.0),
        ] {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
            let ctx = egui::Context::default();
            let mut help = Help {
                guide: true,
                ..Default::default()
            };
            for _ in 0..4 {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                ctx.run_pass(input, |ctx| show(ctx, &mut help, lang))
                    .drop_without_applying_deltas();
            }
            let rect = ctx
                .memory(|m| m.area_rect(guide_id()))
                .expect("the help window is open");
            assert!(
                screen.contains_rect(rect.shrink(0.5)),
                "{rect:?} is not inside {screen:?}"
            );
            let used = ctx.globally_used_rect();
            assert!(
                screen.contains_rect(used.shrink(0.5)),
                "content {used:?} is not inside {screen:?}"
            );
        }
    }
}
