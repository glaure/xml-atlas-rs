//! The rules check's result in the preview's header (`docs/spec/references.md`, REF-16; D39,
//! step 2). The check is the engine's (REF-14); this says when to run it and shows its sum.
//!
//! It runs only while the preview shows: once for the rules' text, then after a pause in
//! editing the document. Matches are not counted in a large document, as in the editor.

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{RichText, Ui};
use xml_atlas_engine::Document;
use xml_atlas_engine::problems::Severity;
use xml_atlas_engine::references::check::{Report, check};

use crate::rules_editor::{COUNT_AT_MOST, PAUSE};

/// Findings named when the pointer rests on the result.
const NAMED: usize = 8;

/// The result, and what it is for.
#[derive(Default)]
pub struct RulesStatus {
    checked: Option<Checked>,
    /// The document's revision seen last, and when to check it.
    waiting: Option<(u64, Instant)>,
}

struct Checked {
    text: Arc<str>,
    revision: u64,
    report: Report,
    /// Matches were counted in the document.
    counted: bool,
}

impl RulesStatus {
    /// Checks when that is due. Returns when to look again while it waits.
    pub fn update(&mut self, text: &Arc<str>, doc: &Document, now: Instant) -> Option<Duration> {
        let revision = doc.revision();
        // The pointer, not the text, as this runs each frame: a tab keeps it while it is the
        // same text.
        let same_text = self
            .checked
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(&c.text, text));
        if same_text {
            if self
                .checked
                .as_ref()
                .is_some_and(|c| c.revision == revision)
            {
                self.waiting = None;
                return None;
            }
            // An edit of the document: wait for a pause, and for the text to be read again.
            let due = match self.waiting {
                Some((seen, due)) if seen == revision => due,
                _ => now + PAUSE,
            };
            self.waiting = Some((revision, due));
            if now < due {
                return Some(due - now);
            }
            if doc.stale().is_some() {
                // Reading it again wakes the window.
                return None;
            }
        }
        let counted = doc.src.len() <= COUNT_AT_MOST && doc.stale().is_none();
        self.checked = Some(Checked {
            report: check(text, counted.then_some(doc)),
            text: Arc::clone(text),
            revision,
            counted,
        });
        self.waiting = None;
        None
    }

    /// Draws the result. True when it was clicked, to edit the rules.
    pub fn show(&self, ui: &mut Ui) -> bool {
        let Some(checked) = &self.checked else {
            return false;
        };
        let (errors, warnings) = (checked.report.errors(), checked.warnings());
        let visuals = ui.visuals();
        let colour = if errors > 0 {
            visuals.error_fg_color
        } else if warnings > 0 {
            visuals.warn_fg_color
        } else {
            visuals.weak_text_color()
        };
        ui.link(RichText::new(checked.summary()).color(colour))
            .on_hover_text(checked.details())
            .clicked()
    }
}

impl Checked {
    fn warnings(&self) -> usize {
        self.report
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count()
    }

    fn summary(&self) -> String {
        let count = |n: usize, what: &str| match n {
            1 => format!("1 {what}"),
            n => format!("{n} {what}s"),
        };
        match (self.report.errors(), self.warnings()) {
            (0, 0) if self.counted => "check OK".to_owned(),
            // No more than that is known without the count.
            (0, 0) => "OK, not counted".to_owned(),
            (0, warnings) => count(warnings, "warning"),
            (errors, 0) => count(errors, "error"),
            (errors, warnings) => {
                format!("{}, {}", count(errors, "error"), count(warnings, "warning"))
            }
        }
    }

    fn details(&self) -> String {
        let findings = &self.report.findings;
        let mut lines: Vec<String> = findings
            .iter()
            .take(NAMED)
            .map(|f| format!("Line {}, column {}: {}", f.line, f.column, f.message))
            .collect();
        if findings.len() > NAMED {
            lines.push(format!("and {} more", findings.len() - NAMED));
        }
        if !self.counted {
            lines.push("Matches in the document were not counted: the editor can.".to_owned());
        }
        lines.push("The check of the rules file. Click to edit it.".to_owned());
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use xml_atlas_engine::Edit;

    use super::*;

    const RULES: &str = r#"{"includes": [{"element": "Größe", "attribute": "src"}]}"#;

    fn summary(status: &RulesStatus) -> String {
        status.checked.as_ref().unwrap().summary()
    }

    /// REF-16: at once for the rules; after an edit of the document, at the pause.
    #[test]
    fn an_edit_of_the_document_waits_for_a_pause() {
        let src = "\u{feff}<R>\r\n<Größe src=\"a\"/>\r\n</R>";
        let mut doc = Document::from_bytes(src.as_bytes().to_vec());
        let text: Arc<str> = RULES.into();
        let mut status = RulesStatus::default();
        let start = Instant::now();
        assert_eq!(status.update(&text, &doc, start), None);
        assert_eq!(summary(&status), "check OK");

        let here = doc.src.windows(4).position(|w| w == b"src=").unwrap();
        doc.apply(Edit::new("Type").replace(here..here + 3, "for"))
            .unwrap();
        assert!(status.update(&text, &doc, start).is_some());
        assert_eq!(summary(&status), "check OK", "not while typing");
        doc.reparse();
        status.update(&text, &doc, start);
        assert_eq!(status.update(&text, &doc, start + PAUSE * 2), None);
        assert_eq!(summary(&status), "1 warning");
    }

    /// Changed rules are checked at once, also when they mean the same as before.
    #[test]
    fn changed_rules_are_checked_at_once() {
        let doc = Document::from_bytes("<R><Größe src=\"a\"/></R>".as_bytes().to_vec());
        let mut status = RulesStatus::default();
        let now = Instant::now();
        status.update(&RULES.into(), &doc, now);
        let typo = RULES.replace("\"src\"", "\"src\", \"enabeld\": false");
        status.update(&typo.into(), &doc, now);
        assert_eq!(summary(&status), "1 warning");
        status.update(&"{\"includes\": [],}".into(), &doc, now);
        assert_eq!(summary(&status), "1 error");
    }

    /// A large document is not counted in, so a rule without a match is not reported there.
    #[test]
    fn a_large_document_is_not_counted_in() {
        let src = format!("<R>{}</R>", " ".repeat(COUNT_AT_MOST));
        let doc = Document::from_bytes(src.into_bytes());
        let mut status = RulesStatus::default();
        status.update(&RULES.into(), &doc, Instant::now());
        let checked = status.checked.as_ref().unwrap();
        assert!(!checked.counted && checked.details().contains("not counted"));
        assert_eq!(checked.summary(), "OK, not counted");
    }
}
