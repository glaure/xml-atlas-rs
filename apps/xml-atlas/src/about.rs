//! Help > About XML Atlas (WIN-4, PKG-2): a centred card with the logo, the version, what
//! the program is, links and its licenses.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, OpenUrl, Painter, Pos2, Rect, RichText,
    Sense, Stroke, Ui, Vec2, vec2,
};

use crate::lang::Lang;
use crate::update;

const HOMEPAGE: &str = "https://github.com/glaure/xml-atlas-rs";

const RELEASES: &str = "https://github.com/glaure/xml-atlas-rs/releases";

/// Width of the card's content.
const WIDTH: f32 = 360.0;
/// The logo's side.
const LOGO_SIZE: f32 = 96.0;
/// Space kept free around the card, for the window's title and the program's edges.
const MARGIN: f32 = 64.0;

/// The icon's colours, as in `packaging/linux/io.github.glaure.XmlAtlas.svg`.
const LOGO_BLUE: Color32 = Color32::from_rgb(0x25, 0x63, 0xa6);
const LOGO_ORANGE: Color32 = Color32::from_rgb(0xf5, 0xb0, 0x41);

/// What the card asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// As Help > Check for updates now (PKG-7).
    CheckForUpdates,
}

/// The About window.
#[derive(Default)]
pub struct About {
    pub open: bool,
    /// The version info went to the clipboard since the window was opened.
    copied: bool,
}

impl About {
    /// Shows the window while it is open. `checking`: an update check is under way.
    pub fn show(&mut self, ctx: &egui::Context, lang: Lang, checking: bool) -> Option<Request> {
        if !self.open {
            self.copied = false;
            return None;
        }
        let mut open = true;
        let mut request = None;
        let screen = ctx.content_rect();
        egui::Window::new(lang.about_title())
            .id(window_id())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(screen.center())
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(screen.height() - MARGIN)
                    .show(ui, |ui| request = self.card(ui, lang, checking));
            });
        self.open = open;
        request
    }

    fn card(&mut self, ui: &mut Ui, lang: Lang, checking: bool) -> Option<Request> {
        let mut request = None;
        ui.set_width(WIDTH);
        ui.vertical_centered(|ui| {
            ui.add_space(12.0);
            logo(ui);
            ui.add_space(10.0);
            ui.label(RichText::new("XML Atlas").size(28.0).strong());
            ui.add_space(4.0);
            badge(ui, &lang.about_version(update::CURRENT));
            ui.add_space(12.0);
            ui.add(egui::Label::new(lang.about_tagline()).wrap());
            ui.add_space(14.0);

            let links = [
                (lang.about_project_page(), HOMEPAGE),
                (lang.about_releases(), RELEASES),
            ];
            centred_row(ui, &links.map(|(label, _)| label), |ui| {
                for (label, url) in links {
                    let link = ui
                        .button(label)
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text(url);
                    if link.clicked() {
                        ui.ctx().open_url(OpenUrl::new_tab(url));
                    }
                }
            });
            ui.add_space(6.0);

            let copy = if self.copied {
                lang.about_copied()
            } else {
                lang.about_copy()
            };
            let check = lang.menu_help_updates();
            centred_row(ui, &[copy, check], |ui| {
                let info = version_info();
                if ui.button(copy).on_hover_text(&info).clicked() {
                    ui.ctx().copy_text(info);
                    self.copied = true;
                }
                // What it finds is told in the status bar, as from the Help menu.
                if ui
                    .add_enabled(!checking, egui::Button::new(check))
                    .on_disabled_hover_text(lang.why_check_running())
                    .clicked()
                {
                    request = Some(Request::CheckForUpdates);
                }
            });

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(6.0);
            for text in [lang.about_license(), lang.about_library_licenses()] {
                ui.add(egui::Label::new(RichText::new(text).small().weak()).wrap());
            }
            ui.add_space(2.0);
        });
        request
    }
}

fn window_id() -> egui::Id {
    egui::Id::unique("help-about")
}

