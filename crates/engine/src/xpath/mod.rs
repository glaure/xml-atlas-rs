//! XPath 1.0 queries on a document (`docs/spec/xpath.md`, XP-2, XP-4, XP-6; D35).
//!
//! [`XPath::parse`] reads an expression; [`XPath::evaluate`] runs it on a document's source
//! and index, on any thread, and stops when asked. The result is a [`Value`]: a node-set, a
//! number, a string or a boolean. Nodes are [`Item`]s: a node of the index, or an attribute,
//! namespace node or folded text that the index keeps inside its element.
//!
//! Where it differs from the recommendation, on purpose:
//! - Text that is only whitespace is not a node, as the index does not keep it.
//! - A name without a prefix also matches elements in the default namespace the root element
//!   declares (D35); prefixes mean what the root element declares them as.
//! - `id()` finds elements by `xml:id`; without a DTD nothing else is known to be an ID.

mod eval;
mod parse;

use std::ops::Range;
use std::sync::atomic::AtomicBool;

use xml_atlas_xml::{Index, NONE, NodeId, NodeKind, ROOT};

use crate::Document;

/// A parsed XPath 1.0 expression.
#[derive(Clone, Debug)]
pub struct XPath {
    expr: parse::Expr,
    text: String,
}

/// What an expression gives (§1).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Nodes in document order, without doubles.
    Nodes(Vec<Item>),
    Number(f64),
    String(String),
    Boolean(bool),
}

/// A node in a result. Ordered by document order through [`Item::order`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    /// A node of the index: the root, an element, a text node (the first of adjacent text
    /// and CDATA), a comment or a processing instruction.
    Node(NodeId),
    /// The element's attribute with this number, counted as [`Index::attribute_pairs`] does.
    Attribute(NodeId, u32),
    /// The element's namespace node with this number among those in scope on it.
    Namespace(NodeId, u32),
    /// The text of an element whose whole content is one text or CDATA section (D11).
    Folded(NodeId),
}

/// The kind of an [`Item`], as XPath names nodes (§5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Root,
    Element,
    Attribute,
    Namespace,
    Text,
    Comment,
    ProcessingInstruction,
}

/// Why an expression gave no value.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum XPathError {
    /// The expression cannot be read; `at` is the character where it goes wrong, from 1.
    #[error("{message} at {at}")]
    Syntax { message: String, at: usize },
    /// It can be read but not evaluated, such as a step from a number.
    #[error("{0}")]
    Failed(String),
    /// [`XPath::evaluate`] was asked to stop.
    #[error("the query was stopped")]
    Stopped,
}

impl XPath {
    /// Reads `text` as an XPath 1.0 expression.
    pub fn parse(text: &str) -> Result<Self, XPathError> {
        Ok(Self {
            expr: parse::parse(text)?,
            text: text.to_string(),
        })
    }

    /// The expression as typed.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Evaluates the expression with the root node as context. Checks `stop` while it walks
    /// and gives [`XPathError::Stopped`] once it is set.
    pub fn evaluate(
        &self,
        src: &[u8],
        index: &Index,
        stop: &AtomicBool,
    ) -> Result<Value, XPathError> {
        eval::Eval::new(src, index, stop).run(&self.expr)
    }
}

/// Parses and evaluates `expr` on `doc`, to the end.
pub fn evaluate(doc: &Document, expr: &str) -> Result<Value, XPathError> {
    XPath::parse(expr)?.evaluate(&doc.src, &doc.index, &AtomicBool::new(false))
}

impl Value {
    fn type_name(&self) -> &'static str {
        match self {
            Value::Nodes(_) => "a node-set",
            Value::Number(_) => "a number",
            Value::String(_) => "a string",
            Value::Boolean(_) => "a boolean",
        }
    }

    /// A value that is not a node-set, written as `string()` writes it (§4.2): `NaN`,
    /// `Infinity`, `12`, `0.5`, `true`. `None` for a node-set.
    pub fn to_text(&self) -> Option<String> {
        match self {
            Value::Nodes(_) => None,
            Value::Number(x) => Some(eval::number_to_string(*x)),
            Value::String(s) => Some(s.clone()),
            Value::Boolean(b) => Some(b.to_string()),
        }
    }
}

