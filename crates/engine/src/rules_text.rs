//! The text of a rules file being edited (`docs/spec/references.md`, REF-15): it keeps the
//! file's byte order mark and line endings, saves atomically, and does not save over what
//! someone else wrote meanwhile.

use std::path::{Path, PathBuf};

const BOM: &str = "\u{feff}";

#[derive(Debug, thiserror::Error)]
pub enum TextError {
    #[error("{path}: {error}")]
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
    #[error("{0} is not UTF-8 text")]
    NotText(PathBuf),
    #[error("{0} is too large to be a rules file")]
    TooLarge(PathBuf),
    #[error("{0} changed on disk since it was read")]
    Changed(PathBuf),
}

/// A rules file as it was read or last saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesText {
    path: PathBuf,
    bytes: Vec<u8>,
    /// The bytes as text to edit: without the mark, and with `\n` where every ending is CRLF.
    text: String,
    bom: bool,
    crlf: bool,
}

impl RulesText {
    pub fn read(path: &Path) -> Result<Self, TextError> {
        let error = |error| TextError::Io {
            path: path.to_owned(),
            error,
        };
        if std::fs::metadata(path).map_err(error)?.len() > crate::rules_folder::MAX_RULES_BYTES {
            return Err(TextError::TooLarge(path.to_owned()));
        }
        let bytes = std::fs::read(path).map_err(error)?;
        let all = std::str::from_utf8(&bytes).map_err(|_| TextError::NotText(path.to_owned()))?;
        let (bom, body) = match all.strip_prefix(BOM) {
            Some(body) => (true, body),
            None => (false, all),
        };
        // Mixed endings are edited as they are, so that none of them changes.
        let breaks = body.matches('\n').count();
        let crlf = breaks > 0 && body.matches("\r\n").count() == breaks;
        let text = if crlf {
            body.replace("\r\n", "\n")
        } else {
            body.to_owned()
        };
        Ok(Self {
            path: path.to_owned(),
            bytes,
            text,
            bom,
            crlf,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The text to edit, as read or last saved.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Follows the file to another name in the folder.
    pub fn moved(&mut self, to: PathBuf) {
        self.path = to;
    }

    /// Whether the file is no longer what was read or saved, or is gone.
    pub fn changed_on_disk(&self) -> bool {
        std::fs::read(&self.path).map_or(true, |now| now != self.bytes)
    }

    /// Writes `text` in the file's own form. Refuses when the file changed on disk, unless
    /// `overwrite`. Text that was not changed leaves every byte as it was.
    pub fn save(&mut self, text: &str, overwrite: bool) -> Result<(), TextError> {
        let changed = self.changed_on_disk();
        if changed && !overwrite {
            return Err(TextError::Changed(self.path.clone()));
        }
        if !changed && text == self.text {
            return Ok(());
        }
        let mut out = String::with_capacity(text.len() + BOM.len());
        if self.bom {
            out.push_str(BOM);
        }
        if self.crlf {
            // Pasted text may bring its own CRLF.
            out.push_str(&text.replace("\r\n", "\n").replace('\n', "\r\n"));
        } else {
            out.push_str(text);
        }
        crate::save_atomic(&self.path, out.as_bytes()).map_err(|error| TextError::Io {
            path: self.path.clone(),
            error,
        })?;
        self.bytes = out.into_bytes();
        self.text = text.to_owned();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, bytes: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "xml-atlas-rules-text-{name}-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Rule 7: an edit changes only what was edited.
    #[test]
    fn saving_keeps_the_mark_the_endings_and_unchanged_bytes() {
        for (name, before, after) in [
            (
                "bom-crlf",
                "\u{feff}{\r\n  \"a\": \"Größe\"\r\n}\r\n",
                "\u{feff}{\r\n  \"b\": \"Größe\"\r\n}\r\n",
            ),
            ("lf", "{\n\t\"a\": 1\n}", "{\n\t\"b\": 1\n}"),
            (
                "mixed",
                "{\r\n\"a\": 1,\n\"c\": 2}\n",
                "{\r\n\"b\": 1,\n\"c\": 2}\n",
            ),
        ] {
            let path = file(name, before);
            let mut rules = RulesText::read(&path).unwrap();
            let text = rules.text().to_owned();
            rules.save(&text, false).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "{name}");
            rules.save(&text.replace("\"a\"", "\"b\""), false).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), after, "{name}");
            assert!(!rules.changed_on_disk());
            std::fs::remove_file(&path).unwrap();
        }
    }

    #[test]
    fn a_change_on_disk_is_not_saved_over_unasked() {
        let path = file("changed", "{}");
        let mut rules = RulesText::read(&path).unwrap();
        std::fs::write(&path, "{ }").unwrap();
        assert!(matches!(
            rules.save("{\"a\":1}", false),
            Err(TextError::Changed(_))
        ));
        assert!(
            matches!(rules.save("{}", false), Err(TextError::Changed(_))),
            "unchanged, too"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ }");
        rules.save("{}", true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        std::fs::remove_file(&path).unwrap();
        assert!(rules.changed_on_disk(), "gone");
    }
}
