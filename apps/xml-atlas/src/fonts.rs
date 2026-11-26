//! The font of the views (TEXT-25): View > Font… chooses a monospaced font installed on the
//! system, its face and its size, for the text, the tree and the grid; menus and dialogs
//! keep egui's font.
//!
//! Only monospaced fonts can be chosen: the text view places every character at a multiple of
//! one character's width. The system's fonts are listed with `fontique` and checked with
//! `skrifa`, both already in the build through egui.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use egui::{FontData, FontDefinitions, FontFamily, FontId, RichText, Ui};
use fontique::{Collection, CollectionOptions, FontInfo, FontStyle, SourceKind};
use skrifa::MetadataProvider;
use skrifa::instance::{LocationRef, Size};
use skrifa::string::StringId;

use crate::lang::Lang;

/// The text view's size when none is chosen, in points.
pub const DEFAULT_SIZE: u8 = 13;

/// The sizes that can be chosen, in points.
pub const SIZES: std::ops::RangeInclusive<u8> = 8..=40;

/// The name of the chosen font in egui's font definitions, and of the family that holds it.
const FAMILY: &str = "xml-atlas-text";

/// The characters whose widths must all be the same for a font to be monospaced.
const PROBE: [char; 6] = ['i', 'M', '0', '<', '"', 'W'];

/// A font's face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Face {
    #[default]
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

/// The text view's font, as chosen and remembered (WIN-3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFont {
    /// A system font's family; `None` is egui's built-in monospaced font, Hack.
    pub family: Option<String>,
    pub face: Face,
    /// In points.
    pub size: u8,
    /// Where the family's font for `face` was found last, and its index in the file: looking
    /// it up among the system's fonts takes about 50 ms, reading the file a few. Checked
    /// against `family` before it is used, and found again when it does not fit.
    pub file: Option<(PathBuf, u32)>,
}

/// A monospaced family on this system, with the faces it has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Family {
    pub name: String,
    pub faces: Vec<Face>,
}

/// View > Font…: the font being chosen, and the system's monospaced families once found.
pub struct FontDialog {
    draft: TextFont,
    families: Option<Vec<Family>>,
    rx: Option<Receiver<Vec<Family>>>,
}

/// Why a font cannot be used.
#[derive(Debug, PartialEq, Eq)]
pub enum FontError {
    NotFound(String),
    NotMonospaced(String),
}

impl FontError {
    pub fn text(&self, lang: Lang) -> String {
        match self {
            FontError::NotFound(name) => lang.font_not_found(name),
            FontError::NotMonospaced(name) => lang.font_not_monospaced(name),
        }
    }
}

/// What the dialog's buttons asked for.
pub enum Answer {
    Use(TextFont),
    Cancel,
}

impl Face {
    pub const ALL: [Face; 4] = [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic];

    pub fn name(self) -> &'static str {
        match self {
            Face::Regular => "regular",
            Face::Bold => "bold",
            Face::Italic => "italic",
            Face::BoldItalic => "bold-italic",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == name)
    }

    fn label(self, lang: Lang) -> &'static str {
        match self {
            Face::Regular => lang.font_regular(),
            Face::Bold => lang.font_bold(),
            Face::Italic => lang.font_italic(),
            Face::BoldItalic => lang.font_bold_italic(),
        }
    }

    fn bold(self) -> bool {
        matches!(self, Face::Bold | Face::BoldItalic)
    }

    fn italic(self) -> bool {
        matches!(self, Face::Italic | Face::BoldItalic)
    }

    /// The face a system font is, when it is one of the four.
    fn of(font: &FontInfo) -> Option<Self> {
        let weight = font.weight().value();
        let bold = if (350.0..=500.0).contains(&weight) {
            false
        } else if (600.0..=800.0).contains(&weight) {
            true
        } else {
            return None;
        };
        let italic = !matches!(font.style(), FontStyle::Normal);
        Some(match (bold, italic) {
            (false, false) => Face::Regular,
            (true, false) => Face::Bold,
            (false, true) => Face::Italic,
            (true, true) => Face::BoldItalic,
        })
    }
}

impl Default for TextFont {
    fn default() -> Self {
        Self {
            family: None,
            face: Face::Regular,
            size: DEFAULT_SIZE,
            file: None,
        }
    }
}

impl TextFont {
    /// The font of the tree, the grid and the cell pane: the chosen one; with the built-in
    /// font, egui's proportional one at the chosen size. `None` while nothing is chosen, so
    /// they keep the sizes they have by default.
    pub fn view_font_id(&self) -> Option<FontId> {
        if *self == TextFont::default() {
            return None;
        }
        let mut font = self.font_id();
        if self.family.is_none() {
            font.family = FontFamily::Proportional;
        }
        Some(font)
    }

    /// The font the text view lays out with.
    pub fn font_id(&self) -> FontId {
        let family = match self.family {
            Some(_) => FontFamily::Name(FAMILY.into()),
            None => FontFamily::Monospace,
        };
        let size = self.size.clamp(*SIZES.start(), *SIZES.end());
        FontId::new(f32::from(size), family)
    }
}