impl Item {
    /// A key in document order (§5): an element, then its namespace nodes, its attributes,
    /// its folded text, then the nodes after it.
    pub fn order(self) -> u64 {
        let (id, sub) = match self {
            Item::Node(id) => (id, 0),
            Item::Namespace(e, k) => (e, 1 + k.min(0xF_FFFF)),
            Item::Attribute(e, k) => (e, 0x10_0000 + k.min(0xFFF_FFFF)),
            Item::Folded(e) => (e, u32::MAX),
        };
        (u64::from(id) << 32) | u64::from(sub)
    }

    pub fn kind(self, index: &Index) -> ItemKind {
        match self {
            Item::Attribute(..) => ItemKind::Attribute,
            Item::Namespace(..) => ItemKind::Namespace,
            Item::Folded(_) => ItemKind::Text,
            Item::Node(id) => match index.node(id).kind {
                NodeKind::Document => ItemKind::Root,
                NodeKind::Element => ItemKind::Element,
                NodeKind::Text | NodeKind::CData => ItemKind::Text,
                NodeKind::ProcessingInstruction => ItemKind::ProcessingInstruction,
                // The XML declaration and the DOCTYPE are never in a result.
                NodeKind::Comment | NodeKind::Declaration | NodeKind::DocType => ItemKind::Comment,
            },
        }
    }

    /// The node the tree and grid select for this item: the element itself, the element of
    /// an attribute, namespace node or folded text, the parent of other nodes.
    pub fn element(self, index: &Index) -> NodeId {
        match self {
            Item::Attribute(e, _) | Item::Namespace(e, _) | Item::Folded(e) => e,
            Item::Node(id) => match index.node(id).kind {
                NodeKind::Element | NodeKind::Document => id,
                _ => match index.node(id).parent {
                    NONE => ROOT,
                    p => p,
                },
            },
        }
    }

    /// The bytes of the item in the source, to show it in the text: an attribute from its
    /// name to its closing quote, folded text between the tags, a text node with the text
    /// and CDATA after it, a namespace node as its element's start tag.
    pub fn span(self, src: &[u8], index: &Index) -> Range<usize> {
        match self {
            Item::Node(id) => {
                let node = index.node(id);
                let mut span = node.span();
                if matches!(node.kind, NodeKind::Text | NodeKind::CData) {
                    let mut next = index.next_sibling(id);
                    while let Some(s) = next
                        .filter(|&s| matches!(index.node(s).kind, NodeKind::Text | NodeKind::CData))
                    {
                        span.end = index.node(s).span().end;
                        next = index.next_sibling(s);
                    }
                }
                span
            }
            Item::Attribute(e, k) => index
                .attribute_spans(src, e)
                .get(k as usize)
                .cloned()
                .unwrap_or_else(|| index.node(e).span()),
            Item::Namespace(e, _) => {
                let n = index.node(e);
                n.start as usize..n.start as usize + n.head_len as usize
            }
            Item::Folded(e) => index
                .folded_span(src, e)
                .unwrap_or_else(|| index.node(e).span()),
        }
    }

    /// Where the item is, as a path: `/orders/order[3]/@id`, `/r/a/text()`.
    pub fn path(self, src: &[u8], index: &Index) -> String {
        let element_path = |id: NodeId| {
            if id == ROOT {
                String::new()
            } else {
                index.path(src, id)
            }
        };
        let stop = AtomicBool::new(false);
        let ev = eval::Eval::new(src, index, &stop);
        match self {
            Item::Node(id) if id == ROOT => "/".into(),
            Item::Node(id) => {
                let node = index.node(id);
                let parent = self.element(index);
                let step = match node.kind {
                    NodeKind::Element => return index.path(src, id),
                    NodeKind::Comment => "comment()".to_string(),
                    NodeKind::ProcessingInstruction => {
                        format!("processing-instruction('{}')", ev.name_of(self))
                    }
                    _ => "text()".to_string(),
                };
                // The position among the parent's children of the same kind, when there are
                // more than one.
                let same: Vec<Item> = children_of(index, parent)
                    .into_iter()
                    .filter(|c| c.kind(index) == self.kind(index))
                    .collect();
                let at = same.iter().position(|&c| c == self).map_or(1, |p| p + 1);
                if same.len() > 1 {
                    format!("{}/{step}[{at}]", element_path(parent))
                } else {
                    format!("{}/{step}", element_path(parent))
                }
            }
            Item::Attribute(e, _) => format!("{}/@{}", element_path(e), ev.name_of(self)),
            Item::Namespace(e, _) => {
                let prefix = ev.name_of(self);
                let prefix = if prefix.is_empty() {
                    "*[name()='']".into()
                } else {
                    prefix
                };
                format!("{}/namespace::{prefix}", element_path(e))
            }
            Item::Folded(e) => format!("{}/text()", element_path(e)),
        }
    }

