//! What the engine says, in the window's language (`docs/spec/language.md`, LANG-2). The
//! engine gives values, whose own words are the English the CLI and the MCP tools use; here
//! each is worded by the window. The matches name every case, so that a new one does not
//! build until it is given its words.

use xml_atlas_engine::command::CommandError;
use xml_atlas_engine::problems::Message;
use xml_atlas_engine::text::GoToError;
use xml_atlas_engine::xml::{IllFormedError, ParseError, ParseReason, SyntaxError};
use xml_atlas_engine::xpath::{
    EvalReason, Found, Operand, SyntaxReason, ValueKind, Wanted, XPathError,
};
use xml_atlas_engine::{
    DeletedNode, EditError, FormatError, FormatOptions, IgnoredEncoding, StepName,
};

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

/// What is wrong with text that is not well-formed, and where.
pub fn parse_error(lang: Lang, error: &ParseError) -> String {
    let ParseError { offset, reason } = error;
    lang.refused_at_byte(&parse_reason(lang, reason), &offset.to_string())
}

/// A problem, as the problems list, the status bar and the text view's hint show it.
pub fn problem(lang: Lang, message: &Message) -> String {
    match message {
        Message::Parse(reason) => parse_reason(lang, reason),
        Message::IgnoredEncoding(IgnoredEncoding::NotUtf16 { label }) => {
            lang.problem_not_utf16(label)
        }
        Message::IgnoredEncoding(IgnoredEncoding::Unknown { label }) => {
            lang.problem_unknown_encoding(label)
        }
        // The mark comes from here: the fonts lack it (LANG-15's test of the texts).
        Message::InvalidBytes { encoding } => lang.problem_invalid_bytes(encoding, "\u{FFFD}"),
        Message::SavedAsUtf8 { declared } => lang.problem_saved_as_utf8(declared),
        // Still the engine's English: references are translated with the rules files.
        Message::Reference(words) => words.clone(),
    }
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

/// Why an XPath expression gives no value, with the character where it goes wrong when
/// it cannot be read.
pub fn xpath_error(lang: Lang, error: &XPathError) -> String {
    match error {
        XPathError::Syntax { reason, at } => {
            lang.xpath_error_at(&xpath_syntax(lang, reason), &at.to_string())
        }
        XPathError::Failed(EvalReason::UndeclaredPrefix(prefix)) => lang.xp_no_prefix(prefix),
        XPathError::Failed(EvalReason::NeedsNodeSet { what, got }) => {
            let what = match what {
                Operand::Union => lang.xp_code("|"),
                Operand::Predicate => lang.xp_a_predicate().into(),
                Operand::Path => lang.xp_a_path().into(),
                Operand::Count => lang.xp_code("count()"),
                Operand::Sum => lang.xp_code("sum()"),
                Operand::NameFunction => lang.xp_the_function().into(),
            };
            let got = match got {
                ValueKind::NodeSet => lang.xp_got_node_set(),
                ValueKind::Number => lang.xp_got_number(),
                ValueKind::String => lang.xp_got_string(),
                ValueKind::Boolean => lang.xp_got_boolean(),
            };
            lang.xp_needs_node_set(&what, got)
        }
        XPathError::Stopped => lang.xp_stopped().into(),
    }
}

/// Why an expression cannot be read, without where.
fn xpath_syntax(lang: Lang, reason: &SyntaxReason) -> String {
    let found = |found: &Found| match found {
        Found::Code(code) => lang.xp_code(code),
        Found::Literal(text) => lang.xp_literal(text),
        Found::Number => lang.xp_a_number().into(),
    };
    match reason {
        SyntaxReason::Empty => lang.xp_empty().into(),
        SyntaxReason::Unexpected(what) => lang.xp_unexpected(&found(what)),
        SyntaxReason::UnclosedString => lang.xp_unclosed_string().into(),
        SyntaxReason::NotANumber(text) => lang.xp_not_a_number(text),
        SyntaxReason::NameAfterDollar => lang.xp_name_after_dollar().into(),
        SyntaxReason::NameAfterColon => lang.xp_name_after_colon().into(),
        SyntaxReason::OperatorExpected(name) => lang.xp_operator_expected(name),
        SyntaxReason::Expected {
            wanted,
            found: what,
        } => {
            let wanted = match wanted {
                Wanted::Symbol(symbol) => lang.xp_code(symbol),
                Wanted::CommaOrParen => lang.xp_comma_or_paren().into(),
                Wanted::Expression => lang.xp_an_expression().into(),
                Wanted::Step => lang.xp_a_step().into(),
            };
            match what {
                Some(what) => lang.xp_expected(&wanted, &found(what)),
                None => lang.xp_expected_at_end(&wanted),
            }
        }
        SyntaxReason::TooDeep => lang.xp_too_deep().into(),
        SyntaxReason::Variable(name) => lang.xp_variable(name),
        SyntaxReason::NoFunction(name) => lang.xp_no_function(name),
        SyntaxReason::ArgumentCount {
            function,
            min,
            max,
            given,
        } => {
            let given = given.to_string();
            match max {
                None => lang.xp_at_least(function, &min.to_string(), &given),
                Some(max) if max == min => lang.xp_arguments(*min as u64, function, &given),
                Some(max) => lang.xp_between(function, &min.to_string(), &max.to_string(), &given),
            }
        }
        SyntaxReason::NoAxis(name) => lang.xp_no_axis(name),
    }
}

/// What is wrong, without where.
fn parse_reason(lang: Lang, reason: &ParseReason) -> String {
    match reason {
        ParseReason::NotClosed(name) => lang.problem_not_closed(name),
        ParseReason::NoOpenElement(name) => lang.problem_no_open_element(name),
        ParseReason::NameTooLong { max } => lang.problem_name_too_long(&max.to_string()),
        ParseReason::NotUtf8(e) => {
            let index = e.valid_up_to().to_string();
            let why = match e.error_len() {
                Some(length) => lang.problem_utf8_invalid(&length.to_string(), &index),
                None => lang.problem_utf8_incomplete(&index),
            };
            lang.problem_not_utf8(&why)
        }
        ParseReason::Syntax(e) => lang.problem_syntax(syntax_error(lang, e)),
        ParseReason::IllFormed(e) => lang.problem_ill_formed(&ill_formed_error(lang, e)),
        // An error XML Atlas does not know comes as the library words it (LANG-4).
        ParseReason::Library(words) => words.clone(),
    }
}

fn syntax_error(lang: Lang, error: &SyntaxError) -> &'static str {
    match error {
        SyntaxError::InvalidBangMarkup => lang.syntax_bang_markup(),
        SyntaxError::UnclosedPI => lang.syntax_unclosed_instruction(),
        SyntaxError::UnclosedXmlDecl => lang.syntax_unclosed_declaration(),
        SyntaxError::UnclosedComment => lang.syntax_unclosed_comment(),
        SyntaxError::UnclosedDoctype => lang.syntax_unclosed_doctype(),
        SyntaxError::UnclosedCData => lang.syntax_unclosed_cdata(),
        SyntaxError::UnclosedTag => lang.syntax_unclosed_tag(),
        SyntaxError::UnclosedSingleQuotedAttributeValue => lang.syntax_unclosed_single_quote(),
        SyntaxError::UnclosedDoubleQuotedAttributeValue => lang.syntax_unclosed_double_quote(),
    }
}

