//! What the engine says, in the window's language (`docs/spec/language.md`, LANG-2). The
//! engine gives values, whose own words are the English the CLI and the MCP tools use; here
//! each is worded by the window. The matches name every case, so that a new one does not
//! build until it is given its words.

use xml_atlas_engine::command::CommandError;
use xml_atlas_engine::problems::Message;
use xml_atlas_engine::references::check::{Remark, RuleName};
use xml_atlas_engine::references::{Invalid, PathPart, Reason};
use xml_atlas_engine::rules_folder::{FolderError, ListFault, Unusable};
use xml_atlas_engine::rules_text::TextError;
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
        Message::Reference { reference, why } => {
            lang.problem_reference(&escaped(reference), &reference_reason(lang, why))
        }
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

/// Why a reference stands for nothing, as the status bar and the problems list say it. The
/// reference preview's own comments stay English (LANG-3).
pub fn reference_reason(lang: Lang, reason: &Reason) -> String {
    match reason {
        Reason::FindsNothing => lang.ref_finds_nothing().into(),
        Reason::RuntimeOnly(name) => lang.ref_runtime(name),
        Reason::UnknownContext(name) => lang.ref_unknown_context(name),
        Reason::NothingAtContext { context, path } => lang.ref_nothing_at_context(context, path),
        Reason::LookedForInside { context, up_to } => lang.ref_looked_for_inside(context, up_to),
        Reason::UnclosedVariable => lang.ref_unclosed_variable().into(),
        Reason::AttributeBeforeLastStep => lang.ref_attribute_before_last().into(),
        Reason::EmptyPath => lang.ref_empty_path().into(),
        Reason::UnreadablePath(path) => lang.ref_unreadable_path(&escaped(path)),
        Reason::BracketNotOpened => lang.ref_bracket_not_opened().into(),
        Reason::Unclosed(path) => lang.ref_unclosed(&escaped(path)),
        Reason::CannotRead { part, path } => {
            let part = match part {
                PathPart::AttributeStep(step) => lang.ref_attribute_step(&escaped(step)),
                PathPart::Step(step) => lang.ref_step(&escaped(step)),
                PathPart::Condition(condition) => lang.ref_condition(condition),
            };
            lang.ref_cannot_read(&part, &escaped(path))
        }
    }
}

/// What is wrong with the contents of a rules file. The names of its keys stay (LANG-3).
fn rules_invalid(lang: Lang, invalid: &Invalid) -> String {
    match invalid {
        Invalid::NoObject => lang.rules_no_object().into(),
        Invalid::NotObject(at) => lang.rules_not_object(at),
        Invalid::NotList(at) => lang.rules_not_list(at),
        Invalid::NotString(at) => lang.rules_not_string(at),
        Invalid::NotBool(at) => lang.rules_not_bool(at),
        Invalid::NotBoolOrObject => lang.rules_not_bool_or_object().into(),
        Invalid::PathNotAbsolute(at) => lang.rules_path_not_absolute(at),
        Invalid::NeedsPathOrNearest(at) => lang.rules_needs_path_or_nearest(at),
        Invalid::Needs { at, key } => lang.rules_needs(at, key),
        Invalid::VariableWithoutDollar(at) => lang.rules_variable_dollar(at),
        Invalid::UnknownKind { at, kind } => lang.rules_unknown_kind(at, &escaped(kind)),
    }
}

/// Why something with the rules folder or its list failed.
pub fn folder_error(lang: Lang, error: &FolderError) -> String {
    match error {
        FolderError::NoConfigFolder => lang.rules_no_folder().into(),
        // The operating system's words (LANG-4).
        FolderError::Io { path, error } => format!("{}: {error}", path.display()),
        FolderError::List { path, reason } => {
            let reason = match reason {
                ListFault::NotJson(error) => lang.folder_list_not_json(error),
                ListFault::NoFiles => lang.folder_list_no_files().into(),
                ListFault::IncompleteFile => lang.folder_list_incomplete().into(),
            };
            lang.folder_list_unreadable(&path.display().to_string(), &reason)
        }
        FolderError::Name(name) => lang.folder_bad_name(&escaped(name)),
        FolderError::Exists(name) => lang.folder_exists(name),
        FolderError::Missing(name) => lang.folder_missing(name),
        FolderError::TooLarge(path) => lang.folder_too_large(&path.display().to_string()),
    }
}

/// Why a rules file's rules cannot be used.
pub fn unusable_rules(lang: Lang, why: &Unusable) -> String {
    match why {
        Unusable::File(error) => folder_error(lang, error),
        Unusable::NotText => lang.unusable_not_utf8().into(),
        Unusable::NotJson(error) => lang.unusable_not_json(error),
        Unusable::Invalid(invalid) => rules_invalid(lang, invalid),
        Unusable::Unread { path, error } => {
            lang.unusable_unread(&path.display().to_string(), error)
        }
    }
}

