//! Settings remembered between sessions (`docs/spec/window.md`, WIN-3).
//!
//! A small TOML file in the platform's configuration folder. It is written whenever a setting
//! changes, atomically, so a crash cannot leave half a file. Reading never fails: a missing
//! file gives the defaults, and a line that cannot be read is skipped.

use std::path::{Path, PathBuf};

use xml_atlas_engine::FormatOptions;
use xml_atlas_engine::find::FindOptions;

/// Files kept in File > Open Recent (OPEN-12).
pub const RECENT_FILES: usize = 10;

/// Where the grid and the text go on the right (WIN-1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// The grid above the text.
    GridAbove,
    /// The text above the grid, as in XML Marker; the default.
    #[default]
    TextAbove,
}

/// Light or dark (WIN-2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeChoice {
    /// The desktop's preference.
    #[default]
    System,
    Light,
    Dark,
}

/// How Light looks (WIN-8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LightStyle {
    /// Greys that set the views apart from the menus; the default.
    #[default]
    Soft,
    /// Light as in 1.0: egui's own, nearly white throughout.
    Classic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub format: FormatOptions,
    pub layout: Layout,
    pub theme: ThemeChoice,
    pub light_style: LightStyle,
    /// Attribute rows in the tree (TREE-1).
    pub tree_attributes: bool,
    /// The grid is shown (WIN-1); hidden, the text takes its place.
    pub grid: bool,
    /// The pane beside the grid for the clicked cell (GRID-16).
    pub cell_pane: bool,
    /// Long lines wrap in the text view (TEXT-13).
    pub word_wrap: bool,
    /// The toolbar under the menu (WIN-5).
    pub toolbar: bool,
    /// The pane with the selected element's references resolved (REF-5).
    pub reference_preview: bool,
    /// The preview as a tree, right of it (REF-10).
    pub preview_tree: bool,
    /// The text view's font (TEXT-25).
    pub text_font: crate::fonts::TextFont,
    /// Allow AI agents (MCP) in Settings (D28, WIN-11).
    pub agents: bool,
    /// Ask GitHub once a day for a newer version (PKG-7, D30).
    pub update_check: bool,
    /// When the update check last asked, in seconds since 1970; `None` before the first.
    pub update_last: Option<u64>,
    /// The newer version the status bar last told about, so it tells once per version.
    pub update_shown: Option<crate::update::Version>,
    pub find: FindOptions,
    /// Recently opened files, newest first, at most [`RECENT_FILES`] (OPEN-12).
    pub recent: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format: FormatOptions::default(),
            layout: Layout::default(),
            theme: ThemeChoice::default(),
            light_style: LightStyle::default(),
            tree_attributes: true,
            grid: true,
            cell_pane: true,
            word_wrap: false,
            toolbar: true,
            reference_preview: false,
            preview_tree: false,
            text_font: Default::default(),
            agents: false,
            update_check: true,
            update_last: None,
            update_shown: None,
            find: FindOptions::default(),
            recent: Vec::new(),
        }
    }
}

impl Settings {
    /// The saved settings, or the defaults.
    pub fn load() -> Self {
        path().map_or_else(Self::default, |p| Self::load_from(&p))
    }

    /// Writes the settings file, creating its folder.
    pub fn save(&self) -> std::io::Result<()> {
        match path() {
            Some(p) => self.save_to(&p),
            None => Ok(()),
        }
    }