/// What *Copy version info* gives, for a bug report; the same in every language.
fn version_info() -> String {
    format!(
        "XML Atlas {} ({} {})",
        update::CURRENT,
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// A row of buttons with these labels in the middle: egui centres single widgets only, so
/// the room on the left is worked out from the labels.
fn centred_row(ui: &mut Ui, labels: &[&str], add: impl FnOnce(&mut Ui)) {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let spacing = ui.spacing();
    let (padding, gap) = (spacing.button_padding.x, spacing.item_spacing.x);
    let buttons: f32 = labels
        .iter()
        .map(|label| text_size(ui, label, font.clone()).x + 2.0 * padding)
        .sum();
    let width = buttons + gap * labels.len().saturating_sub(1) as f32;
    let room = ui.available_width();
    ui.horizontal(|ui| {
        ui.add_space(((room - width) / 2.0).max(0.0));
        add(ui);
    });
}

fn text_size(ui: &Ui, text: &str, font: FontId) -> Vec2 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
        .size()
}

/// The version in a rounded pill.
fn badge(ui: &mut Ui, text: &str) {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let colour = ui.visuals().text_color();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, colour);
    let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(24.0, 6.0), Sense::hover());
    let radius = CornerRadius::same(u8::MAX);
    ui.painter()
        .rect_filled(rect, radius, ui.visuals().widgets.inactive.weak_bg_fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
}

/// The program's icon, drawn from the shapes of its SVG so that it is sharp at every scale.
fn logo(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(LOGO_SIZE), Sense::hover());
    paint_logo(ui.painter(), rect);
}