/// Why the rules editor cannot read or save its file.
pub fn text_error(lang: Lang, error: &TextError) -> String {
    let path = |path: &std::path::Path| path.display().to_string();
    match error {
        // The operating system's words (LANG-4).
        TextError::Io { path, error } => format!("{}: {error}", path.display()),
        TextError::NotText(file) => lang.file_not_utf8(&path(file)),
        TextError::TooLarge(file) => lang.folder_too_large(&path(file)),
        TextError::Changed(file) => lang.file_changed(&path(file)),
    }
}

/// What the check of a rules file finds.
pub fn remark(lang: Lang, remark: &Remark) -> String {
    match remark {
        Remark::NotJson(error) => lang.check_not_json(error),
        Remark::Invalid(invalid) => rules_invalid(lang, invalid),
        // The engine's words: no case of the parser's leads here.
        Remark::Other(words) => words.clone(),
        Remark::NoReferences => lang.check_no_references().into(),
        Remark::GivenTwice(key) => lang.check_given_twice(key),
        Remark::UnknownKey { key, meant: None } => lang.check_unknown_key(&escaped(key)),
        Remark::UnknownKey {
            key,
            meant: Some(meant),
        } => lang.check_unknown_key_meant(&escaped(key), meant),
        Remark::SecondInclude(element) => lang.check_second_include(element),
        Remark::MatchesNothing(rule) => lang.check_matches_nothing(&rule_name(lang, rule)),
    }
}

/// A rule as the check names it; a name that is a key of the rules file stays (LANG-3).
pub fn rule_name(lang: Lang, rule: &RuleName) -> String {
    match rule {
        RuleName::Include { element, attribute } => lang.rule_include(element, attribute),
        RuleName::AttributeReferences | RuleName::Repeat => rule.to_string(),
        RuleName::Context(name) => lang.rule_context(name),
        RuleName::RunTimeContext(name) => lang.rule_run_time_context(name),
        RuleName::Variable(name) => lang.rule_variable(name),
    }
}

