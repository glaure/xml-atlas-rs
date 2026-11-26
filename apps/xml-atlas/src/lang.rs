//! The language of the window's texts (`docs/spec/language.md`; `docs/decisions.md`, D45).
//! Each text is a method of [`Lang`], made by the build script from `lang/*.toml`; a text a
//! language lacks gives the English one. The language is passed along from the settings, so
//! that a test can draw in either.

/// A language the window's texts are in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    En,
    De,
}

/// What Settings offers (LANG-6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LanguageChoice {
    /// The system's language, where we have it (LANG-7).
    System,
    #[default]
    English,
    German,
}

impl Lang {
    /// A number the window writes in its own texts, with `places` after the decimal mark:
    /// a comma in German (LANG-17). Line numbers and a document's values never come here.
    pub fn decimal(self, number: f64, places: usize) -> String {
        let text = format!("{number:.places$}");
        match self {
            Lang::En => text,
            Lang::De => text.replace('.', ","),
        }
    }

    /// Whether `count` takes the form `one` (LANG-11).
    fn is_one(self, count: u64) -> bool {
        match self {
            Lang::En | Lang::De => count == 1,
        }
    }
}

impl LanguageChoice {
    /// The language this choice gives on a system whose language is `system`, such as
    /// `de-AT`; `None` when it cannot be read.
    pub fn language(self, system: Option<&str>) -> Lang {
        match self {
            LanguageChoice::English => Lang::En,
            LanguageChoice::German => Lang::De,
            LanguageChoice::System => {
                let first = system
                    .and_then(|s| s.split(['-', '_', '.']).next())
                    .unwrap_or_default();
                if first.eq_ignore_ascii_case("de") {
                    Lang::De
                } else {
                    Lang::En
                }
            }
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/texts.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang_generate::{File, Generated, generate};

    const EN: &str = "save = \"Save\"\n\
                      go = \"Go to {line} of {name}\"\n\
                      braces = \"{{x}}\"\n\
                      rows = { one = \"one row of {name}\", other = \"{count} rows of {name}\" }\n";

    fn build(de: &str) -> Result<Generated, String> {
        let en = File {
            code: "en",
            source: EN,
        };
        generate(
            &en,
            &[File {
                code: "de",
                source: de,
            }],
        )
    }

    /// LANG-12: a text a language lacks, an empty one and a plural without a form it needs
    /// are shown in English, whole, and the build names them.
    #[test]
    fn what_a_language_lacks_is_english_and_named() {
        let made = build(
            "save = \"\"\nrows = { one = \"eine Zeile von {name}\" }\ngo = \"{name}: zu {line}\"\n",
        )
        .unwrap();
        assert_eq!(made.warnings.len(), 1);
        let warning = &made.warnings[0];
        assert!(
            warning.starts_with("de.toml lacks 3 text(s), shown in English: braces, rows, save"),
            "{warning}"
        );
        let code = made.code;
        assert!(code.contains("Lang::En | Lang::De => \"Save\""), "{code}");
        assert!(!code.contains("eine Zeile"), "half a plural: {code}");
        // A placeholder may stand elsewhere, and a brace meant as such is one.
        assert!(
            code.contains("Lang::De => format!(\"{name}: zu {line}\")"),
            "{code}"
        );
        assert!(code.contains("Lang::En | Lang::De => \"{x}\""), "{code}");
        // The parameters come in English's order, whatever German's is.
        assert!(
            code.contains("pub fn go(self, line: &str, name: &str)"),
            "{code}"
        );
    }

    /// LANG-13: the build fails, and says which file, which text and why.
    #[test]
    fn a_wrong_text_fails_the_build() {
        for (de, why) in [
            ("save = \"x", "de.toml cannot be read"),
            ("save = \"a\"\nsave = \"b\"", "de.toml cannot be read"),
            ("svae = \"Speichern\"", "`svae`: en.toml has no text"),
            ("go = \"Zu {zeile} von {name}\"", "`go`: its placeholders"),
            ("go = \"Zu {line}\"", "`go`: its placeholders"),
            ("save = \"{x}\"", "`save`: its placeholders"),
            ("save = \"{ x\"", "`save`: a `{` is not closed"),
            ("save = \"x }\"", "`save`: a `}` stands alone"),
            ("rows = \"Zeilen\"", "`rows`: it is a plural in only one"),
            (
                "save = { one = \"a\", other = \"b\" }",
                "`save`: it is a plural in only one",
            ),
            (
                "rows = { one = \"{name}\", other = \"{name}\", few = \"{name}\" }",
                "`rows`: en.toml has no form `few`",
            ),
            (
                "rows = { one = \"eine\", other = \"{count} von {name}\" }",
                "`rows`: its placeholders",
            ),
            ("Save = \"x\"", "`Save`: a name is lower case"),
            ("save = 3", "`save`: neither a text"),
        ] {
            let error = build(de).err().unwrap_or_else(|| panic!("{de:?} built"));
            assert!(error.starts_with("de.toml"), "{error}");
            assert!(error.contains(why), "{de:?}: {error}");
        }
        // Every form may leave out the number.
        build("rows = { one = \"eine von {name}\", other = \"viele von {name}\" }").unwrap();
    }

    #[test]
    fn plurals_take_one_for_exactly_one() {
        for (count, english, german) in [
            (0, "0 days ago", "vor 0 Tagen"),
            (1, "a day ago", "vor einem Tag"),
            (2, "2 days ago", "vor 2 Tagen"),
        ] {
            assert_eq!(Lang::En.settings_updates_days(count), english);
            assert_eq!(Lang::De.settings_updates_days(count), german);
        }
    }

    /// LANG-6, LANG-7: System is German for German of any country, else English.
    #[test]
    fn system_follows_a_german_system_only() {
        let system = |s| LanguageChoice::System.language(s);
        for german in ["de", "de-DE", "de-AT", "de_CH.UTF-8", "DE-de"] {
            assert_eq!(system(Some(german)), Lang::De, "{german}");
        }
        for other in ["en-US", "fr-FR", "den", "nds-DE", "C", ""] {
            assert_eq!(system(Some(other)), Lang::En, "{other}");
        }
        assert_eq!(system(None), Lang::En);
        assert_eq!(LanguageChoice::default(), LanguageChoice::English);
        assert_eq!(LanguageChoice::English.language(Some("de-DE")), Lang::En);
        assert_eq!(LanguageChoice::German.language(Some("en-US")), Lang::De);
    }

    /// LANG-15: the program's fonts have every character of the German texts and guide.
    #[test]
    fn the_fonts_have_every_german_character() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let source = include_str!("../lang/de.toml");
            let texts: toml::Table = source.parse().unwrap();
            let mut characters = std::collections::BTreeSet::new();
            let guide = include_str!("../assets/help.de.md");
            characters.extend(guide.chars().filter(|c| !c.is_control()));
            for value in texts.values() {
                characters.extend(value.to_string().chars().filter(|c| !c.is_control()));
            }
            for font in [
                egui::FontId::proportional(14.0),
                egui::FontId::monospace(14.0),
            ] {
                let missing: String = ui.fonts_mut(|f| {
                    characters
                        .iter()
                        .copied()
                        .filter(|&c| !f.has_glyph(&font, c))
                        .collect()
                });
                assert!(missing.is_empty(), "{font:?} lacks {missing:?}");
            }
        });
        out.textures_delta.clear();
    }
}
