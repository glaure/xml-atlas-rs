//! Settings remembered between sessions (`docs/spec/window.md`, WIN-3).
//!
//! A small TOML file in the platform's configuration folder. It is written whenever a setting
//! changes, atomically, so a crash cannot leave half a file. Reading never fails: a missing
//! file gives the defaults, and a line that cannot be read is skipped.

use std::path::{Path, PathBuf};

use xml_atlas_engine::FormatOptions;
use xml_atlas_engine::find::FindOptions;

/// Overrides where the settings file is, e.g. for tests.
pub const PATH_VARIABLE: &str = "XML_ATLAS_SETTINGS";

const FILE_NAME: &str = "settings.toml";
const APP_DIR: &str = "xml-atlas";

/// Where the grid and the text go on the right (WIN-1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// The grid above the text.
    #[default]
    GridAbove,
    /// The text above the grid, as in XML Marker.
    TextAbove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub format: FormatOptions,
    pub layout: Layout,
    /// Attribute rows in the tree (TREE-1).
    pub tree_attributes: bool,
    pub find: FindOptions,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format: FormatOptions::default(),
            layout: Layout::default(),
            tree_attributes: true,
            find: FindOptions::default(),
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

    /// The settings as TOML.
    pub fn to_text(&self) -> String {
        let f = &self.format;
        let layout = match self.layout {
            Layout::GridAbove => "grid-above",
            Layout::TextAbove => "text-above",
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
             tree_attributes = {}\n\
             \n\
             [find]\n\
             match_case = {}\n\
             whole_word = {}\n",
            f.indent_size,
            f.tabs,
            f.max_attributes,
            f.remove_comments,
            f.trim_text,
            self.tree_attributes,
            self.find.match_case,
            self.find.whole_word,
        )
    }

    /// Reads what [`Settings::to_text`] writes. Unknown or unreadable lines are skipped.
    pub fn from_text(text: &str) -> Self {
        let mut s = Self::default();
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
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
                ("view", "tree_attributes") => set(&mut s.tree_attributes, boolean(value)),
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

/// The settings file: [`PATH_VARIABLE`] if set, else `xml-atlas/settings.toml` in the
/// platform's configuration folder. `None` when there is no home folder to put it in.
pub fn path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(PATH_VARIABLE).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    config_dir().map(|d| d.join(APP_DIR).join(FILE_NAME))
}

fn config_dir() -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    }
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
            layout: Layout::TextAbove,
            tree_attributes: false,
            find: FindOptions {
                match_case: true,
                whole_word: true,
            },
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
    }

    #[test]
    fn save_and_load_through_the_file() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-settings-{}", std::process::id()));
        let file = dir.join("nested").join(FILE_NAME);
        let s = Settings {
            layout: Layout::TextAbove,
            ..Settings::default()
        };
        s.save_to(&file).unwrap();
        assert_eq!(Settings::load_from(&file), s);
        // A damaged line does not cost the readable ones.
        std::fs::write(
            &file,
            b"\x00\xff [[[ = =\n[view]\nlayout = \"text-above\"\n",
        )
        .unwrap();
        assert_eq!(Settings::load_from(&file).layout, Layout::TextAbove);
        std::fs::remove_dir_all(&dir).unwrap();
        // A missing file gives the defaults.
        assert_eq!(Settings::load_from(&file), Settings::default());
    }
}