/// `text` as it stands between the quotes in the engine's words, which write it as Rust
/// writes a string.
fn escaped(text: &str) -> String {
    let quoted = format!("{text:?}");
    quoted[1..quoted.len() - 1].to_owned()
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
    use xml_atlas_engine::references::check::check;
    use xml_atlas_engine::references::{self, Resolution, Rules};

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

    /// Every problem of a file's text and encoding.
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
        // An error of the library that is not known is shown as it comes (LANG-4).
        let message = Message::Parse(ParseReason::Library("something new".into()));
        assert_eq!(problem(Lang::De, &message), message.to_string());
    }

    /// Every reason a reference stands for nothing, got from the engine, in a file with a
    /// byte order mark, CRLF and non-ASCII names: the window's English is the engine's, and
    /// so is every warning in the problems list.
    #[test]
    fn why_references_find_nothing() {
        let rules = Rules::parse(
            r#"{
                "includes": [{"element": "Use", "attribute": "src"}],
                "contexts": {"Weg": {"path": "/Doc/Nirgends"},
                             "Nah": {"nearest": "General", "up_to": "Maß"}},
                "runtime_contexts": ["Rt"]
            }"#,
        )
        .unwrap();
        let paths = [
            "Fehlt",
            "Rt:x",
            "Nirgendwo:x",
            "Weg:x",
            "Nah:x",
            "${abc",
            "$Anders/x",
            "@a/b",
            " ",
            "a//",
            "a]",
            "a[@x='1'",
            "a/@1bad",
            "a/b c",
            "a[Größe]",
            "a[1]x",
            "Größe\\x",
        ];
        let uses: String = paths
            .iter()
            .map(|p| format!("  <Use src=\"{}\"/>\r\n", p.replace('<', "&lt;")))
            .collect();
        let source = format!("\u{feff}<Doc>\r\n{uses}</Doc>\r\n");
        let document = xml_atlas_engine::Document::from_bytes(source.into_bytes());
        let root = document.index.root_element().unwrap();
        let mut reasons = Vec::new();
        for r in references::references(&document, &rules, root) {
            match references::resolve(&document, &rules, &r, r.site).resolution {
                Resolution::Runtime(why)
                | Resolution::Elsewhere(why)
                | Resolution::Unresolved(why) => reasons.push(why),
                other => panic!("{}: {other:?}", r.expr),
            }
        }
        assert_eq!(reasons.len(), paths.len());
        for why in &reasons {
            refusal(reference_reason, why.clone(), &[]);
        }
        let warnings = references::problems(&document, &rules);
        assert!(warnings.len() > 10);
        for p in warnings {
            assert_eq!(problem(Lang::En, &p.message), p.message.to_string());
            let german = problem(Lang::De, &p.message);
            assert!(german.starts_with("Verweis „"), "{german}");
        }
        let german: Vec<_> = reasons
            .iter()
            .map(|why| reference_reason(Lang::De, why))
            .collect();
        for wanted in [
            "findet nichts",
            "Rt ist erst zur Laufzeit bekannt",
            "verwendet den Kontext Weg, aber unter /Doc/Nirgends ist nichts",
            "enthält „${“ ohne „}“",
            "enthält in „a/b c“ den Schritt „b c“; die Vorschau kann das nicht lesen",
        ] {
            assert!(german.iter().any(|g| g == wanted), "{wanted}: {german:#?}");
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

    /// What the check finds in faulty rules files, with the rules it counts: got from the
    /// engine, with non-ASCII names; the window's English is the engine's.
    #[test]
    fn what_the_check_of_rules_says() {
        let document = xml_atlas_engine::Document::from_bytes(
            "\u{feff}<Doc>\r\n  <Größe/>\r\n</Doc>\r\n".into(),
        );
        let include = r#"{"element": "Größe", "attribute": "src"}"#;
        let texts = [
            "[1]".to_owned(),
            r#"{"includes": 1}"#.into(),
            r#"{"includes": [1]}"#.into(),
            r#"{"includes": [{"enabled": 1}]}"#.into(),
            r#"{"includes": [{}]}"#.into(),
            r#"{"includes": [{"element": 1}]}"#.into(),
            r#"{"attribute_references": 1}"#.into(),
            r#"{"contexts": 1}"#.into(),
            r#"{"contexts": {"Maß": {"path": "x"}}}"#.into(),
            r#"{"contexts": {"Maß": {}}}"#.into(),
            r#"{"variables": {"Maß": "parent_name"}}"#.into(),
            r#"{"variables": {"$Maß": "größe"}}"#.into(),
            r#"{"includes": [], }"#.into(),
            "{".into(),
            "{}".into(),
            format!(r#"{{"includez": [], "Maß": 1, "includes": [{include}, {include}]}}"#),
            r#"{"includes": [{"element": "Nie", "attribute": "src"}],
                "attribute_references": true,
                "contexts": {"Weg": {"path": "/Doc/Nirgends"}},
                "runtime_contexts": ["Rt"], "variables": {"$V": "parent_name"},
                "repeat": {"count": "anzahl"}}"#
                .into(),
        ];
        let mut german = Vec::new();
        for text in &texts {
            let report = check(text, Some(&document));
            assert!(!report.findings.is_empty(), "{text}");
            for f in report.findings {
                assert_eq!(remark(Lang::En, &f.message), f.message.to_string());
                // A rule named by its key alone keeps that name; the sentence is German.
                assert_ne!(remark(Lang::De, &f.message), f.message.to_string());
                german.push(remark(Lang::De, &f.message));
            }
            for count in report.counts {
                assert_eq!(rule_name(Lang::En, &count.rule), count.rule.to_string());
                german.push(rule_name(Lang::De, &count.rule));
            }
        }
        for wanted in [
            "die Datei enthält kein JSON-Objekt",
            "variables.$Maß: unbekannte Art „größe“",
            "unbekannter Schlüssel „includez“; er wird ignoriert. Ist „includes“ gemeint?",
            "Einbindung <Nie src> trifft auf nichts im Dokument zu",
            "repeat trifft auf nichts im Dokument zu",
            "Laufzeitkontext Rt",
            "attribute_references",
        ] {
            assert!(german.iter().any(|g| g == wanted), "{wanted}: {german:#?}");
        }
    }

    /// Why a rules file or the folder cannot be used, read or saved.
    #[test]
    fn why_rules_files_cannot_be_used() {
        let path = || std::path::PathBuf::from("/Regeln/Maß.json");
        let list = |reason| FolderError::List {
            path: path(),
            reason,
        };
        for error in [
            FolderError::NoConfigFolder,
            list(ListFault::NotJson("expected value".into())),
            list(ListFault::NoFiles),
            list(ListFault::IncompleteFile),
            FolderError::Name("a/\"Maß\"".into()),
            FolderError::Exists("Maß.json".into()),
            FolderError::Missing("Maß.json".into()),
            FolderError::TooLarge(path()),
        ] {
            refusal(folder_error, error, &[]);
        }
        for why in [
            Unusable::NotText,
            Unusable::NotJson("expected value".into()),
            Unusable::Invalid(Invalid::NotList("includes".into())),
            Unusable::Unread {
                path: path(),
                error: "no".into(),
            },
            Unusable::File(FolderError::TooLarge(path())),
        ] {
            refusal(unusable_rules, why, &[]);
        }
        for error in [
            TextError::NotText(path()),
            TextError::TooLarge(path()),
            TextError::Changed(path()),
        ] {
            // `TextError` holds an `io::Error` in one case, so it is passed as it is.
            assert_eq!(text_error(Lang::En, &error), error.to_string());
            let german = text_error(Lang::De, &error);
            assert!(
                german != error.to_string() && german.contains("Maß"),
                "{german}"
            );
        }
        // The operating system's own words stay (LANG-4).
        let io = FolderError::Io {
            path: path(),
            error: "denied".into(),
        };
        assert_eq!(folder_error(Lang::De, &io), io.to_string());
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
