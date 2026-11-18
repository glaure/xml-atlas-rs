//! Every colour the window uses, in one place (D29, step 1).
//!
//! A [`Theme`] holds egui's [`Visuals`] (panels, text, widgets, selection, links, warning and
//! error, corner rounding) and the colours the views paint themselves: the syntax colours of
//! the text (TEXT-2), the selected element and the marked item (TEXT-6, TEXT-11), the text
//! selection and the grid's hatching (GRID-13). The tree's icons (TREE-11) use the syntax
//! colours.
//!
//! [`install`] hands a light and a dark theme to egui once; egui shows the one the mode picks
//! (WIN-2), and each view takes the shown one with [`current`] once a frame.

use std::sync::Arc;

use egui::{Color32, Context, Id, Ui, Visuals};

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

    /// Light: egui's light visuals and the syntax colours used since 0.1.
    pub fn light() -> Self {
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

    /// The built-in theme for dark or light.
    pub fn built_in(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
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

    /// The built-in themes are egui's visuals, so the window looks as before D29.
    #[test]
    fn built_in_themes_keep_eguis_visuals() {
        assert_eq!(Theme::dark().visuals, Visuals::dark());
        assert_eq!(Theme::light().visuals, Visuals::light());
        assert!(Theme::built_in(true).visuals.dark_mode);
        assert!(!Theme::built_in(false).visuals.dark_mode);
    }

    /// The view colours are the ones the views computed themselves before D29.
    #[test]
    fn view_colours_are_as_before() {
        for (theme, v) in [
            (Theme::dark(), Visuals::dark()),
            (Theme::light(), Visuals::light()),
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
