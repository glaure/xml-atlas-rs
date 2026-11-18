//! Every colour the window uses, in one place (D29).
//!
//! A [`Theme`] holds egui's [`Visuals`] (panels, text, widgets, selection, links, warning and
//! error, corner rounding) and the colours the views paint themselves: the syntax colours of
//! the text (TEXT-2), the selected element and the marked item (TEXT-6, TEXT-11), the text
//! selection, the grid's hatching (GRID-13) and the views' background. The tree's icons
//! (TREE-11) use the syntax colours.
//!
//! Light comes in two styles (WIN-8): Soft, the default, with greys that set the views apart
//! from the menus, and Classic, egui's own as in 1.0. Dark is egui's own.
//!
//! [`install`] hands a light and a dark theme to egui once; egui shows the one the mode picks
//! (WIN-2), and each view takes the shown one with [`current`] once a frame.

use std::sync::Arc;

use egui::{Color32, Context, Frame, Id, Stroke, Style, Ui, Visuals};

use crate::settings::LightStyle;

/// How much of the selection colour fills the selected element (TEXT-6).
const SELECTED_ELEMENT_ALPHA: f32 = 0.35;
/// How much of the selection colour fills the marked item inside it (TEXT-11).
const MARKED_ITEM_ALPHA: f32 = 0.7;
/// How much of the selection's text colour outlines the marked item.
const MARKED_ITEM_OUTLINE_ALPHA: f32 = 0.8;
/// How much of the weak text colour draws the hatching of absent cells (GRID-13).
const HATCH_ALPHA: f32 = 0.35;

/// The colours of the XML syntax in the text view (TEXT-2) and of the tree's icons (TREE-11).
#[derive(Clone, Debug, PartialEq)]
pub struct Syntax {
    pub text: Color32,
    pub tag: Color32,
    pub attribute_name: Color32,
    pub attribute_value: Color32,
    pub comment: Color32,
    pub punctuation: Color32,
}

/// One light or dark set of every colour the window uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// egui's own: panels, text, widgets, selection, links, warning and error, rounding.
    pub visuals: Visuals,
    pub syntax: Syntax,
    /// Fill behind the selected element's extent in the text (TEXT-6).
    pub selected_element: Color32,
    /// Fill and outline of what was clicked in the grid or tree, inside it (TEXT-11).
    pub marked_item: Color32,
    pub marked_item_outline: Color32,
    /// Fill behind the selected text.
    pub text_selection: Color32,
    /// Lines over a grid cell whose row has no such attribute, child or text (GRID-13).
    pub hatch: Color32,
    /// Fill of the tree, grid, cell pane and text; the menus, toolbars and status bar have
    /// the visuals' `panel_fill`.
    pub view_background: Color32,
}

/// The greys of a softer light theme (WIN-8): chrome around the views, the views themselves,
/// and what sits on them.
struct LightShades {
    /// Menus, toolbars, status bar, dialogs.
    chrome: Color32,
    /// Tree, grid, cell pane and text.
    view: Color32,
    /// Every other grid row.
    stripes: Color32,
    /// Text fields and scroll bar tracks.
    field: Color32,
    /// Separators and panel borders.
    separator: Color32,
    /// Buttons at rest, under the pointer, and pressed.
    button: Color32,
    button_hover: Color32,
    button_active: Color32,
    /// Text in menus, labels and the tree.
    text: Color32,
}

impl Theme {
    /// Dark: egui's dark visuals and the syntax colours used since 0.1.
    pub fn dark() -> Self {
        Self::from_visuals(
            Visuals::dark(),
            Syntax {
                text: Color32::from_rgb(0xd4, 0xd4, 0xd4),
                tag: Color32::from_rgb(0x56, 0x9c, 0xd6),
                attribute_name: Color32::from_rgb(0x9c, 0xdc, 0xfe),
                attribute_value: Color32::from_rgb(0xce, 0x91, 0x78),
                comment: Color32::from_rgb(0x6a, 0x99, 0x55),
                punctuation: Color32::from_rgb(0x80, 0x80, 0x80),
            },
        )
    }