/// Gives egui the chosen font, behind egui's own monospaced fonts for characters it lacks,
/// and notes where it was found in `font.file`. A font that cannot be found or read leaves
/// egui's fonts as they are and says why.
pub fn apply(ctx: &egui::Context, font: &mut TextFont) -> Result<(), FontError> {
    let Some(name) = font.family.clone() else {
        ctx.set_fonts(FontDefinitions::default());
        font.file = None;
        return Ok(());
    };
    let started = std::time::Instant::now();
    let cached = font.file.as_ref().and_then(|(path, index)| {
        let data = std::fs::read(path).ok()?;
        has_family(&data, *index, &name).then_some((data, *index))
    });
    let (data, index) = match cached {
        Some(found) => found,
        None => {
            let (data, index, path) =
                load(&name, font.face).ok_or_else(|| FontError::NotFound(name.clone()))?;
            font.file = path.map(|p| (p, index));
            (data, index)
        }
    };
    if !is_monospace(&data, index) {
        return Err(FontError::NotMonospaced(name));
    }
    let mut defs = FontDefinitions::default();
    let mut data = FontData::from_owned(data);
    data.index = index;
    defs.font_data.insert(FAMILY.into(), Arc::new(data));
    let mut chain = vec![FAMILY.to_string()];
    chain.extend(defs.families[&FontFamily::Monospace].iter().cloned());
    defs.families.insert(FontFamily::Name(FAMILY.into()), chain);
    ctx.set_fonts(defs);
    log::debug!("text font {name} loaded in {:?}", started.elapsed());
    Ok(())
}

/// The system's monospaced families and their faces, sorted by name. It reads every family's
/// regular font, so it runs on a thread of its own.
pub fn monospace_families() -> Vec<Family> {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: true,
    });
    let names: Vec<String> = collection.family_names().map(str::to_owned).collect();
    let mut out: Vec<Family> = names
        .into_iter()
        .filter_map(|name| {
            let family = collection.family_by_name(&name)?;
            let mut faces: Vec<Face> = family.fonts().iter().filter_map(Face::of).collect();
            faces.sort_by_key(|f| Face::ALL.iter().position(|a| a == f));
            faces.dedup();
            let regular = family
                .fonts()
                .iter()
                .find(|f| Face::of(f) == Some(Face::Regular))?;
            let data = regular.load(None)?;
            is_monospace(data.as_ref(), regular.index()).then_some(Family { name, faces })
        })
        .collect();
    out.sort_by_key(|f| f.name.to_lowercase());
    out.dedup_by(|a, b| a.name == b.name);
    out
}

impl FontDialog {
    pub fn new(current: &TextFont) -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(monospace_families());
        });
        Self {
            draft: current.clone(),
            families: None,
            rx: Some(rx),
        }
    }

    /// Draws the dialog; returns what its buttons asked for.
    pub fn show(&mut self, ctx: &egui::Context, lang: Lang) -> Option<Answer> {
        if let Some(found) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.families = Some(found);
            self.rx = None;
        }
        if self.rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut answer = None;
        egui::Window::new(lang.font_title())
            // Its own id: the title changes with the language.
            .id(egui::Id::unique("font-dialog"))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(lang.font_intro());
                ui.add_space(6.0);
                self.family_choice(ui, lang);
                self.face_choice(ui, lang);
                ui.horizontal(|ui| {
                    ui.label(lang.font_size());
                    ui.add(
                        egui::DragValue::new(&mut self.draft.size)
                            .range(SIZES)
                            .speed(0.2)
                            .suffix(" pt"),
                    );
                });
                ui.add_space(6.0);
                ui.label(lang.font_preview());
                ui.label(
                    RichText::new("<order id=\"1\">Grüße &amp; 0O 1lI</order>")
                        .font(self.preview_font()),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(lang.dialog_ok()).clicked() {
                        answer = Some(Answer::Use(self.draft.clone()));
                    }
                    if ui.button(lang.dialog_cancel()).clicked() {
                        answer = Some(Answer::Cancel);
                    }
                    if ui.button(lang.font_default()).clicked() {
                        self.draft = TextFont::default();
                    }
                });
            });
        answer
    }

    fn family_choice(&mut self, ui: &mut Ui, lang: Lang) {
        let shown = self
            .draft
            .family
            .clone()
            .unwrap_or_else(|| lang.font_built_in().into());
        ui.horizontal(|ui| {
            ui.label(lang.font_family());
            egui::ComboBox::from_id_salt("text-font-family")
                .selected_text(shown)
                .width(240.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.draft.family.is_none(), lang.font_built_in())
                        .clicked()
                    {
                        self.draft.family = None;
                        self.draft.face = Face::Regular;
                        self.draft.file = None;
                    }
                    match &self.families {
                        None => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(lang.font_looking());
                            });
                        }
                        Some(families) => {
                            for f in families {
                                let chosen = self.draft.family.as_deref() == Some(&f.name);
                                if ui.selectable_label(chosen, &f.name).clicked() {
                                    self.draft.family = Some(f.name.clone());
                                    self.draft.file = None;
                                    if !f.faces.contains(&self.draft.face) {
                                        self.draft.face = Face::Regular;
                                    }
                                }
                            }
                        }
                    }
                });
        });
    }

    fn face_choice(&mut self, ui: &mut Ui, lang: Lang) {
        let faces: Vec<Face> = match &self.draft.family {
            None => vec![Face::Regular],
            Some(name) => self
                .families
                .as_ref()
                .and_then(|fs| fs.iter().find(|f| &f.name == name))
                .map_or_else(|| vec![self.draft.face], |f| f.faces.clone()),
        };
        ui.horizontal(|ui| {
            ui.label(lang.font_style());
            for face in Face::ALL {
                ui.add_enabled_ui(faces.contains(&face), |ui| {
                    if ui
                        .radio_value(&mut self.draft.face, face, face.label(lang))
                        .changed()
                    {
                        self.draft.file = None;
                    }
                });
            }
        });
    }

    /// The preview uses the chosen size; the family shows once the font is applied.
    fn preview_font(&self) -> FontId {
        FontId::monospace(f32::from(self.draft.size))
    }
}