fn ill_formed_error(lang: Lang, error: &IllFormedError) -> String {
    match error {
        IllFormedError::MissingDeclVersion(None) => lang.ill_no_version().into(),
        IllFormedError::MissingDeclVersion(Some(attribute)) => {
            lang.ill_version_not_first(attribute)
        }
        IllFormedError::UnknownVersion => lang.ill_unknown_version().into(),
        IllFormedError::MissingDoctypeName => lang.ill_no_doctype_name().into(),
        IllFormedError::MissingEndTag(name) => lang.ill_missing_end_tag(name),
        IllFormedError::UnmatchedEndTag(name) => lang.ill_unmatched_end_tag(name),
        IllFormedError::MismatchedEndTag { expected, found } => {
            lang.ill_mismatched_end_tag(expected, found)
        }
        IllFormedError::DoubleHyphenInComment => lang.ill_double_hyphen().into(),
        IllFormedError::UnclosedReference => lang.ill_unclosed_reference().into(),
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
            reason: ParseReason::NotClosed("Größe".into()),
        };
        refusal(parse_error, broken(), &["<Größe>", "bei Byte 42"]);
        refusal(
            format_error,
            FormatError::Malformed(broken()),
            &["nicht wohlgeformt", "bei Byte 42"],
        );
    }

    /// Every problem there is but a reference's, which is still the engine's words.
    #[test]
    fn xml_problems_and_encoding_warnings() {
        let name = || "Größe".to_owned();
        let broken = |src: &[u8]| {
            let error = std::str::from_utf8(src).unwrap_err();
            Message::Parse(ParseReason::NotUtf8(error))
        };
        for (message, given) in [
            (broken(b"ab\xFFc"), &["1 Byte ab Position 2"][..]),
            (broken(b"abc\xC3"), &["ab Position 3"]),
            (
                Message::IgnoredEncoding(IgnoredEncoding::NotUtf16 {
                    label: "UTF-16".into(),
                }),
                &["UTF-16"],
            ),
            (
                Message::IgnoredEncoding(IgnoredEncoding::Unknown {
                    label: "klingon".into(),
                }),
                &["klingon"],
            ),
            (
                Message::InvalidBytes {
                    encoding: "UTF-16LE".into(),
                },
                &["UTF-16LE", "\u{FFFD}"],
            ),
            (
                Message::SavedAsUtf8 {
                    declared: "windows-1252".into(),
                },
                &["windows-1252"],
            ),
        ] {
            refusal(problem, message, given);
        }
        let syntax = [
            SyntaxError::InvalidBangMarkup,
            SyntaxError::UnclosedPI,
            SyntaxError::UnclosedXmlDecl,
            SyntaxError::UnclosedComment,
            SyntaxError::UnclosedDoctype,
            SyntaxError::UnclosedCData,
            SyntaxError::UnclosedTag,
            SyntaxError::UnclosedSingleQuotedAttributeValue,
            SyntaxError::UnclosedDoubleQuotedAttributeValue,
        ];
        for e in syntax {
            refusal(problem, Message::Parse(ParseReason::Syntax(e)), &[]);
        }
        for (e, given) in [
            (IllFormedError::MissingDeclVersion(None), &["version"][..]),
            (
                IllFormedError::MissingDeclVersion(Some("Maß".into())),
                &["Maß"],
            ),
            (IllFormedError::UnknownVersion, &["1.1"]),
            (IllFormedError::MissingDoctypeName, &["<!DOCTYPE>"]),
            (IllFormedError::MissingEndTag(name()), &["</Größe>"]),
            (IllFormedError::UnmatchedEndTag(name()), &["</Größe>"]),
            (
                IllFormedError::MismatchedEndTag {
                    expected: name(),
                    found: "Maß".into(),
                },
                &["</Größe>", "</Maß>"],
            ),
            (IllFormedError::DoubleHyphenInComment, &["--"]),
            (IllFormedError::UnclosedReference, &[";"]),
        ] {
            refusal(problem, Message::Parse(ParseReason::IllFormed(e)), given);
        }
        for (reason, given) in [
            (ParseReason::NotClosed(name()), &["<Größe>"][..]),
            (ParseReason::NoOpenElement(name()), &["</Größe>"]),
            (ParseReason::NameTooLong { max: 65535 }, &["65535"]),
        ] {
            refusal(problem, Message::Parse(reason), given);
        }
        // What is not known, or not translated yet, is shown as it comes (LANG-4).
        for message in [
            Message::Parse(ParseReason::Library("something new".into())),
            Message::Reference("Reference \"Maß\" finds nothing".into()),
        ] {
            assert_eq!(problem(Lang::De, &message), message.to_string());
        }
    }

    /// Broken files as they are opened, with a byte order mark, CRLF and non-ASCII names:
    /// what the parser finds is German, and the window's English is the engine's.
    #[test]
    fn problems_of_broken_files() {
        let mut seen = Vec::new();
        for src in [
            "\u{feff}<r>\r\n<Größe></r>\r\n".as_bytes(),
            "\u{feff}<r></Maß>\r\n</r>".as_bytes(),
            "<r><!-- Grüße".as_bytes(),
            "<r><![CDATA[ä".as_bytes(),
            "<r><?ziel ö".as_bytes(),
            "<r><a b=\"ü".as_bytes(),
            "<r><a b='ü".as_bytes(),
            "<r><Größe".as_bytes(),
            "<!DOCTYPE r [".as_bytes(),
            "<?xml version=\"1.0\"".as_bytes(),
            "<r><!x".as_bytes(),
            "<r>a &amp b</r>".as_bytes(),
            b"<r>\r\n<a>\xFF</a></r>",
        ] {
            let document = xml_atlas_engine::Document::from_bytes(src.to_vec());
            let problems = document.problems();
            let shown = String::from_utf8_lossy(src);
            assert!(!problems.is_empty(), "{shown}");
            for p in problems {
                assert_eq!(problem(Lang::En, &p.message), p.message.to_string());
                assert_ne!(
                    problem(Lang::De, &p.message),
                    p.message.to_string(),
                    "{shown}"
                );
                seen.push(problem(Lang::De, &p.message));
            }
        }
        for start in ["Element <Größe>", "End-Tag </Maß>", "Syntaxfehler: "] {
            assert!(seen.iter().any(|s| s.starts_with(start)), "{seen:#?}");
        }
    }

    /// Every reason of XPath, got from the engine: the window's English is its English.
    #[test]
    fn xpath_errors() {
        let document = xml_atlas_engine::Document::from_bytes(
            "\u{feff}<r>\r\n  <Größe a=\"1\">12</Größe>\r\n</r>\r\n".into(),
        );
        let deep = "(".repeat(300);
        for expr in [
            "",
            "a ! b",
            "'it''s'",
            "1 2",
            "'open",
            "$ ",
            "p:",
            "a Größe",
            "//a[@id='1'",
            "/a/",
            "count(//r",
            "//a[@id='1' and]",
            "$x",
            "maß()",
            "seitwärts::a",
            "count()",
            "true(1)",
            "substring('a')",
            "string(1,2)",
            "concat('a')",
            "//r | 1",
            "(1)[1]",
            "\"s\"/a",
            "count('x')",
            "sum(1)",
            "name(1)",
            "local-name(true())",
            "//q:Größe",
            &deep,
        ] {
            let error = xml_atlas_engine::xpath::evaluate(&document, expr).expect_err(expr);
            assert_eq!(xpath_error(Lang::En, &error), error.to_string(), "{expr}");
            assert_ne!(xpath_error(Lang::De, &error), error.to_string(), "{expr}");
        }
        refusal(xpath_error, XPathError::Stopped, &[]);
        // Cases the expressions above do not reach.
        for found in [
            Found::Code("]".into()),
            Found::Literal("Maß".into()),
            Found::Number,
        ] {
            let reason = SyntaxReason::Expected {
                wanted: Wanted::CommaOrParen,
                found: Some(found),
            };
            refusal(
                xpath_error,
                XPathError::Syntax {
                    reason: Box::new(reason),
                    at: 3,
                },
                &["Position 3"],
            );
        }
        for got in [
            ValueKind::NodeSet,
            ValueKind::Number,
            ValueKind::String,
            ValueKind::Boolean,
        ] {
            let reason = EvalReason::NeedsNodeSet {
                what: Operand::Path,
                got,
            };
            refusal(xpath_error, XPathError::Failed(reason), &["Knotenmenge"]);
        }
        let german = |expr| {
            let error = xml_atlas_engine::xpath::evaluate(&document, expr).unwrap_err();
            xpath_error(Lang::De, &error)
        };
        assert_eq!(
            german("count()"),
            "„count()“ erwartet 1 Argument, nicht 0 an Position 1"
        );
        assert_eq!(
            german("//q:Größe"),
            "das Präfix „q“ ist im Wurzelelement nicht deklariert"
        );
        assert_eq!(
            german("count('x')"),
            "„count()“ braucht eine Knotenmenge, nicht eine Zeichenkette"
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