    /// Light as in 1.0 (WIN-8): egui's light visuals and the syntax colours used since 0.1.
    pub fn light_classic() -> Self {
        Self::from_visuals(
            Visuals::light(),
            Syntax {
                text: Color32::from_rgb(0x1f, 0x1f, 0x1f),
                tag: Color32::from_rgb(0x00, 0x00, 0xb0),
                attribute_name: Color32::from_rgb(0xa0, 0x30, 0x00),
                attribute_value: Color32::from_rgb(0x00, 0x70, 0x30),
                comment: Color32::from_rgb(0x70, 0x70, 0x70),
                punctuation: Color32::from_rgb(0x60, 0x60, 0x60),
            },
        )
    }

    /// Light, the default (WIN-8): menus and toolbars in a light grey, the views a little
    /// lighter, darker separators and text, so the views stand apart and the window glares less.
    pub fn light() -> Self {
        let gray = Color32::from_gray;
        Self::soft_light(LightShades {
            chrome: gray(232),
            view: gray(247),
            stripes: gray(240),
            field: gray(252),
            separator: gray(182),
            button: gray(218),
            button_hover: gray(206),
            button_active: gray(180),
            text: gray(45),
        })
    }

    /// The built-in light theme in `style`.
    pub fn light_in(style: LightStyle) -> Self {
        match style {
            LightStyle::Soft => Self::light(),
            LightStyle::Classic => Self::light_classic(),
        }
    }