    fn load_from(path: &Path) -> Self {
        // Bytes that are not UTF-8 spoil only their own line, not the whole file.
        match std::fs::read(path) {
            Ok(bytes) => Self::from_text(&String::from_utf8_lossy(&bytes)),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("cannot read settings {}: {e}", path.display());
                }
                Self::default()
            }
        }
    }

    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        xml_atlas_engine::save_atomic(path, self.to_text().as_bytes())
    }

    /// Puts `path` first in the recent files, removing it further down and dropping the oldest
    /// beyond [`RECENT_FILES`]. Paths are made absolute, so the same file is listed once.
    pub fn add_recent(&mut self, path: &Path) {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(RECENT_FILES);
    }

    /// The settings as TOML.
    pub fn to_text(&self) -> String {
        let f = &self.format;
        let layout = match self.layout {
            Layout::GridAbove => "grid-above",
            Layout::TextAbove => "text-above",
        };
        let theme = match self.theme {
            ThemeChoice::System => "system",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
        };
        let light_style = match self.light_style {
            LightStyle::Soft => "soft",
            LightStyle::Classic => "classic",
        };
        format!(
            "# XML Atlas settings. The program rewrites this file when a setting changes.\n\
             \n\
             [format]\n\
             indent_size = {}\n\
             tabs = {}\n\
             max_attributes = {}\n\
             remove_comments = {}\n\
             trim_text = {}\n\
             \n\
             [view]\n\
             layout = \"{layout}\"\n\
             theme = \"{theme}\"\n\
             light_style = \"{light_style}\"\n\
             tree_attributes = {}\n\
             grid = {}\n\
             cell_pane = {}\n\
             word_wrap = {}\n\
             toolbar = {}\n\
             reference_preview = {}\n\
             preview_tree = {}\n\
             agents = {}\n\
             \n\
             [updates]\n\
             check = {}\n\
             last_check = {}\n\
             shown = \"{}\"\n\
             \n\
             [text]\n\
             font = {}\n\
             face = \"{}\"\n\
             size = {}\n\
             file = {}\n\
             index = {}\n\
             \n\
             [find]\n\
             match_case = {}\n\
             whole_word = {}\n\
             \n\
             [recent]\n\
             files = [\n{}]\n",
            f.indent_size,
            f.tabs,
            f.max_attributes,
            f.remove_comments,
            f.trim_text,
            self.tree_attributes,
            self.grid,
            self.cell_pane,
            self.word_wrap,
            self.toolbar,
            self.reference_preview,
            self.preview_tree,
            self.agents,
            self.update_check,
            // 0 for never: no check was made in 1970.
            self.update_last.unwrap_or(0),
            self.update_shown.map(|v| v.to_string()).unwrap_or_default(),
            quote(self.text_font.family.as_deref().unwrap_or("")),
            self.text_font.face.name(),
            self.text_font.size,
            quote(
                self.text_font
                    .file
                    .as_ref()
                    .and_then(|(p, _)| p.to_str())
                    .unwrap_or("")
            ),
            self.text_font.file.as_ref().map_or(0, |(_, i)| *i),
            self.find.match_case,
            self.find.whole_word,
            self.recent
                .iter()
                // A path that is not UTF-8 cannot be written into TOML; it is not remembered.
                .filter_map(|p| p.to_str())
                .map(|p| format!("    {},\n", quote(p)))
                .collect::<String>(),
        )
    }

    /// Reads what [`Settings::to_text`] writes. Unknown or unreadable lines are skipped.
    pub fn from_text(text: &str) -> Self {
        let mut s = Self::default();
        let mut section = String::new();
        // Inside the multi-line `files = [ ... ]` of `[recent]`.
        let mut in_files = false;
        // `[text] index`, the font file's face; it may come before or after `file`.
        let mut font_index = 0;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if in_files {
                if line == "]" {
                    in_files = false;
                } else if let Some(path) = unquote(line.trim_end_matches(',')) {
                    if s.recent.len() < RECENT_FILES {
                        s.recent.push(PathBuf::from(path));
                    }
                } else {
                    log::warn!("settings: skipped recent file {line:?}");
                }
                continue;
            }
            if section == "recent" && line.replace(' ', "") == "files=[" {
                in_files = true;
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.trim().to_string();
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                log::warn!("settings: skipped {line:?}");
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let ok = match (section.as_str(), key) {
                ("format", "indent_size") => set(&mut s.format.indent_size, number(value)),
                ("format", "tabs") => set(&mut s.format.tabs, boolean(value)),
                ("format", "max_attributes") => set(&mut s.format.max_attributes, number(value)),
                ("format", "remove_comments") => set(&mut s.format.remove_comments, boolean(value)),
                ("format", "trim_text") => set(&mut s.format.trim_text, boolean(value)),
                ("view", "layout") => set(
                    &mut s.layout,
                    match string(value) {
                        Some("grid-above") => Some(Layout::GridAbove),
                        Some("text-above") => Some(Layout::TextAbove),
                        _ => None,
                    },
                ),
                ("view", "theme") => set(
                    &mut s.theme,
                    match string(value) {
                        Some("system") => Some(ThemeChoice::System),
                        Some("light") => Some(ThemeChoice::Light),
                        Some("dark") => Some(ThemeChoice::Dark),
                        _ => None,
                    },
                ),
                ("view", "light_style") => set(
                    &mut s.light_style,
                    match string(value) {
                        Some("soft") => Some(LightStyle::Soft),
                        Some("classic") => Some(LightStyle::Classic),
                        _ => None,
                    },
                ),
                ("view", "tree_attributes") => set(&mut s.tree_attributes, boolean(value)),
                ("view", "grid") => set(&mut s.grid, boolean(value)),
                ("view", "cell_pane") => set(&mut s.cell_pane, boolean(value)),
                ("view", "word_wrap") => set(&mut s.word_wrap, boolean(value)),
                ("view", "toolbar") => set(&mut s.toolbar, boolean(value)),
                ("view", "reference_preview") => set(&mut s.reference_preview, boolean(value)),
                ("view", "preview_tree") => set(&mut s.preview_tree, boolean(value)),
                ("view", "agents") => set(&mut s.agents, boolean(value)),
                ("updates", "check") => set(&mut s.update_check, boolean(value)),
                ("updates", "last_check") => set(
                    &mut s.update_last,
                    value.parse::<u64>().ok().map(|t| (t > 0).then_some(t)),
                ),
                ("updates", "shown") => set(
                    &mut s.update_shown,
                    string(value).and_then(|v| {
                        if v.is_empty() {
                            Some(None)
                        } else {
                            crate::update::Version::parse(v).map(Some)
                        }
                    }),
                ),
                ("text", "font") => set(
                    &mut s.text_font.family,
                    unquote(value).map(|f| (!f.is_empty()).then_some(f)),
                ),
                ("text", "face") => set(
                    &mut s.text_font.face,
                    string(value).and_then(crate::fonts::Face::from_name),
                ),
                ("text", "file") => set(
                    &mut s.text_font.file,
                    unquote(value).map(|f| (!f.is_empty()).then(|| (PathBuf::from(f), font_index))),
                ),
                ("text", "index") => {
                    font_index = number(value)
                        .and_then(|n| u32::try_from(n).ok())
                        .unwrap_or(0);
                    if let Some((_, i)) = &mut s.text_font.file {
                        *i = font_index;
                    }
                    true
                }
                ("text", "size") => set(
                    &mut s.text_font.size,
                    value
                        .parse::<u8>()
                        .ok()
                        .filter(|v| crate::fonts::SIZES.contains(v)),
                ),
                ("find", "match_case") => set(&mut s.find.match_case, boolean(value)),
                ("find", "whole_word") => set(&mut s.find.whole_word, boolean(value)),
                _ => false,
            };
            if !ok {
                log::warn!("settings: skipped {line:?} in [{section}]");
            }
        }
        s
    }
}

