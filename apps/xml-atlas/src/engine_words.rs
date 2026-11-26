//! What the engine says, in the window's language (`docs/spec/language.md`, LANG-2). The
//! engine gives values, whose own words are the English the CLI and the MCP tools use; here
//! each is worded by the window. The matches name every case, so that a new one does not
//! build until it is given its words.

use xml_atlas_engine::command::CommandError;
use xml_atlas_engine::text::GoToError;
use xml_atlas_engine::xml::ParseError;
use xml_atlas_engine::{DeletedNode, EditError, FormatError, FormatOptions, StepName};

use crate::lang::Lang;

/// An undo step's name, as Edit > Undo and the status bar show it.
pub fn step_name(lang: Lang, step: &StepName) -> String {
    match step {
        StepName::Typing => lang.step_typing().into(),
        StepName::Paste => lang.step_paste().into(),
        StepName::Cut => lang.step_cut().into(),
        StepName::Delete => lang.step_delete().into(),
        StepName::Replace => lang.step_replace().into(),
        StepName::ReplaceAll(count) => lang.step_replace_all(*count as u64),
        StepName::Format => lang.step_format().into(),
        StepName::Compact => lang.step_compact().into(),
        StepName::ConvertToUtf8 => lang.step_convert_utf8().into(),
        StepName::EditText { element } => lang.step_edit_text(element),
        StepName::SetAttribute { attribute, element } => {
            lang.step_set_attribute(attribute, element)
        }
        StepName::RemoveAttribute { attribute, element } => {
            lang.step_remove_attribute(attribute, element)
        }
        StepName::Rename { from, to } => lang.step_rename(from, to),
        StepName::DeleteNode(node) => match node {
            DeletedNode::Element(element) => lang.step_delete_element(element),
            DeletedNode::Comment => lang.step_delete_comment().into(),
            DeletedNode::ProcessingInstruction => lang.step_delete_instruction().into(),
            DeletedNode::CData => lang.step_delete_cdata().into(),
            DeletedNode::Text => lang.step_delete_text().into(),
        },
        StepName::Duplicate { element } => lang.step_duplicate(element),
        StepName::MoveUp { element } => lang.step_move_up(element),
        StepName::MoveDown { element } => lang.step_move_down(element),
        StepName::Other(name) => name.clone(),
    }
}

/// The settings a document was formatted with, as the status bar names them after Format.
pub fn format_summary(lang: Lang, options: &FormatOptions) -> String {
    // Every field named, so that a new setting is not left out of the summary unnoticed.
    let &FormatOptions {
        indent_size,
        tabs,
        max_attributes,
        remove_comments,
        trim_text,
    } = options;
    let mut summary = if tabs {
        lang.summary_tabs().to_owned()
    } else {
        lang.summary_indent(&indent_size.to_string())
    };
    summary = match max_attributes {
        0 => lang.summary_never_wrapped(&summary),
        n => lang.summary_at_most(&summary, &n.to_string()),
    };
    if remove_comments {
        summary = lang.status_comments_removed(&summary);
    }
    if trim_text {
        summary = lang.status_text_trimmed(&summary);
    }
    summary
}

/// Why a structured edit cannot be run.
pub fn command_error(lang: Lang, error: &CommandError) -> String {
    match error {
        CommandError::NotAnElement => lang.refused_not_element().into(),
        CommandError::RootElement => lang.refused_root().into(),
        CommandError::NotTextOnly(element) => lang.refused_not_text_only(element),
        CommandError::BadName(name) => lang.refused_bad_name(name),
        CommandError::NoAttribute { element, name } => lang.refused_no_attribute(element, name),
        CommandError::NoSibling(element) => lang.refused_no_sibling(element),
        CommandError::NoSuchPath(path) => lang.refused_no_such_path(path),
        CommandError::AmbiguousPath { path, count } => {
            lang.refused_ambiguous_path(path, &count.to_string())
        }
        CommandError::BadPath(path) => lang.refused_bad_path(path),
    }
}

/// Why an edit was refused.
pub fn edit_error(lang: Lang, error: &EditError) -> String {
    match error {
        EditError::OutOfRange { start, end, len } => {
            lang.refused_outside(&start.to_string(), &end.to_string(), &len.to_string())
        }
        EditError::Overlap(offset) => lang.refused_overlap(&offset.to_string()),
        EditError::Empty => lang.refused_no_change().into(),
    }
}

/// Why a document cannot be formatted.
pub fn format_error(lang: Lang, error: &FormatError) -> String {
    match error {
        FormatError::Malformed(e) => lang.refused_format(&parse_error(lang, e)),
    }
}

/// What is wrong with text that is not well-formed, and where. The message itself is still
/// the parser's English: it is translated with the problems (LANG-2).
pub fn parse_error(lang: Lang, error: &ParseError) -> String {
    let ParseError { offset, message } = error;
    lang.refused_at_byte(message, &offset.to_string())
}