    /// The item's name: an element's or attribute's as written, a processing instruction's
    /// target, a namespace node's prefix; empty for others.
    pub fn name(self, src: &[u8], index: &Index) -> String {
        let stop = AtomicBool::new(false);
        eval::Eval::new(src, index, &stop)
            .name_of(self)
            .into_owned()
    }

    /// The item's string-value on one line, at most `max` characters, for a list.
    pub fn value(self, src: &[u8], index: &Index, max: usize) -> String {
        let stop = AtomicBool::new(false);
        let mut ev = eval::Eval::new(src, index, &stop);
        let text = ev
            .string_value_upto(self, max.saturating_mul(4))
            .unwrap_or_default();
        let mut out = String::new();
        let mut space = false;
        for c in text.chars() {
            if c.is_whitespace() {
                space = !out.is_empty();
                continue;
            }
            if space {
                out.push(' ');
                space = false;
            }
            if out.chars().count() >= max {
                out.push('…');
                break;
            }
            out.push(c);
        }
        out
    }
}

/// The children of `id` as XPath sees them (see `Eval::children`).
fn children_of(index: &Index, id: NodeId) -> Vec<Item> {
    let node = index.node(id);
    if node.kind == NodeKind::Element && node.folded != xml_atlas_xml::Folded::No {
        return vec![Item::Folded(id)];
    }
    let mut out = Vec::new();
    let mut text = false;
    for c in index.children(id) {
        let kind = index.node(c).kind;
        if matches!(kind, NodeKind::Declaration | NodeKind::DocType) {
            continue;
        }
        let t = matches!(kind, NodeKind::Text | NodeKind::CData);
        if !(t && text) {
            out.push(Item::Node(c));
        }
        text = t;
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;

    const DOC: &str = r#"<?xml version="1.0"?>
<!DOCTYPE doc>
<doc xml:lang="en">
  <chapter id="c1"><title>One</title><para type="warning">p1</para><para>p2</para></chapter>
  <chapter id="c2"><title>Two</title><para>p3</para><!-- note --><para type="warning">p4 &amp; more</para>
    <section><para>p5</para></section></chapter>
  <?render fast?>
  <appendix xml:lang="de-AT"><para>p6</para></appendix>
</doc>
"#;

    /// What `expr` gives on `src`: each node's path, or the value.
    fn q(src: &str, expr: &str) -> Vec<String> {
        let doc = Document::from_bytes(src.as_bytes().to_vec());
        match evaluate(&doc, expr) {
            Ok(Value::Nodes(nodes)) => nodes.iter().map(|n| n.path(&doc.src, &doc.index)).collect(),
            Ok(v) => vec![v.to_text().unwrap()],
            Err(e) => vec![format!("error: {e}")],
        }
    }

    /// The string-values of what `expr` selects.
    fn values(src: &str, expr: &str) -> Vec<String> {
        let doc = Document::from_bytes(src.as_bytes().to_vec());
        let Value::Nodes(nodes) = evaluate(&doc, expr).unwrap() else {
            panic!("{expr} is not a node-set")
        };
        nodes
            .iter()
            .map(|n| n.value(&doc.src, &doc.index, 100))
            .collect()
    }

    fn one(src: &str, expr: &str) -> String {
        let mut v = q(src, expr);
        assert_eq!(v.len(), 1, "{expr}: {v:?}");
        v.remove(0)
    }

    #[test]
    fn location_paths() {
        assert_eq!(
            q(DOC, "/doc/chapter"),
            ["/doc/chapter[1]", "/doc/chapter[2]"]
        );
        assert_eq!(values(DOC, "/doc/chapter[2]/para"), ["p3", "p4 & more"]);
        assert_eq!(
            values(DOC, "//para"),
            ["p1", "p2", "p3", "p4 & more", "p5", "p6"]
        );
        // Each parent's first `para`, against the first `para` of the document.
        assert_eq!(values(DOC, "//para[1]"), ["p1", "p3", "p5", "p6"]);
        assert_eq!(values(DOC, "(//para)[1]"), ["p1"]);
        assert_eq!(
            values(DOC, "//para[last()]"),
            ["p2", "p4 & more", "p5", "p6"]
        );
        assert_eq!(values(DOC, "/doc/chapter[title='Two']/@id"), ["c2"]);
        assert_eq!(values(DOC, "//para[@type='warning']"), ["p1", "p4 & more"]);
        assert_eq!(
            values(DOC, "/doc/chapter/para[@type=\"warning\"][1]"),
            ["p1", "p4 & more"]
        );
        assert_eq!(values(DOC, "/doc/chapter/para[1][@type='warning']"), ["p1"]);
        assert_eq!(q(DOC, "//chapter[para]").len(), 2);
        assert_eq!(
            q(DOC, "//*[self::chapter or self::appendix]"),
            ["/doc/chapter[1]", "/doc/chapter[2]", "/doc/appendix"]
        );
        assert_eq!(q(DOC, "/doc/*[last()]"), ["/doc/appendix"]);
        assert_eq!(q(DOC, "/*"), ["/doc"]);
        assert_eq!(q(DOC, "/"), ["/"]);
        assert_eq!(q(DOC, "//section/.."), ["/doc/chapter[2]"]);
        assert_eq!(q(DOC, "//section/ancestor::*"), ["/doc", "/doc/chapter[2]"]);
        assert_eq!(q(DOC, "/doc/chapter[2]/@*"), ["/doc/chapter[2]/@id"]);
        assert_eq!(q(DOC, "//@*").len(), 6);
    }

    #[test]
    fn axes() {
        // A reverse axis counts from the context node: the nearest preceding sibling.
        assert_eq!(
            values(DOC, "//section/preceding-sibling::para[1]"),
            ["p4 & more"]
        );
        assert_eq!(
            values(DOC, "//section/preceding-sibling::para"),
            ["p3", "p4 & more"]
        );
        assert_eq!(
            values(DOC, "//title/following-sibling::para[1]"),
            ["p1", "p3"]
        );
        assert_eq!(values(DOC, "//section/following::para"), ["p6"]);
        assert_eq!(
            values(DOC, "//section/preceding::para"),
            ["p1", "p2", "p3", "p4 & more"]
        );
        assert_eq!(values(DOC, "//section/preceding::para[1]"), ["p4 & more"]);
        assert_eq!(values(DOC, "//section/para/ancestor-or-self::*[2]"), ["p5"]);
        assert_eq!(
            q(DOC, "//para[.='p5']/ancestor-or-self::*"),
            [
                "/doc",
                "/doc/chapter[2]",
                "/doc/chapter[2]/section",
                "/doc/chapter[2]/section/para"
            ]
        );
        assert_eq!(
            q(DOC, "/doc/descendant::section"),
            ["/doc/chapter[2]/section"]
        );
        assert_eq!(
            q(DOC, "//section/descendant-or-self::node()"),
            [
                "/doc/chapter[2]/section",
                "/doc/chapter[2]/section/para",
                "/doc/chapter[2]/section/para/text()"
            ]
        );
        // Attributes: their parent is the element; following starts with its content.
        assert_eq!(q(DOC, "//@type/parent::*")[0], "/doc/chapter[1]/para[1]");
        assert_eq!(
            values(DOC, "/doc/chapter[1]/@id/following::para[1]"),
            ["p1"]
        );
        assert_eq!(
            q(DOC, "/doc/chapter[1]/@id/preceding::*"),
            Vec::<String>::new()
        );
        assert_eq!(q(DOC, "/doc/self::doc"), ["/doc"]);
        assert_eq!(q(DOC, "/doc/self::chapter"), Vec::<String>::new());
        // Folded text has the element as parent and no siblings.
        assert_eq!(
            q(DOC, "//title/text()/.."),
            ["/doc/chapter[1]/title", "/doc/chapter[2]/title"]
        );
        assert_eq!(
            q(DOC, "//title/text()/following-sibling::node()"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn other_nodes() {
        assert_eq!(q(DOC, "//comment()"), ["/doc/chapter[2]/comment()"]);
        assert_eq!(one(DOC, "string(//comment())"), " note ");
        assert_eq!(
            q(DOC, "//processing-instruction('render')"),
            ["/doc/processing-instruction('render')"]
        );
        assert_eq!(one(DOC, "string(//processing-instruction())"), "fast");
        assert_eq!(one(DOC, "name(//processing-instruction())"), "render");
        assert_eq!(q(DOC, "//processing-instruction('other')").len(), 0);
        // Whitespace between elements is not text; folded text is.
        assert_eq!(one(DOC, "count(//text())"), "8");
        assert_eq!(one(DOC, "count(/doc/node())"), "4");
        assert_eq!(one(DOC, "count(/node())"), "1");
    }

    #[test]
    fn functions() {
        let f = |e: &str| one(DOC, e);
        assert_eq!(f("count(//para)"), "6");
        assert_eq!(f("last()"), "1");
        assert_eq!(f("count(//para[position() > 1])"), "2");
        assert_eq!(f("local-name(/doc/@xml:lang)"), "lang");
        assert_eq!(f("name(/doc/@xml:lang)"), "xml:lang");
        assert_eq!(f("namespace-uri(/doc/@xml:lang)"), eval::XML_NAMESPACE);
        assert_eq!(f("name(/nothing)"), "");
        assert_eq!(f("string(/doc/chapter[1])"), "Onep1p2");
        assert_eq!(f("string((//para)[4])"), "p4 & more");
        assert_eq!(f("concat('a', 1, true())"), "a1true");
        assert_eq!(f("starts-with('größe', 'grö')"), "true");
        assert_eq!(f("contains((//para)[4], '&')"), "true");
        assert_eq!(f("substring-before('1999/04/01', '/')"), "1999");
        assert_eq!(f("substring-after('1999/04/01', '/')"), "04/01");
        assert_eq!(f("substring-after('abc', 'x')"), "");
        assert_eq!(f("substring('12345', 2, 3)"), "234");
        assert_eq!(f("substring('12345', 2)"), "2345");
        assert_eq!(f("substring('12345', 1.5, 2.6)"), "234");
        assert_eq!(f("substring('12345', 0, 3)"), "12");
        assert_eq!(f("substring('12345', 0 div 0, 3)"), "");
        assert_eq!(f("substring('12345', 1, 0 div 0)"), "");
        assert_eq!(f("substring('12345', -42, 1 div 0)"), "12345");
        assert_eq!(f("substring('12345', -1 div 0, 1 div 0)"), "");
        assert_eq!(f("substring('größe', 3, 2)"), "öß");
        assert_eq!(f("string-length('größe')"), "5");
        assert_eq!(f("string-length(//title)"), "3");
        assert_eq!(f("normalize-space('  a \t b\n ')"), "a b");
        assert_eq!(f("translate('bar', 'abc', 'ABC')"), "BAr");
        assert_eq!(f("translate('--aaa--', 'abc-', 'ABC')"), "AAA");
        assert_eq!(f("boolean(//para)"), "true");
        assert_eq!(f("boolean(//nothing)"), "false");
        assert_eq!(f("not(0)"), "true");
        assert_eq!(f("boolean('')"), "false");
        assert_eq!(f("boolean(0 div 0)"), "false");
        assert_eq!(f("count(//para[lang('en')])"), "5");
        assert_eq!(f("count(//para[lang('DE')])"), "1");
        assert_eq!(f("count(//para[lang('de-AT')])"), "1");
        assert_eq!(f("count(//para[lang('d')])"), "0");
        assert_eq!(f("sum(//chapter/@id[false()])"), "0");
        assert_eq!(f("floor(-1.5)"), "-2");
        assert_eq!(f("ceiling(1.2)"), "2");
        assert_eq!(f("round(2.5)"), "3");
        assert_eq!(f("round(-2.5)"), "-2");
        assert_eq!(f("round(-0.4)"), "0");
        assert_eq!(f("1 div round(-0.4)"), "-Infinity");
    }

    #[test]
    fn numbers_and_comparisons() {
        let f = |e: &str| one(DOC, e);
        assert_eq!(f("number('  -12.5 ')"), "-12.5");
        assert_eq!(f("number('1e3')"), "NaN");
        assert_eq!(f("number('+1')"), "NaN");
        assert_eq!(f("number('.5')"), "0.5");
        assert_eq!(f("number('5.')"), "5");
        assert_eq!(f("number(true())"), "1");
        assert_eq!(f("1 div 0"), "Infinity");
        assert_eq!(f("0 div 0"), "NaN");
        assert_eq!(f("-0"), "0");
        assert_eq!(f("3.0"), "3");
        assert_eq!(f("2 * 3 - 1"), "5");
        assert_eq!(f("5 mod 2"), "1");
        assert_eq!(f("5 mod -2"), "1");
        assert_eq!(f("-5 mod 2"), "-1");
        assert_eq!(f("100000000000000000000"), "100000000000000000000");
        assert_eq!(f("1 = '1'"), "true");
        assert_eq!(f("1 = '1.0'"), "true");
        assert_eq!(f("'1' = '1.0'"), "false");
        assert_eq!(f("true() = 'x'"), "true");
        assert_eq!(f("'abc' < 'abd'"), "false");
        assert_eq!(f("//para = 'p3'"), "true");
        assert_eq!(f("//para != 'p3'"), "true");
        assert_eq!(f("//nothing != 'p3'"), "false");
        assert_eq!(f("//nothing = false()"), "true");
        assert_eq!(f("//chapter/@id = //chapter[2]/@id"), "true");
        assert_eq!(f("count(//chapter[@id > 0])"), "0");
        assert_eq!(f("2 > //title"), "false");
        assert_eq!(f("(1 < 2) < 1"), "false");
        assert_eq!(f("count(//para | //title | //para)"), "8");
    }

    #[test]
    fn non_ascii_cdata_bom_and_crlf() {
        let src = "\u{feff}<?xml version=\"1.0\"?>\r\n<r>\r\n  <名前 属性=\"値 &#x263A;\">テキスト<![CDATA[<x>]]> and</名前>\r\n  <b><![CDATA[only]]></b>\r\n</r>\r\n";
        assert_eq!(one(src, "string(//名前)"), "テキスト<x> and");
        // Adjacent text and CDATA are one text node.
        assert_eq!(one(src, "count(//名前/text())"), "1");
        assert_eq!(values(src, "//名前/@属性"), ["値 ☺"]);
        assert_eq!(values(src, "//b/text()"), ["only"]);
        assert_eq!(q(src, "//名前/text()"), ["/r/名前/text()"]);
        let doc = Document::from_bytes(src.as_bytes().to_vec());
        let Value::Nodes(n) = evaluate(&doc, "//名前/text()").unwrap() else {
            panic!()
        };
        let span = n[0].span(&doc.src, &doc.index);
        assert_eq!(&doc.src[span], "テキスト<![CDATA[<x>]]> and".as_bytes());
        let Value::Nodes(n) = evaluate(&doc, "//@属性").unwrap() else {
            panic!()
        };
        let span = n[0].span(&doc.src, &doc.index);
        assert_eq!(&doc.src[span], "属性=\"値 &#x263A;\"".as_bytes());
        assert_eq!(doc.line_of(n[0].span(&doc.src, &doc.index).start as u64), 2);
        assert_eq!(
            n[0].element(&doc.index),
            doc.index.root_element().unwrap() + 1
        );
    }

    #[test]
    fn namespaces() {
        let src = r#"<r xmlns="urn:a" xmlns:p="urn:p"><p:x/><y/><z xmlns="urn:other"><y/></z><q:w xmlns:q="urn:p"/><v xmlns=""><y/></v></r>"#;
        // Without a prefix: no namespace, or the root's default one (D35).
        assert_eq!(q(src, "//y"), ["/r/y", "/r/v/y"]);
        assert_eq!(one(src, "count(//p:x)"), "1");
        assert_eq!(one(src, "count(//p:*)"), "2");
        assert_eq!(one(src, "local-name(//p:x)"), "x");
        assert_eq!(one(src, "name(//p:*[2])"), "q:w");
        assert_eq!(one(src, "namespace-uri(/r/y)"), "urn:a");
        assert_eq!(
            one(src, "namespace-uri(//*[local-name()='z']/*)"),
            "urn:other"
        );
        assert_eq!(one(src, "namespace-uri(//v/y)"), "");
        assert_eq!(one(src, "count(//*[local-name()='y'])"), "3");
        assert_eq!(
            one(src, "//q:w"),
            "error: the prefix `q` is not declared on the root element"
        );
        // Declarations are namespace nodes, not attributes.
        assert_eq!(one(src, "count(/r/@*)"), "0");
        assert_eq!(one(src, "count(/r/namespace::*)"), "3");
        assert_eq!(one(src, "count(//v/namespace::*)"), "2");
        assert_eq!(values(src, "/r/namespace::p"), ["urn:p"]);
        assert_eq!(q(src, "/r/namespace::p"), ["/r/namespace::p"]);
        // A document without namespaces.
        assert_eq!(one("<a:b xmlns:a='u'/>", "count(/a:b)"), "1");
        assert_eq!(one("<b/>", "count(/b)"), "1");
        assert_eq!(
            one("<b/>", "//x:b"),
            "error: the prefix `x` is not declared on the root element"
        );
    }

    #[test]
    fn ids() {
        let src = r#"<r><a xml:id="x1"/><b xml:id="x2"/><c id="x3"/></r>"#;
        assert_eq!(q(src, "id('x2 x1')"), ["/r/a", "/r/b"]);
        assert_eq!(q(src, "id('x3')"), Vec::<String>::new());
        assert_eq!(q(src, "id(//nothing)"), Vec::<String>::new());
    }

    #[test]
    fn evaluation_errors() {
        assert_eq!(
            one(DOC, "1/a"),
            "error: a path needs a node-set, not a number"
        );
        assert_eq!(
            one(DOC, "count(1)"),
            "error: `count()` needs a node-set, not a number"
        );
        assert_eq!(
            one(DOC, "//a | 1"),
            "error: `|` needs a node-set, not a number"
        );
        assert_eq!(
            one(DOC, "'a'[1]"),
            "error: a predicate needs a node-set, not a string"
        );
        assert_eq!(
            one(DOC, "//para["),
            "error: expected an expression, found the end at 8"
        );
    }

    #[test]
    fn values_and_paths_for_a_list() {
        let doc = Document::from_bytes(DOC.as_bytes().to_vec());
        let Value::Nodes(n) = evaluate(&doc, "/doc | (//para)[4] | (//para)[4]/text()").unwrap()
        else {
            panic!()
        };
        assert_eq!(n[0].value(&doc.src, &doc.index, 12), "Onep1p2Twop3…");
        assert_eq!(n[1].path(&doc.src, &doc.index), "/doc/chapter[2]/para[2]");
        assert_eq!(n[1].kind(&doc.index), ItemKind::Element);
        assert_eq!(n[2].kind(&doc.index), ItemKind::Text);
        assert_eq!(
            n[2].element(&doc.index),
            match n[1] {
                Item::Node(id) => id,
                _ => unreachable!(),
            }
        );
        assert_eq!(n[1].name(&doc.src, &doc.index), "para");
        // Document order: an element, its attributes, its text, then what follows.
        let el = Item::Node(5);
        assert!(el.order() < Item::Namespace(5, 0).order());
        assert!(Item::Namespace(5, 3).order() < Item::Attribute(5, 0).order());
        assert!(Item::Attribute(5, 9).order() < Item::Folded(5).order());
        assert!(Item::Folded(5).order() < Item::Node(6).order());
    }

    #[test]
    fn stops_when_asked() {
        let mut src = String::from("<r>");
        for i in 0..5000 {
            src.push_str(&format!("<a n=\"{i}\"/>"));
        }
        src.push_str("</r>");
        let doc = Document::from_bytes(src.into_bytes());
        let stop = AtomicBool::new(false);
        let x = XPath::parse("count(//a[@n > 10])").unwrap();
        assert_eq!(
            x.evaluate(&doc.src, &doc.index, &stop),
            Ok(Value::Number(4989.0))
        );
        stop.store(true, Ordering::Relaxed);
        assert_eq!(
            x.evaluate(&doc.src, &doc.index, &stop),
            Err(XPathError::Stopped)
        );
    }
}