    /// The default built-in theme for dark or light.
    pub fn built_in(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    /// A light theme with `shades` in place of egui's near-white greys, and the syntax colours
    /// of [`Theme::light_classic`].
    fn soft_light(shades: LightShades) -> Self {
        let mut v = Visuals::light();
        v.panel_fill = shades.chrome;
        v.window_fill = shades.chrome;
        v.window_stroke.color = shades.separator;
        v.faint_bg_color = shades.stripes;
        v.extreme_bg_color = shades.field;
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = shades.chrome;
        w.noninteractive.weak_bg_fill = shades.chrome;
        w.noninteractive.bg_stroke.color = shades.separator;
        w.noninteractive.fg_stroke = Stroke::new(1.0, shades.text);
        for (state, fill) in [
            (&mut w.inactive, shades.button),
            (&mut w.hovered, shades.button_hover),
            (&mut w.open, shades.button_hover),
            (&mut w.active, shades.button_active),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
        }
        let mut theme = Self::from_visuals(v, Self::light_classic().syntax);
        theme.view_background = shades.view;
        theme
    }

    /// The frame of a view's panel: `central` for a central panel, otherwise a side, top or
    /// bottom one, filled with the view background.
    pub fn view_frame(&self, style: &Style, central: bool) -> Frame {
        let frame = if central {
            Frame::central_panel(style)
        } else {
            Frame::side_top_panel(style)
        };
        frame.fill(self.view_background)
    }

    /// The view colours that follow from `visuals`' selection and weak text.
    fn from_visuals(visuals: Visuals, syntax: Syntax) -> Self {
        let selection = visuals.selection;
        Self {
            selected_element: selection.bg_fill.gamma_multiply(SELECTED_ELEMENT_ALPHA),
            marked_item: selection.bg_fill.gamma_multiply(MARKED_ITEM_ALPHA),
            marked_item_outline: selection
                .stroke
                .color
                .gamma_multiply(MARKED_ITEM_OUTLINE_ALPHA),
            text_selection: selection.bg_fill,
            hatch: visuals.weak_text_color().gamma_multiply(HATCH_ALPHA),
            view_background: visuals.panel_fill,
            visuals,
            syntax,
        }
    }
}

/// Hands `light` and `dark` to egui, which shows the one the mode picks (WIN-2). Called when
/// the themes change, not every frame.
pub fn install(ctx: &Context, light: Theme, dark: Theme) {
    ctx.set_visuals_of(egui::Theme::Light, light.visuals.clone());
    ctx.set_visuals_of(egui::Theme::Dark, dark.visuals.clone());
    ctx.data_mut(|d| {
        d.insert_temp(key(false), Arc::new(light));
        d.insert_temp(key(true), Arc::new(dark));
    });
}

/// The theme shown in `ui`: the installed one for its light or dark visuals, or the built-in
/// one before [`install`].
pub fn current(ui: &Ui) -> Arc<Theme> {
    let dark = ui.visuals().dark_mode;
    ui.data(|d| d.get_temp(key(dark)))
        .unwrap_or_else(|| Arc::new(Theme::built_in(dark)))
}

/// Where [`install`] keeps the light or dark theme in egui's memory.
fn key(dark: bool) -> Id {
    Id::unique(("xml-atlas-theme", dark))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG's minimum contrast for text, and for weak text (as for large text).
    const TEXT_CONTRAST: f64 = 4.5;
    const WEAK_CONTRAST: f64 = 3.0;

    /// `fg` drawn over the opaque `bg`, blended as egui blends its premultiplied colours.
    fn over(fg: Color32, bg: Color32) -> Color32 {
        let rest = 255 - u32::from(fg.a());
        let mix = |f: u8, b: u8| (u32::from(f) + u32::from(b) * rest / 255).min(255) as u8;
        Color32::from_rgb(
            mix(fg.r(), bg.r()),
            mix(fg.g(), bg.g()),
            mix(fg.b(), bg.b()),
        )
    }

    /// WCAG's relative luminance of an opaque sRGB colour.
    fn luminance(c: Color32) -> f64 {
        let channel = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    /// WCAG's contrast ratio of `fg` drawn on `bg`.
    fn contrast(fg: Color32, bg: Color32) -> f64 {
        let (a, b) = (luminance(over(fg, bg)), luminance(bg));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Every colour of text in `theme` that is too faint on what it is drawn on, with its
    /// contrast.
    fn faint(theme: &Theme) -> Vec<(&'static str, f64)> {
        let v = &theme.visuals;
        let s = &theme.syntax;
        let view = theme.view_background;
        let checks = [
            ("text on view", v.text_color(), view, TEXT_CONTRAST),
            (
                "weak text on view",
                v.weak_text_color(),
                view,
                WEAK_CONTRAST,
            ),
            (
                "text on chrome",
                v.text_color(),
                v.panel_fill,
                TEXT_CONTRAST,
            ),
            (
                "weak text on chrome",
                v.weak_text_color(),
                v.panel_fill,
                WEAK_CONTRAST,
            ),
            (
                "selected text",
                v.selection.stroke.color,
                v.selection.bg_fill,
                TEXT_CONTRAST,
            ),
            ("syntax text", s.text, view, TEXT_CONTRAST),
            ("tag", s.tag, view, TEXT_CONTRAST),
            ("attribute name", s.attribute_name, view, TEXT_CONTRAST),
            ("attribute value", s.attribute_value, view, TEXT_CONTRAST),
            ("comment", s.comment, view, TEXT_CONTRAST),
            ("punctuation", s.punctuation, view, TEXT_CONTRAST),
        ];
        checks
            .into_iter()
            .map(|(name, fg, bg, least)| (name, contrast(fg, bg), least))
            .filter(|(_, c, least)| c < least)
            .map(|(name, c, _)| (name, c))
            .collect()
    }

    /// Colours kept as they were in 1.0 although they miss the contrast (owner, 2026-11-18,
    /// D29): Dark stays as it is, and Classic is 1.0's Light.
    const EXEMPT: [(&str, &str); 5] = [
        ("dark", "weak text on view"),
        ("dark", "weak text on chrome"),
        ("dark", "punctuation"),
        ("light classic", "weak text on view"),
        ("light classic", "weak text on chrome"),
    ];

    /// Text, syntax colours and the selection can be read on their backgrounds: 4.5:1, and
    /// 3:1 for weak text (WIN-7).
    #[test]
    fn themes_have_enough_contrast() {
        let themes = [
            ("dark", Theme::dark()),
            ("light", Theme::light()),
            ("light classic", Theme::light_classic()),
        ];
        let mut failed = Vec::new();
        for (name, theme) in &themes {
            for (what, c) in faint(theme) {
                if !EXEMPT.contains(&(*name, what)) {
                    failed.push(format!("{name}: {what} {c:.2}"));
                }
            }
        }
        assert!(failed.is_empty(), "too faint: {failed:#?}");
    }

    /// Dark and Classic are egui's visuals, so they look as in 1.0.
    #[test]
    fn built_in_themes_keep_eguis_visuals() {
        assert_eq!(Theme::dark().visuals, Visuals::dark());
        assert_eq!(Theme::light_classic().visuals, Visuals::light());
        assert!(Theme::built_in(true).visuals.dark_mode);
        assert!(!Theme::built_in(false).visuals.dark_mode);
    }

    /// Soft Light gives the views their own background, darker separators and darker text;
    /// Dark and Classic keep one background for everything, as in 1.0 (WIN-8).
    #[test]
    fn soft_light_sets_the_views_apart() {
        let soft = Theme::light();
        let classic = Theme::light_classic();
        assert!(!soft.visuals.dark_mode);
        assert_ne!(soft.view_background, soft.visuals.panel_fill);
        assert!(luminance(soft.view_background) > luminance(soft.visuals.panel_fill));
        assert!(
            luminance(soft.visuals.widgets.noninteractive.bg_stroke.color)
                < luminance(classic.visuals.widgets.noninteractive.bg_stroke.color)
        );
        assert!(luminance(soft.visuals.text_color()) < luminance(classic.visuals.text_color()));
        assert_eq!(Theme::light_in(LightStyle::Soft), soft);
        assert_eq!(Theme::light_in(LightStyle::Classic), classic);
        assert_eq!(LightStyle::default(), LightStyle::Soft);
        for theme in [Theme::dark(), Theme::light_classic()] {
            assert_eq!(theme.view_background, theme.visuals.panel_fill);
        }
    }

    /// The view colours are the ones the views computed themselves before D29.
    #[test]
    fn view_colours_are_as_before() {
        for (theme, v) in [
            (Theme::dark(), Visuals::dark()),
            (Theme::light_classic(), Visuals::light()),
        ] {
            assert_eq!(
                theme.selected_element,
                v.selection.bg_fill.gamma_multiply(0.35)
            );
            assert_eq!(theme.marked_item, v.selection.bg_fill.gamma_multiply(0.7));
            assert_eq!(
                theme.marked_item_outline,
                v.selection.stroke.color.gamma_multiply(0.8)
            );
            assert_eq!(theme.text_selection, v.selection.bg_fill);
            assert_eq!(theme.hatch, v.weak_text_color().gamma_multiply(0.35));
        }
    }

    /// Views find the installed theme for the mode egui shows, and the built-in one before.
    #[test]
    fn views_find_the_installed_theme() {
        let ctx = Context::default();
        let mut light = Theme::light();
        light.syntax.tag = Color32::from_rgb(1, 2, 3);
        let mut dark = Theme::dark();
        dark.syntax.tag = Color32::from_rgb(4, 5, 6);
        let mut seen = Vec::new();
        for theme in [egui::ThemePreference::Light, egui::ThemePreference::Dark] {
            ctx.set_theme(theme);
            let mut out = ctx.run_ui(Default::default(), |ui| seen.push(current(ui).syntax.tag));
            out.textures_delta.clear();
        }
        assert_eq!(seen, [Theme::light().syntax.tag, Theme::dark().syntax.tag]);

        install(&ctx, light, dark);
        seen.clear();
        for theme in [egui::ThemePreference::Light, egui::ThemePreference::Dark] {
            ctx.set_theme(theme);
            let mut out = ctx.run_ui(Default::default(), |ui| seen.push(current(ui).syntax.tag));
            out.textures_delta.clear();
        }
        assert_eq!(
            seen,
            [Color32::from_rgb(1, 2, 3), Color32::from_rgb(4, 5, 6)]
        );
    }
}