/// Why Go to line cannot make a position of its input.
pub fn goto_error(lang: Lang, error: &GoToError) -> String {
    match error {
        GoToError::Empty => lang.goto_empty().into(),
        GoToError::NotANumber(input) => lang.goto_not_a_number(input),
        GoToError::Zero => lang.goto_zero().into(),
        GoToError::PastTheEnd(lines) => lang.goto_past_the_end(*lines as u64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A step of every kind, with non-ASCII names.
    fn every_step() -> Vec<StepName> {
        let element = || "Größe".to_owned();
        let attribute = || "Maß".to_owned();
        let mut steps = vec![
            StepName::Typing,
            StepName::Paste,
            StepName::Cut,
            StepName::Delete,
            StepName::Replace,
            StepName::ReplaceAll(0),
            StepName::ReplaceAll(1),
            StepName::ReplaceAll(1000),
            StepName::Format,
            StepName::Compact,
            StepName::ConvertToUtf8,
            StepName::EditText { element: element() },
            StepName::SetAttribute {
                attribute: attribute(),
                element: element(),
            },
            StepName::RemoveAttribute {
                attribute: attribute(),
                element: element(),
            },
            StepName::Rename {
                from: element(),
                to: "Länge".into(),
            },
            StepName::Duplicate { element: element() },
            StepName::MoveUp { element: element() },
            StepName::MoveDown { element: element() },
            StepName::Other("{eigener} Name".into()),
        ];
        for node in [
            DeletedNode::Element(element()),
            DeletedNode::Comment,
            DeletedNode::ProcessingInstruction,
            DeletedNode::CData,
            DeletedNode::Text,
        ] {
            steps.push(StepName::DeleteNode(node));
        }
        steps
    }

    /// The window's English is the engine's, which the CLI and the MCP tools give (LANG-3).
    #[test]
    fn english_is_the_engines_own_words() {
        for step in every_step() {
            assert_eq!(step_name(Lang::En, &step), step.to_string(), "{step:?}");
        }
        for bits in 0..8 {
            for max_attributes in [0, 1, 6] {
                let options = FormatOptions {
                    indent_size: 3,
                    tabs: bits & 1 != 0,
                    max_attributes,
                    remove_comments: bits & 2 != 0,
                    trim_text: bits & 4 != 0,
                };
                assert_eq!(format_summary(Lang::En, &options), options.to_string());
            }
        }
    }

    /// Every step but a caller's own is German; names from the document stay as they are.
    #[test]
    fn every_step_is_german_with_the_documents_names() {
        for step in every_step() {
            let german = step_name(Lang::De, &step);
            match &step {
                StepName::Other(name) => assert_eq!(&german, name),
                _ => assert_ne!(german, step.to_string(), "{step:?}"),
            }
            for name in ["Größe", "Maß", "Länge"] {
                assert_eq!(
                    german.contains(name),
                    step.to_string().contains(name),
                    "{german}"
                );
            }
        }
        let rename = StepName::Rename {
            from: "a".into(),
            to: "b".into(),
        };
        assert_eq!(step_name(Lang::De, &rename), "<a> in <b> umbenennen");
        let options = FormatOptions {
            indent_size: 2,
            tabs: false,
            max_attributes: 0,
            remove_comments: true,
            trim_text: false,
        };
        assert_eq!(
            format_summary(Lang::De, &options),
            "Einrückung 2, Attribute nie umbrochen, Kommentare entfernt"
        );
    }

    /// Every reason in both languages: the window's English is the engine's, and the German
    /// keeps the names, paths and numbers it was given.
    fn refusal<E: std::fmt::Display>(word: fn(Lang, &E) -> String, error: E, given: &[&str]) {
        assert_eq!(word(Lang::En, &error), error.to_string());
        let german = word(Lang::De, &error);
        assert_ne!(german, error.to_string());
        for part in given {
            assert!(german.contains(part), "{part} in {german}");
        }
    }

    #[test]
    fn refused_commands_and_edits() {
        let name = || "Größe".to_owned();
        let path = || "/Wurzel/Maß[2]".to_owned();
        for (error, given) in [
            (CommandError::NotAnElement, &[][..]),
            (CommandError::RootElement, &[]),
            (CommandError::NotTextOnly(name()), &["<Größe>"]),
            (CommandError::BadName("1 {x}".into()), &["1 {x}"]),
            (
                CommandError::NoAttribute {
                    element: name(),
                    name: "Maß".into(),
                },
                &["<Größe>", "Maß"],
            ),
            (CommandError::NoSibling(name()), &["<Größe>"]),
            (CommandError::NoSuchPath(path()), &["/Wurzel/Maß[2]"]),
            (
                CommandError::AmbiguousPath {
                    path: path(),
                    count: 1234,
                },
                &["/Wurzel/Maß[2]", "1234", "[n]"],
            ),
            (
                CommandError::BadPath("Maß".into()),
                &["Maß", "/root/child[2]"],
            ),
        ] {
            refusal(command_error, error, given);
        }
        let outside = EditError::OutOfRange {
            start: 11,
            end: 22,
            len: 3,
        };
        refusal(edit_error, outside, &["11..22", "3 Bytes"]);
        refusal(edit_error, EditError::Overlap(7), &["7"]);
        refusal(edit_error, EditError::Empty, &[]);
        let broken = || ParseError {
            offset: 42,
            message: "element <Größe> is not closed".into(),
        };
        refusal(parse_error, broken(), &["<Größe>", "bei Byte 42"]);
        refusal(
            format_error,
            FormatError::Malformed(broken()),
            &["nicht wohlgeformt", "bei Byte 42"],
        );
    }

    #[test]
    fn refused_lines_to_go_to() {
        refusal(goto_error, GoToError::Empty, &[]);
        refusal(
            goto_error,
            GoToError::NotANumber("zwölf".into()),
            &["zwölf"],
        );
        refusal(goto_error, GoToError::Zero, &[]);
        refusal(goto_error, GoToError::PastTheEnd(4000), &["4000"]);
        // One line is where the window says it better than the engine's "1 lines".
        let one = GoToError::PastTheEnd(1);
        assert_eq!(goto_error(Lang::En, &one), "The file has 1 line");
        assert_eq!(goto_error(Lang::De, &one), "Die Datei hat 1 Zeile");
    }
}