/// The settings file: `XML_ATLAS_SETTINGS` if set, else `xml-atlas/settings.toml` in the
/// platform's configuration folder. `None` when there is no home folder to put it in. The
/// same rule finds the file agents connect with (D28), so it lives in `xml-atlas-mcp`.
pub fn path() -> Option<PathBuf> {
    xml_atlas_mcp::bridge::settings_file()
}

fn set<T>(slot: &mut T, value: Option<T>) -> bool {
    match value {
        Some(v) => {
            *slot = v;
            true
        }
        None => false,
    }
}

fn number(value: &str) -> Option<usize> {
    value.parse().ok()
}

fn boolean(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn string(value: &str) -> Option<&str> {
    value.strip_prefix('"')?.strip_suffix('"')
}

/// `s` as a TOML basic string: in quotes, with `\\` and `"` escaped.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The value of a TOML basic string written by [`quote`].
fn unquote(s: &str) -> Option<String> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next()? {
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                _ => return None,
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_settings_read_back_the_same() {
        let s = Settings {
            format: FormatOptions {
                indent_size: 2,
                tabs: true,
                max_attributes: 0,
                remove_comments: true,
                trim_text: true,
            },
            layout: Layout::GridAbove,
            theme: ThemeChoice::Light,
            light_style: LightStyle::Classic,
            tree_attributes: false,
            grid: false,
            cell_pane: false,
            word_wrap: true,
            toolbar: false,
            reference_preview: true,
            preview_tree: true,
            agents: true,
            update_check: false,
            update_last: Some(1_800_000_000),
            update_shown: Some(crate::update::Version(1, 10, 2)),
            text_font: crate::fonts::TextFont {
                family: Some("DejaVu Sans Mono \"Größe\"".into()),
                face: crate::fonts::Face::BoldItalic,
                size: 15,
                file: Some((
                    PathBuf::from("/usr/share/fonts/truetype/dejavu/Größe.ttc"),
                    2,
                )),
            },
            find: FindOptions {
                match_case: true,
                whole_word: true,
            },
            recent: vec![
                PathBuf::from("/home/me/Größe \"quoted\".xml"),
                PathBuf::from(r"C:\Users\me\orders [2].xml"),
            ],
        };
        assert_eq!(Settings::from_text(&s.to_text()), s);
        assert_eq!(
            Settings::from_text(&Settings::default().to_text()),
            Settings::default()
        );
    }

    #[test]
    fn missing_unknown_and_bad_lines_keep_the_defaults() {
        let text = "\
            [format]\n\
            indent_size = 8\n\
            tabs = maybe\n\
            colour = \"red\"\n\
            not a setting\n\
            [view]\n\
            layout = \"sideways\"\n\
            [other]\n\
            indent_size = 1\n";
        let s = Settings::from_text(text);
        let d = Settings::default();
        assert_eq!(s.format.indent_size, 8);
        assert_eq!(s.format.tabs, d.format.tabs);
        assert_eq!(s.layout, d.layout);
        assert_eq!(Settings::from_text(""), d);
        // As in XML Marker: the text above the grid (WIN-1).
        assert_eq!(d.layout, Layout::TextAbove);
    }

    #[test]
    fn recent_files_newest_first_without_doubles() {
        let mut s = Settings::default();
        for i in 0..12 {
            s.add_recent(Path::new(&format!("/nowhere/{i}.xml")));
        }
        assert_eq!(s.recent.len(), RECENT_FILES);
        assert_eq!(s.recent[0], Path::new("/nowhere/11.xml"));
        s.add_recent(Path::new("/nowhere/5.xml"));
        assert_eq!(s.recent[0], Path::new("/nowhere/5.xml"));
        assert_eq!(s.recent.iter().filter(|p| p.ends_with("5.xml")).count(), 1);
        assert_eq!(s.recent.len(), RECENT_FILES);
    }

    #[test]
    fn save_and_load_through_the_file() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-settings-{}", std::process::id()));
        let file = dir.join("nested").join("settings.toml");
        let s = Settings {
            layout: Layout::GridAbove,
            ..Settings::default()
        };
        s.save_to(&file).unwrap();
        assert_eq!(Settings::load_from(&file), s);
        // A damaged line does not cost the readable ones.
        std::fs::write(
            &file,
            b"\x00\xff [[[ = =\n[view]\nlayout = \"grid-above\"\n",
        )
        .unwrap();
        assert_eq!(Settings::load_from(&file).layout, Layout::GridAbove);
        std::fs::remove_dir_all(&dir).unwrap();
        // A missing file gives the defaults.
        assert_eq!(Settings::load_from(&file), Settings::default());
    }
}