/// Lays out the tree, the grid or the cell pane drawn in `ui` with `font`, when one is chosen.
pub fn use_in_views(ui: &mut Ui, font: Option<&FontId>) {
    if let Some(font) = font {
        let styles = &mut ui.style_mut().text_styles;
        styles.insert(egui::TextStyle::Body, font.clone());
        styles.insert(egui::TextStyle::Button, font.clone());
    }
}

/// The font of `family` closest to `face`: its data, its index in the file, and the file.
fn load(family: &str, face: Face) -> Option<(Vec<u8>, u32, Option<PathBuf>)> {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: true,
    });
    let info = collection.family_by_name(family)?;
    let font = info.fonts().iter().min_by_key(|f| {
        let weight = f.weight().value();
        let want = if face.bold() { 700.0 } else { 400.0 };
        let slant = (!matches!(f.style(), FontStyle::Normal)) != face.italic();
        (slant as u32) * 10_000 + (weight - want).abs() as u32
    })?;
    let data = font.load(None)?;
    let path = match font.source().kind() {
        SourceKind::Path(path) => Some(path.to_path_buf()),
        SourceKind::Memory(_) => None,
    };
    Some((data.as_ref().to_vec(), font.index(), path))
}

/// Whether the font at `index` in `data` belongs to `family`, by any of its names.
fn has_family(data: &[u8], index: u32, family: &str) -> bool {
    let Ok(font) = skrifa::FontRef::from_index(data, index) else {
        return false;
    };
    [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
        .into_iter()
        .flat_map(|id| font.localized_strings(id))
        .any(|s| s.chars().eq(family.chars()))
}

/// Whether every character of [`PROBE`] is there and as wide as the others.
fn is_monospace(data: &[u8], index: u32) -> bool {
    let Ok(font) = skrifa::FontRef::from_index(data, index) else {
        return false;
    };
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let widths: Option<Vec<f32>> = PROBE
        .iter()
        .map(|&c| charmap.map(c).and_then(|g| metrics.advance_width(g)))
        .collect();
    match widths.as_deref() {
        Some([first, rest @ ..]) if *first > 0.0 => {
            rest.iter().all(|w| (w - first).abs() <= first * 0.01)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monospace_fonts_are_told_apart() {
        assert!(is_monospace(epaint_default_fonts::HACK_REGULAR, 0));
        assert!(!is_monospace(epaint_default_fonts::UBUNTU_LIGHT, 0));
        assert!(!is_monospace(b"not a font", 0));
    }

    #[test]
    fn faces_by_name() {
        for face in Face::ALL {
            assert_eq!(Face::from_name(face.name()), Some(face));
        }
        assert_eq!(Face::from_name("heavy"), None);
    }

    #[test]
    fn size_stays_in_range() {
        let font = TextFont {
            size: 200,
            ..TextFont::default()
        };
        assert_eq!(font.font_id().family, FontFamily::Monospace);
        // Tree and grid: unchanged by default, egui's proportional font at the size with the
        // built-in font, and the chosen font otherwise.
        assert_eq!(TextFont::default().view_font_id(), None);
        assert_eq!(
            font.view_font_id(),
            Some(FontId::proportional(f32::from(*SIZES.end())))
        );
        let chosen = TextFont {
            family: Some("DejaVu Sans Mono".into()),
            ..TextFont::default()
        };
        assert_eq!(
            chosen.view_font_id(),
            Some(FontId::new(13.0, FontFamily::Name(FAMILY.into())))
        );
        assert_eq!(font.font_id().size, f32::from(*SIZES.end()));
    }

    #[test]
    fn family_names_are_read_from_the_font() {
        assert!(has_family(epaint_default_fonts::HACK_REGULAR, 0, "Hack"));
        assert!(!has_family(epaint_default_fonts::HACK_REGULAR, 0, "Ubuntu"));
        assert!(!has_family(b"not a font", 0, "Hack"));
    }
}
