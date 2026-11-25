//! Help > XML Atlas Help (F1) and Help > About XML Atlas (WIN-4, PKG-2).
//!
//! The guide is `assets/help.md`, built into the program so it works offline. It uses a small
//! part of Markdown: `#` and `##` headings, `- ` list items, paragraphs, and `code`; a line
//! that starts with spaces continues the item or paragraph above it.

use egui::text::LayoutJob;
use egui::{FontId, RichText, TextFormat, Ui};

const GUIDE: &str = include_str!("../assets/help.md");

const HOMEPAGE: &str = "https://github.com/glaure/xml-atlas-rs";

const RELEASES: &str = "https://github.com/glaure/xml-atlas-rs/releases";

/// The guide window's size when there is room for it.
const GUIDE_SIZE: egui::Vec2 = egui::vec2(560.0, 620.0);

/// Space kept free around the guide window, for its frame and the program's edges.
const MARGIN: f32 = 24.0;

/// Which help windows are open.
#[derive(Default)]
pub struct Help {
    pub guide: bool,
    pub about: bool,
}

/// A piece of the guide.
#[derive(Debug, PartialEq, Eq)]
enum Block {
    Title(String),
    Heading(String),
    Item(String),
    Paragraph(String),
}

/// Draws the open help windows.
pub fn show(ctx: &egui::Context, help: &mut Help) {
    // Never larger than the program's window, so the guide wraps at the window's width and
    // scrolls, however many displays the desktop spans (#178).
    let room = ctx.content_rect().size() - egui::vec2(MARGIN, MARGIN);
    let size = egui::vec2(GUIDE_SIZE.x.min(room.x), GUIDE_SIZE.y.min(room.y));
    egui::Window::new("XML Atlas Help")
        .id(guide_id())
        .open(&mut help.guide)
        .default_size(size)
        .max_size(room)
        .vscroll(true)
        .show(ctx, |ui| guide(ui, GUIDE));
    egui::Window::new("About XML Atlas")
        .open(&mut help.about)
        .collapsible(false)
        .resizable(false)
        .show(ctx, about);
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

fn about(ui: &mut Ui) {
    ui.heading("XML Atlas");
    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
    ui.add_space(6.0);
    ui.label("Fast viewer and editor for large XML files.");
    ui.label("Licensed under the MIT license or the Apache License 2.0, at your option.");
    ui.label(
        "The licenses of the libraries it contains are installed with it, in \
         THIRD-PARTY-LICENSES.txt.",
    );
    ui.add_space(6.0);
    ui.hyperlink_to("Project page", HOMEPAGE);
    ui.hyperlink_to("Releases", RELEASES);
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

    /// The guide starts with its title, every section has something in it, and no word is
    /// lost: every non-empty line of the file is in a block.
    #[test]
    fn the_guide_reads_whole() {
        let b = blocks(GUIDE);
        assert_eq!(b[0], Block::Title("XML Atlas Help".into()));
        for pair in b.windows(2) {
            if let Block::Heading(h) = &pair[0] {
                assert!(
                    !matches!(pair[1], Block::Heading(_)),
                    "section {h} is empty"
                );
            }
        }
        let words = |s: &str| s.split_whitespace().count();
        let in_blocks: usize = b
            .iter()
            .map(|b| match b {
                Block::Title(t) | Block::Heading(t) | Block::Item(t) | Block::Paragraph(t) => {
                    words(t)
                }
            })
            .sum();
        let in_file: usize = GUIDE
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
        assert_eq!(in_blocks, in_file);
    }

    /// The guide fits inside a main window that is smaller than its default size (#178).
    #[test]
    fn the_guide_fits_a_small_window() {
        for (w, h) in [(900.0, 600.0), (480.0, 360.0)] {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
            let ctx = egui::Context::default();
            let mut help = Help {
                guide: true,
                about: false,
            };
            for _ in 0..4 {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                ctx.run_pass(input, |ctx| show(ctx, &mut help))
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