fn paint_logo(painter: &Painter, rect: Rect) {
    // The SVG's units: 128 to a side.
    let scale = rect.width() / 128.0;
    let at = |x: f32, y: f32| -> Pos2 { rect.min + vec2(x, y) * scale };
    let area = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_max(at(x, y), at(x + w, y + h));
    let rounded = |r: f32| CornerRadius::same((r * scale).round() as u8);
    // Round ends and joins: a dot on every point.
    let line = |points: &[(f32, f32)], width: f32, colour: Color32| {
        let points: Vec<Pos2> = points.iter().map(|&(x, y)| at(x, y)).collect();
        for &point in &points {
            painter.circle_filled(point, width * scale / 2.0, colour);
        }
        painter.line(points, Stroke::new(width * scale, colour));
    };

    painter.rect_filled(area(8.0, 8.0, 112.0, 112.0), rounded(22.0), LOGO_BLUE);
    let white = Color32::WHITE;
    line(&[(36.0, 40.0), (20.0, 64.0), (36.0, 88.0)], 9.0, white);
    line(&[(92.0, 40.0), (108.0, 64.0), (92.0, 88.0)], 9.0, white);
    line(
        &[(54.0, 46.0), (54.0, 82.0), (70.0, 82.0)],
        5.0,
        LOGO_ORANGE,
    );
    line(&[(54.0, 64.0), (70.0, 64.0)], 5.0, LOGO_ORANGE);
    for (x, y) in [(46.0, 34.0), (68.0, 57.0), (68.0, 75.0)] {
        painter.rect_filled(area(x, y, 16.0, 14.0), rounded(3.0), LOGO_ORANGE);
    }
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, OutputCommand, PointerButton, RawInput, Shape, pos2};

    use super::*;

    const SCREEN: Vec2 = vec2(1280.0, 820.0);

    /// A drawn text, where it is and what it is clipped to.
    type Text = (String, Rect, Rect);

    struct Card {
        ctx: Context,
        about: About,
        lang: Lang,
        checking: bool,
        screen: Vec2,
        texts: Vec<Text>,
        commands: Vec<OutputCommand>,
        requests: Vec<Request>,
    }

    impl Card {
        fn new(lang: Lang) -> Self {
            let mut card = Card {
                ctx: Context::default(),
                about: About {
                    open: true,
                    ..Default::default()
                },
                lang,
                checking: false,
                screen: SCREEN,
                texts: Vec::new(),
                commands: Vec::new(),
                requests: Vec::new(),
            };
            card.settle();
            card
        }

        /// A window learns its size in its first frames.
        fn settle(&mut self) {
            for _ in 0..4 {
                self.frame(Vec::new());
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.screen)),
                events,
                ..Default::default()
            };
            let (about, lang, checking) = (&mut self.about, self.lang, self.checking);
            let requests = &mut self.requests;
            let mut out = self.ctx.run_ui(input, |ui| {
                requests.extend(about.show(ui.ctx(), lang, checking));
            });
            out.textures_delta.clear();
            self.commands.extend(out.platform_output.commands);
            self.texts.clear();
            for clipped in out.shapes {
                collect(clipped.shape, clipped.clip_rect, &mut self.texts);
            }
        }

        fn rect_of(&self, text: &str) -> Rect {
            self.texts
                .iter()
                .find(|(t, ..)| t == text)
                .unwrap_or_else(|| panic!("no {text:?} in {:?}", self.texts))
                .1
        }

        fn click(&mut self, text: &str) {
            let pos = self.rect_of(text).center();
            self.frame(vec![Event::PointerMoved(pos)]);
            for pressed in [true, false] {
                self.frame(vec![Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }]);
            }
            self.frame(vec![Event::PointerGone]);
        }
    }

    fn collect(shape: Shape, clip: Rect, out: &mut Vec<Text>) {
        match shape {
            Shape::Text(t) => {
                let rect = t.galley.rect.translate(t.pos.to_vec2());
                out.push((t.galley.text().to_owned(), rect, clip));
            }
            Shape::Vec(shapes) => shapes.into_iter().for_each(|s| collect(s, clip, out)),
            _ => {}
        }
    }

    /// WIN-4 and LANG-15: all of the card is there in both languages, whole, nothing over
    /// anything else, and in the middle of the window.
    #[test]
    fn the_card_is_whole_in_both_languages() {
        for (lang, title, buttons) in [
            (
                Lang::En,
                "About XML Atlas",
                [
                    "Project page",
                    "Releases",
                    "Copy version info",
                    "Check for updates now",
                ],
            ),
            (
                Lang::De,
                "Über XML Atlas",
                [
                    "Projektseite",
                    "Versionen",
                    "Versionsinfo kopieren",
                    "Jetzt nach Updates suchen",
                ],
            ),
        ] {
            let card = Card::new(lang);
            for text in [title, "XML Atlas", &lang.about_version(update::CURRENT)] {
                card.rect_of(text);
            }
            for text in [lang.about_tagline(), lang.about_license()] {
                card.rect_of(text);
            }
            card.rect_of(lang.about_library_licenses());
            let window = card
                .ctx
                .memory(|m| m.area_rect(window_id()))
                .expect("the window was drawn");
            let screen = Rect::from_min_size(Pos2::ZERO, SCREEN);
            assert!(screen.contains_rect(window), "{window:?}");
            assert!(
                window.center().distance(screen.center()) < 2.0,
                "{window:?}"
            );
            for (i, (text, rect, clip)) in card.texts.iter().enumerate() {
                assert!(clip.expand(1.0).contains_rect(*rect), "{text:?} is cut");
                for (other, o, _) in &card.texts[i + 1..] {
                    let shared = rect.intersect(*o);
                    assert!(
                        shared.width() <= 1.0 || shared.height() <= 1.0,
                        "{text:?} lies over {other:?}"
                    );
                }
            }
            // Each pair of buttons sits in the middle.
            for pair in buttons.chunks(2) {
                let row = card.rect_of(pair[0]).union(card.rect_of(pair[1]));
                assert!(
                    (row.center().x - window.center().x).abs() < 2.0,
                    "{pair:?}: {row:?} in {window:?}"
                );
            }
        }
    }

    #[test]
    fn the_links_open_their_pages() {
        let mut card = Card::new(Lang::En);
        for (label, url) in [("Project page", HOMEPAGE), ("Releases", RELEASES)] {
            card.commands.clear();
            card.click(label);
            assert!(
                card.commands
                    .iter()
                    .any(|c| matches!(c, OutputCommand::OpenUrl(o) if o.url == url)),
                "{label}: {:?}",
                card.commands
            );
        }
        assert!(card.requests.is_empty());
    }

    #[test]
    fn copy_gives_the_version_and_the_system_and_says_so() {
        let mut card = Card::new(Lang::De);
        card.click("Versionsinfo kopieren");
        let expected = format!(
            "XML Atlas {} ({} {})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        assert!(
            card.commands
                .contains(&OutputCommand::CopyText(expected.clone())),
            "{:?}",
            card.commands
        );
        card.settle();
        card.rect_of("Kopiert");
        // Closed and opened again, the button offers to copy again.
        card.about.open = false;
        card.frame(Vec::new());
        card.about.open = true;
        card.settle();
        card.rect_of("Versionsinfo kopieren");
    }

    #[test]
    fn check_for_updates_asks_the_app_unless_a_check_runs() {
        let mut card = Card::new(Lang::En);
        card.click("Check for updates now");
        assert_eq!(card.requests, [Request::CheckForUpdates]);
        card.requests.clear();
        card.checking = true;
        card.settle();
        card.click("Check for updates now");
        assert!(card.requests.is_empty());
    }

    /// In a window too low for it, the card stays inside and scrolls.
    #[test]
    fn the_card_stays_inside_a_low_window() {
        let mut card = Card::new(Lang::En);
        card.screen = vec2(640.0, 300.0);
        card.ctx = Context::default();
        card.settle();
        let window = card
            .ctx
            .memory(|m| m.area_rect(window_id()))
            .expect("the window was drawn");
        let screen = Rect::from_min_size(pos2(0.0, 0.0), card.screen);
        assert!(
            screen.expand(1.0).contains_rect(window),
            "{window:?} in {screen:?}"
        );
    }
}
