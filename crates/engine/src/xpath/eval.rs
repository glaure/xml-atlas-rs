//! Evaluating an XPath 1.0 expression on the index (W3C XPath 1.0, §2 to §4; D35).
//!
//! Nodes are stored in document order and a node's subtree is the run of nodes right after
//! it, so the descendant, following and preceding axes are ranges of `NodeId`s, a name test
//! compares the name's bytes in the source, and document order is a comparison of numbers.
//! Names, values and text are decoded only when a test or a function needs them.

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

use xml_atlas_xml::{Folded, Index, NodeId, NodeKind, ROOT, unescape};

use super::parse::{Arith, Axis, Cmp, Expr, Func, Path, Start, Step, Test};
use super::{Item, ItemKind, Value, XPathError};

/// The namespace the prefix `xml` stands for in every document.
pub(super) const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// How often a long walk looks at the stop flag, in nodes.
const STOP_CHECK: u32 = 1024;

/// One evaluation over one document.
pub(super) struct Eval<'d> {
    pub src: &'d [u8],
    pub index: &'d Index,
    stop: &'d AtomicBool,
    ticks: u32,
    /// Whether the source mentions `xmlns` at all; until then, no name has a namespace.
    namespaces: Option<bool>,
}

/// The context of an expression (§1): a node, its position and the size of its set.
#[derive(Clone, Copy)]
struct Ctx {
    item: Item,
    pos: usize,
    size: usize,
}

/// Which kind of node a name test selects on an axis (§2.3).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Principal {
    Element,
    Attribute,
    Namespace,
}

type R<T> = Result<T, XPathError>;

impl<'d> Eval<'d> {
    pub fn new(src: &'d [u8], index: &'d Index, stop: &'d AtomicBool) -> Self {
        Self {
            src,
            index,
            stop,
            ticks: 0,
            namespaces: None,
        }
    }

    /// Evaluates `e` with the root node as context.
    pub fn run(&mut self, e: &Expr) -> R<Value> {
        let ctx = Ctx {
            item: Item::Node(ROOT),
            pos: 1,
            size: 1,
        };
        self.eval(e, ctx)
    }

    fn tick(&mut self) -> R<()> {
        self.ticks = self.ticks.wrapping_add(1);
        if self.ticks.is_multiple_of(STOP_CHECK) && self.stop.load(Ordering::Relaxed) {
            return Err(XPathError::Stopped);
        }
        Ok(())
    }

    fn eval(&mut self, e: &Expr, ctx: Ctx) -> R<Value> {
        Ok(match e {
            Expr::Or(a, b) => {
                let a = self.eval(a, ctx)?;
                Value::Boolean(
                    self.boolean(&a) || {
                        let b = self.eval(b, ctx)?;
                        self.boolean(&b)
                    },
                )
            }
            Expr::And(a, b) => {
                let a = self.eval(a, ctx)?;
                Value::Boolean(
                    self.boolean(&a) && {
                        let b = self.eval(b, ctx)?;
                        self.boolean(&b)
                    },
                )
            }
            Expr::Compare(op, a, b) => {
                let a = self.eval(a, ctx)?;
                let b = self.eval(b, ctx)?;
                Value::Boolean(self.compare(*op, &a, &b)?)
            }
            Expr::Arith(op, a, b) => {
                let a = self.eval(a, ctx)?;
                let a = self.number(&a);
                let b = self.eval(b, ctx)?;
                let b = self.number(&b);
                Value::Number(match op {
                    Arith::Add => a + b,
                    Arith::Sub => a - b,
                    Arith::Mul => a * b,
                    Arith::Div => a / b,
                    // The remainder takes the sign of the dividend, as in C's fmod (§3.5).
                    Arith::Mod => a % b,
                })
            }
            Expr::Neg(a) => {
                let a = self.eval(a, ctx)?;
                Value::Number(-self.number(&a))
            }
            Expr::Union(a, b) => {
                let a = self.nodes(a, ctx, "`|`")?;
                let b = self.nodes(b, ctx, "`|`")?;
                Value::Nodes(union(a, b))
            }
            Expr::Path(p) => Value::Nodes(self.path(p, ctx)?),
            Expr::Filter(primary, predicates) => {
                let mut nodes = self.nodes(primary, ctx, "a predicate")?;
                for p in predicates {
                    nodes = self.filter(nodes, p, false)?;
                }
                Value::Nodes(nodes)
            }
            Expr::Literal(s) => Value::String(s.clone()),
            Expr::Number(v) => Value::Number(*v),
            Expr::Call(f, args) => self.call(*f, args, ctx)?,
        })
    }

    /// Evaluates `e`, which must give a node-set for `what`.
    fn nodes(&mut self, e: &Expr, ctx: Ctx, what: &str) -> R<Vec<Item>> {
        match self.eval(e, ctx)? {
            Value::Nodes(n) => Ok(n),
            other => Err(XPathError::Failed(format!(
                "{what} needs a node-set, not {}",
                other.type_name()
            ))),
        }
    }

    fn path(&mut self, p: &Path, ctx: Ctx) -> R<Vec<Item>> {
        let mut nodes = match &p.start {
            Start::Root => vec![Item::Node(ROOT)],
            Start::Context => vec![ctx.item],
            Start::Filter(e) => self.nodes(e, ctx, "a path")?,
        };
        for step in &p.steps {
            nodes = self.step(&nodes, step)?;
        }
        Ok(nodes)
    }

    /// One step from every node of `input`: the union of what each gives, in document order.
    fn step(&mut self, input: &[Item], step: &Step) -> R<Vec<Item>> {
        let mut out: Vec<Item> = Vec::new();
        let mut sorted = true;
        let mut found = Vec::new();
        for &n in input {
            self.tick()?;
            found.clear();
            self.axis(n, step.axis, &step.test, &mut found)?;
            for p in &step.predicates {
                found = self.filter(std::mem::take(&mut found), p, step.axis.is_reverse())?;
            }
            if let (Some(last), Some(first)) = (out.last(), found.first())
                && first.order() <= last.order()
            {
                sorted = false;
            }
            out.extend_from_slice(&found);
        }
        if !sorted {
            out.sort_unstable_by_key(|i| i.order());
            out.dedup();
        }
        Ok(out)
    }

    /// The nodes of `nodes` for which `predicate` holds (§2.4). A number holds at that
    /// position; positions count backwards on a reverse axis.
    fn filter(&mut self, nodes: Vec<Item>, predicate: &Expr, reverse: bool) -> R<Vec<Item>> {
        let size = nodes.len();
        if let Expr::Number(k) = predicate {
            // `[3]`: no need to evaluate anything per node.
            let pos = *k;
            if pos.fract() != 0.0 || pos < 1.0 || pos > size as f64 {
                return Ok(Vec::new());
            }
            let pos = pos as usize;
            let i = if reverse { size - pos } else { pos - 1 };
            return Ok(vec![nodes[i]]);
        }
        let mut out = Vec::new();
        for (i, &item) in nodes.iter().enumerate() {
            self.tick()?;
            let pos = if reverse { size - i } else { i + 1 };
            let keep = match self.eval(predicate, Ctx { item, pos, size })? {
                Value::Number(v) => v == pos as f64,
                v => self.boolean(&v),
            };
            if keep {
                out.push(item);
            }
        }
        Ok(out)
    }

    // ---- Axes (§2.2) ----

    /// The nodes on `axis` from `n` that pass `test`, in document order.
    fn axis(&mut self, n: Item, axis: Axis, test: &Test, out: &mut Vec<Item>) -> R<()> {
        let principal = match axis {
            Axis::Attribute => Principal::Attribute,
            Axis::Namespace => Principal::Namespace,
            _ => Principal::Element,
        };
        let mut push = |me: &mut Self, c: Item| -> R<()> {
            if me.test(c, principal, test)? {
                out.push(c);
            }
            Ok(())
        };
        match axis {
            Axis::Itself => push(self, n)?,
            Axis::Child => {
                if let Item::Node(id) = n {
                    for c in self.children(id) {
                        self.tick()?;
                        push(self, c)?;
                    }
                }
            }
            Axis::Descendant | Axis::DescendantOrSelf => {
                if axis == Axis::DescendantOrSelf {
                    push(self, n)?;
                }
                if let Item::Node(id) = n {
                    let end = id + 1 + self.index.node(id).descendants;
                    self.range(id + 1, end, test, principal, out)?;
                }
            }
            Axis::Parent => {
                if let Some(p) = self.parent(n) {
                    push(self, p)?;
                }
            }
            Axis::Ancestor | Axis::AncestorOrSelf => {
                let mut chain = Vec::new();
                if axis == Axis::AncestorOrSelf {
                    chain.push(n);
                }
                let mut cur = n;
                while let Some(p) = self.parent(cur) {
                    chain.push(p);
                    cur = p;
                }
                for &c in chain.iter().rev() {
                    push(self, c)?;
                }
            }
            Axis::FollowingSibling => {
                if let Item::Node(id) = n
                    && id != ROOT
                {
                    let mut text = is_text(self.index.node(id).kind);
                    let mut next = self.index.next_sibling(id);
                    while let Some(s) = next {
                        self.tick()?;
                        let t = is_text(self.index.node(s).kind);
                        if xpath_node(self.index.node(s).kind) && !(t && text) {
                            push(self, Item::Node(s))?;
                        }
                        text = t;
                        next = self.index.next_sibling(s);
                    }
                }
            }
            Axis::PrecedingSibling => {
                if let Item::Node(id) = n
                    && id != ROOT
                {
                    let parent = self.index.node(id).parent;
                    for c in self.children(parent) {
                        self.tick()?;
                        if c == Item::Node(id) || c.order() >= Item::Node(id).order() {
                            break;
                        }
                        push(self, c)?;
                    }
                }
            }
            Axis::Following => {
                let len = self.index.len() as NodeId;
                let start = match n {
                    Item::Node(id) => id + 1 + self.index.node(id).descendants,
                    Item::Attribute(e, _) | Item::Namespace(e, _) => {
                        if self.index.node(e).folded != Folded::No {
                            push(self, Item::Folded(e))?;
                        }
                        e + 1
                    }
                    Item::Folded(e) => e + 1,
                };
                self.range(start, len, test, principal, out)?;
            }
            Axis::Preceding => {
                let (before, ancestors) = match n {
                    Item::Node(id) => (id, self.ancestors(id)),
                    Item::Attribute(e, _) | Item::Namespace(e, _) | Item::Folded(e) => {
                        let mut a = self.ancestors(e);
                        a.push(e);
                        (e, a)
                    }
                };
                let mut skip = ancestors.into_iter().peekable();
                // Text right after an ancestor's start continues no text before it, so the
                // run rule of `range` holds for each piece between ancestors.
                let mut from = 1;
                while from < before {
                    let to = match skip.peek() {
                        Some(&a) if a < before => a,
                        _ => before,
                    };
                    if to > from {
                        self.range(from, to, test, principal, out)?;
                    }
                    if skip.peek().is_some_and(|&a| a < before) {
                        skip.next();
                    }
                    from = to + 1;
                }
            }
            Axis::Attribute => {
                if let Item::Node(e) = n
                    && self.index.node(e).kind == NodeKind::Element
                {
                    let names: Vec<(usize, bool)> = self
                        .index
                        .attribute_pairs(self.src, e)
                        .enumerate()
                        .map(|(k, (name, _))| (k, is_namespace_declaration(name)))
                        .collect();
                    for (k, declares) in names {
                        if !declares {
                            push(self, Item::Attribute(e, k as u32))?;
                        }
                    }
                }
            }
            Axis::Namespace => {
                if let Item::Node(e) = n
                    && self.index.node(e).kind == NodeKind::Element
                {
                    for k in 0..self.in_scope(e).len() {
                        push(self, Item::Namespace(e, k as u32))?;
                    }
                }
            }
        }
        Ok(())
    }

    /// The nodes with ids `from..to` (and the folded text of elements among them) that pass
    /// `test`, in document order. Text that continues text right before it is part of that
    /// text node (§5.7), and is skipped.
    fn range(
        &mut self,
        from: NodeId,
        to: NodeId,
        test: &Test,
        principal: Principal,
        out: &mut Vec<Item>,
    ) -> R<()> {
        if principal != Principal::Element {
            // Attributes and namespace nodes are on no range axis.
            return Ok(());
        }
        let wants_text = matches!(test, Test::Node | Test::Text);
        // A name test without a prefix on a document without namespaces: compare the length
        // first, which the index holds, and touch the source only when it fits.
        let name_len = match test {
            Test::Name {
                prefix: None,
                local,
            } if !self.has_namespaces() => Some(local.len()),
            _ => None,
        };
        for j in from..to {
            self.tick()?;
            let node = self.index.node(j);
            match node.kind {
                NodeKind::Element => {
                    if name_len.is_none_or(|l| l == node.name_len as usize)
                        && self.test(Item::Node(j), principal, test)?
                    {
                        out.push(Item::Node(j));
                    }
                    if wants_text && node.folded != Folded::No {
                        out.push(Item::Folded(j));
                    }
                }
                NodeKind::Text | NodeKind::CData => {
                    if wants_text && !self.continues_text(j) {
                        out.push(Item::Node(j));
                    }
                }
                NodeKind::Comment | NodeKind::ProcessingInstruction => {
                    // A name test selects elements here, never these.
                    let keep = name_len.is_none() && self.test(Item::Node(j), principal, test)?;
                    out.extend(keep.then_some(Item::Node(j)));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Whether text node `j` follows text of the same parent directly: then both are one
    /// text node, which starts at the first.
    fn continues_text(&self, j: NodeId) -> bool {
        j > 0 && {
            let prev = self.index.node(j - 1);
            is_text(prev.kind) && prev.parent == self.index.node(j).parent
        }
    }

    /// The children of `id` as XPath sees them: folded text as a text node, adjacent text
    /// and CDATA as one, the XML declaration and the DOCTYPE left out.
    fn children(&self, id: NodeId) -> Vec<Item> {
        let node = self.index.node(id);
        if node.kind == NodeKind::Element && node.folded != Folded::No {
            return vec![Item::Folded(id)];
        }
        if !matches!(node.kind, NodeKind::Element | NodeKind::Document) {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut text = false;
        for c in self.index.children(id) {
            let kind = self.index.node(c).kind;
            if !xpath_node(kind) {
                continue;
            }
            let t = is_text(kind);
            if !(t && text) {
                out.push(Item::Node(c));
            }
            text = t;
        }
        out
    }

    fn parent(&self, n: Item) -> Option<Item> {
        match n {
            Item::Node(id) if id == ROOT => None,
            Item::Node(id) => {
                let p = self.index.node(id).parent;
                Some(Item::Node(if p == xml_atlas_xml::NONE { ROOT } else { p }))
            }
            Item::Attribute(e, _) | Item::Namespace(e, _) | Item::Folded(e) => Some(Item::Node(e)),
        }
    }

    /// The ids of the ancestors of `id`, the root first.
    fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = id;
        while cur != ROOT {
            let p = self.index.node(cur).parent;
            let p = if p == xml_atlas_xml::NONE { ROOT } else { p };
            out.push(p);
            cur = p;
        }
        out.reverse();
        out
    }

    // ---- Node tests (§2.3) ----

    fn test(&mut self, c: Item, principal: Principal, test: &Test) -> R<bool> {
        let kind = c.kind(self.index);
        Ok(match test {
            Test::Node => true,
            Test::Text => kind == ItemKind::Text,
            Test::Comment => kind == ItemKind::Comment,
            Test::Pi(target) => {
                kind == ItemKind::ProcessingInstruction
                    && target
                        .as_deref()
                        .is_none_or(|t| self.name_of(c).as_ref() == t)
            }
            Test::Any { prefix } => {
                principal_kind(principal) == kind
                    && match prefix {
                        None => true,
                        Some(p) => {
                            let uri = self.prefix_uri(p)?;
                            principal != Principal::Namespace
                                && self.namespace_uri_of(c).as_deref() == Some(uri.as_str())
                        }
                    }
            }
            Test::Name { prefix, local } => {
                if principal_kind(principal) != kind {
                    return Ok(false);
                }
                if principal == Principal::Namespace {
                    // A namespace node's name is its prefix, which has no namespace.
                    return Ok(prefix.is_none() && self.name_of(c).as_ref() == local.as_str());
                }
                let name = self.raw_name(c);
                let (own_prefix, own_local) = split_name(name);
                if own_local != local.as_bytes() {
                    return Ok(false);
                }
                match (principal, prefix) {
                    // An attribute without a prefix has no namespace (§2.3, Namespaces in XML).
                    (Principal::Attribute | Principal::Namespace, None) => own_prefix.is_none(),
                    // Elements: no namespace, or the document's default one (D35).
                    (Principal::Element, None) => {
                        if !self.has_namespaces() {
                            own_prefix.is_none()
                        } else {
                            match self.namespace_uri_of(c) {
                                None => true,
                                Some(uri) => self.root_default().as_deref() == Some(uri.as_str()),
                            }
                        }
                    }
                    (_, Some(p)) => {
                        let uri = self.prefix_uri(p)?;
                        self.namespace_uri_of(c).as_deref() == Some(uri.as_str())
                    }
                }
            }
        })
    }

    // ---- Namespaces ----

    fn has_namespaces(&mut self) -> bool {
        *self
            .namespaces
            .get_or_insert_with(|| memchr::memmem::find(self.src, b"xmlns").is_some())
    }

    /// The root element's own declaration of `prefix`; `xml` is always declared.
    pub fn prefix_uri(&mut self, prefix: &str) -> R<String> {
        if prefix == "xml" {
            return Ok(XML_NAMESPACE.into());
        }
        let decl = format!("xmlns:{prefix}");
        self.index
            .root_element()
            .and_then(|r| {
                self.index
                    .attribute_pairs(self.src, r)
                    .find(|(n, _)| *n == decl.as_bytes())
                    .map(|(_, v)| unescape(v).into_owned())
            })
            .ok_or_else(|| {
                XPathError::Failed(format!(
                    "the prefix `{prefix}` is not declared on the root element"
                ))
            })
    }

    /// The default namespace the root element declares, if any.
    fn root_default(&self) -> Option<String> {
        let r = self.index.root_element()?;
        self.index
            .attribute_pairs(self.src, r)
            .find(|(n, _)| *n == b"xmlns")
            .map(|(_, v)| unescape(v).into_owned())
            .filter(|v| !v.is_empty())
    }

    /// The namespace of an element or attribute name, from the declarations in scope.
    pub fn namespace_uri_of(&mut self, c: Item) -> Option<String> {
        let (element, prefix, is_attribute) = match c {
            Item::Node(id) if self.index.node(id).kind == NodeKind::Element => {
                (id, split_name(self.index.name(self.src, id)).0, false)
            }
            Item::Attribute(e, _) => (e, split_name(self.raw_name(c)).0, true),
            _ => return None,
        };
        if is_attribute && prefix.is_none() {
            return None;
        }
        if prefix == Some(b"xml") {
            return Some(XML_NAMESPACE.into());
        }
        if !self.has_namespaces() {
            return None;
        }
        let mut cur = element;
        while cur != ROOT && cur != xml_atlas_xml::NONE {
            for (name, value) in self.index.attribute_pairs(self.src, cur) {
                let declares = match prefix {
                    None => name == b"xmlns",
                    Some(p) => name.strip_prefix(b"xmlns:") == Some(p),
                };
                if declares {
                    let uri = unescape(value);
                    return (!uri.is_empty()).then(|| uri.into_owned());
                }
            }
            cur = self.index.node(cur).parent;
        }
        None
    }

    /// The namespaces in scope on element `e`, as (prefix, URI); the default one has an empty
    /// prefix. The nearest declaration of a prefix wins; `xml` is always there.
    pub fn in_scope(&self, e: NodeId) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut undeclared: Vec<String> = Vec::new();
        let mut cur = e;
        while cur != ROOT && cur != xml_atlas_xml::NONE {
            for (name, value) in self.index.attribute_pairs(self.src, cur) {
                let prefix = if name == b"xmlns" {
                    String::new()
                } else if let Some(p) = name.strip_prefix(b"xmlns:") {
                    String::from_utf8_lossy(p).into_owned()
                } else {
                    continue;
                };
                if out.iter().any(|(p, _)| *p == prefix) || undeclared.contains(&prefix) {
                    continue;
                }
                let uri = unescape(value).into_owned();
                if uri.is_empty() {
                    undeclared.push(prefix);
                } else {
                    out.push((prefix, uri));
                }
            }
            cur = self.index.node(cur).parent;
        }
        out.push(("xml".into(), XML_NAMESPACE.into()));
        out
    }

    // ---- Names and values (§5) ----

    /// The name as written: an element's or attribute's QName, a processing instruction's
    /// target, a namespace node's prefix. Empty for other nodes.
    pub fn raw_name(&self, c: Item) -> &'d [u8] {
        match c {
            Item::Node(id) => {
                let node = self.index.node(id);
                match node.kind {
                    NodeKind::Element => self.index.name(self.src, id),
                    NodeKind::ProcessingInstruction => pi_parts(self.index.raw(self.src, id)).0,
                    _ => b"",
                }
            }
            Item::Attribute(e, k) => self
                .index
                .attribute_pairs(self.src, e)
                .nth(k as usize)
                .map_or(&b""[..], |(n, _)| n),
            Item::Namespace(..) | Item::Folded(_) => b"",
        }
    }

    /// [`Eval::raw_name`] as text; a namespace node's prefix too.
    pub fn name_of(&self, c: Item) -> Cow<'d, str> {
        match c {
            Item::Namespace(e, k) => self
                .in_scope(e)
                .into_iter()
                .nth(k as usize)
                .map_or(Cow::Borrowed(""), |(p, _)| Cow::Owned(p)),
            _ => String::from_utf8_lossy(self.raw_name(c)),
        }
    }

    /// The string-value of a node (§5): all the text below a root or element, an attribute's
    /// value, a text node's text, a comment's or processing instruction's content, a
    /// namespace node's URI.
    pub fn string_value(&mut self, c: Item) -> R<Cow<'d, str>> {
        self.string_value_upto(c, usize::MAX)
    }

    /// [`Eval::string_value`], cut after about `max` bytes: for showing a value.
    pub fn string_value_upto(&mut self, c: Item, max: usize) -> R<Cow<'d, str>> {
        let index = self.index;
        let src = self.src;
        Ok(match c {
            Item::Folded(e) => index.text(src, e),
            Item::Attribute(e, k) => index
                .attribute_pairs(src, e)
                .nth(k as usize)
                .map_or(Cow::Borrowed(""), |(_, v)| unescape(v)),
            Item::Namespace(e, k) => Cow::Owned(
                self.in_scope(e)
                    .into_iter()
                    .nth(k as usize)
                    .map(|(_, u)| u)
                    .unwrap_or_default(),
            ),
            Item::Node(id) => {
                let node = index.node(id);
                match node.kind {
                    NodeKind::Text | NodeKind::CData => {
                        let mut out = index.text(src, id);
                        let mut next = index.next_sibling(id);
                        while let Some(s) = next.filter(|&s| is_text(index.node(s).kind)) {
                            out.to_mut().push_str(&index.text(src, s));
                            next = index.next_sibling(s);
                        }
                        out
                    }
                    NodeKind::Comment => {
                        let raw = index.raw(src, id);
                        let inner = raw
                            .strip_prefix(b"<!--")
                            .and_then(|r| r.strip_suffix(b"-->"))
                            .unwrap_or(raw);
                        String::from_utf8_lossy(inner)
                    }
                    NodeKind::ProcessingInstruction => {
                        String::from_utf8_lossy(pi_parts(index.raw(src, id)).1)
                    }
                    NodeKind::Element if node.folded != Folded::No => index.text(src, id),
                    NodeKind::Element | NodeKind::Document => {
                        let end = id + 1 + node.descendants;
                        let mut out = Cow::Borrowed("");
                        for j in id + 1..end {
                            self.tick()?;
                            let n = index.node(j);
                            let t = match n.kind {
                                NodeKind::Text | NodeKind::CData => index.text(src, j),
                                NodeKind::Element if n.folded != Folded::No => index.text(src, j),
                                _ => continue,
                            };
                            if out.is_empty() {
                                out = t;
                            } else {
                                out.to_mut().push_str(&t);
                            }
                            if out.len() >= max {
                                break;
                            }
                        }
                        out
                    }
                    _ => Cow::Borrowed(""),
                }
            }
        })
    }

    // ---- Conversions (§4.2 to §4.4) ----

    pub fn string(&mut self, v: &Value) -> R<String> {
        Ok(match v {
            Value::Nodes(n) => match n.first() {
                Some(&first) => self.string_value(first)?.into_owned(),
                None => String::new(),
            },
            Value::String(s) => s.clone(),
            Value::Number(x) => number_to_string(*x),
            Value::Boolean(b) => b.to_string(),
        })
    }

    fn number(&mut self, v: &Value) -> f64 {
        match v {
            Value::Number(x) => *x,
            Value::Boolean(b) => f64::from(u8::from(*b)),
            Value::String(s) => string_to_number(s),
            // Stopping shows up in the next walk; the value then does not matter.
            Value::Nodes(_) => self.string(v).map_or(f64::NAN, |s| string_to_number(&s)),
        }
    }

    fn boolean(&self, v: &Value) -> bool {
        match v {
            Value::Nodes(n) => !n.is_empty(),
            Value::String(s) => !s.is_empty(),
            Value::Number(x) => *x != 0.0 && !x.is_nan(),
            Value::Boolean(b) => *b,
        }
    }

    // ---- Comparisons (§3.4) ----

    fn compare(&mut self, op: Cmp, a: &Value, b: &Value) -> R<bool> {
        match (a, b) {
            (Value::Nodes(x), Value::Nodes(y)) => {
                // The right side's values once, then each of the left against them.
                let mut ys = Vec::with_capacity(y.len());
                for &n in y {
                    ys.push(self.string_value(n)?.into_owned());
                }
                for &n in x {
                    let s = self.string_value(n)?;
                    if ys
                        .iter()
                        .any(|t| compare_atoms(op, &Atom::Str(&s), &Atom::Str(t)))
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            (Value::Nodes(x), other) => self.compare_nodes(op, x, other),
            (other, Value::Nodes(y)) => self.compare_nodes(op.flipped(), y, other),
            _ => {
                let (a, b) = (self.atom(a), self.atom(b));
                Ok(compare_atoms(op, &a.as_atom(), &b.as_atom()))
            }
        }
    }

    /// A node-set against a number, string or boolean: true when some node's value holds.
    fn compare_nodes(&mut self, op: Cmp, nodes: &[Item], other: &Value) -> R<bool> {
        match other {
            Value::Boolean(b) => Ok(compare_atoms(
                op,
                &Atom::Bool(!nodes.is_empty()),
                &Atom::Bool(*b),
            )),
            Value::Number(x) => {
                for &n in nodes {
                    let s = self.string_value(n)?;
                    if compare_atoms(op, &Atom::Num(string_to_number(&s)), &Atom::Num(*x)) {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Value::String(t) => {
                for &n in nodes {
                    let s = self.string_value(n)?;
                    if compare_atoms(op, &Atom::Str(&s), &Atom::Str(t)) {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Value::Nodes(_) => unreachable!("handled by compare"),
        }
    }

    fn atom(&self, v: &Value) -> OwnedAtom {
        match v {
            Value::Boolean(b) => OwnedAtom::Bool(*b),
            Value::Number(x) => OwnedAtom::Num(*x),
            Value::String(s) => OwnedAtom::Str(s.clone()),
            Value::Nodes(_) => unreachable!("handled by compare"),
        }
    }

    // ---- Functions (§4) ----

    fn call(&mut self, f: Func, args: &[Expr], ctx: Ctx) -> R<Value> {
        // The argument, or the context node as a node-set when there is none.
        let arg_nodes = |me: &mut Self, what: &str| -> R<Vec<Item>> {
            match args.first() {
                Some(a) => me.nodes(a, ctx, what),
                None => Ok(vec![ctx.item]),
            }
        };
        let string_arg = |me: &mut Self, i: usize| -> R<String> {
            let v = me.eval(&args[i], ctx)?;
            me.string(&v)
        };
        Ok(match f {
            Func::Last => Value::Number(ctx.size as f64),
            Func::Position => Value::Number(ctx.pos as f64),
            Func::Count => Value::Number(self.nodes(&args[0], ctx, "`count()`")?.len() as f64),
            Func::Id => {
                let v = self.eval(&args[0], ctx)?;
                let mut ids = Vec::new();
                match &v {
                    Value::Nodes(n) => {
                        for &i in n {
                            ids.extend(
                                self.string_value(i)?
                                    .split_ascii_whitespace()
                                    .map(str::to_string),
                            );
                        }
                    }
                    other => ids.extend(
                        self.string(other)?
                            .split_ascii_whitespace()
                            .map(str::to_string),
                    ),
                }
                Value::Nodes(self.elements_with_id(&ids)?)
            }
            Func::LocalName | Func::Name | Func::NamespaceUri => {
                let nodes = arg_nodes(self, "the function")?;
                let Some(&n) = nodes.first() else {
                    return Ok(Value::String(String::new()));
                };
                Value::String(match f {
                    Func::Name => self.name_of(n).into_owned(),
                    Func::LocalName => match n {
                        Item::Namespace(..) => self.name_of(n).into_owned(),
                        _ => String::from_utf8_lossy(split_name(self.raw_name(n)).1).into_owned(),
                    },
                    _ => self.namespace_uri_of(n).unwrap_or_default(),
                })
            }
            Func::String => match args.first() {
                Some(a) => {
                    let v = self.eval(a, ctx)?;
                    Value::String(self.string(&v)?)
                }
                None => Value::String(self.string_value(ctx.item)?.into_owned()),
            },
            Func::Concat => {
                let mut out = String::new();
                for i in 0..args.len() {
                    out.push_str(&string_arg(self, i)?);
                }
                Value::String(out)
            }
            Func::StartsWith => {
                let (a, b) = (string_arg(self, 0)?, string_arg(self, 1)?);
                Value::Boolean(a.starts_with(&b))
            }
            Func::Contains => {
                let (a, b) = (string_arg(self, 0)?, string_arg(self, 1)?);
                Value::Boolean(a.contains(&b))
            }
            Func::SubstringBefore => {
                let (a, b) = (string_arg(self, 0)?, string_arg(self, 1)?);
                Value::String(a.find(&b).map_or(String::new(), |i| a[..i].to_string()))
            }
            Func::SubstringAfter => {
                let (a, b) = (string_arg(self, 0)?, string_arg(self, 1)?);
                Value::String(
                    a.find(&b)
                        .map_or(String::new(), |i| a[i + b.len()..].to_string()),
                )
            }
            Func::Substring => {
                let s = string_arg(self, 0)?;
                let v = self.eval(&args[1], ctx)?;
                let start = round(self.number(&v));
                let len = match args.get(2) {
                    Some(a) => {
                        let v = self.eval(a, ctx)?;
                        Some(round(self.number(&v)))
                    }
                    None => None,
                };
                Value::String(substring(&s, start, len))
            }
            Func::StringLength => {
                let s = match args.first() {
                    Some(_) => string_arg(self, 0)?,
                    None => self.string_value(ctx.item)?.into_owned(),
                };
                Value::Number(s.chars().count() as f64)
            }
            Func::NormalizeSpace => {
                let s = match args.first() {
                    Some(_) => string_arg(self, 0)?,
                    None => self.string_value(ctx.item)?.into_owned(),
                };
                Value::String(
                    s.split([' ', '\t', '\r', '\n'])
                        .filter(|w| !w.is_empty())
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            }
            Func::Translate => {
                let s = string_arg(self, 0)?;
                let from: Vec<char> = string_arg(self, 1)?.chars().collect();
                let to: Vec<char> = string_arg(self, 2)?.chars().collect();
                Value::String(
                    s.chars()
                        .filter_map(|c| match from.iter().position(|&f| f == c) {
                            Some(i) => to.get(i).copied(),
                            None => Some(c),
                        })
                        .collect(),
                )
            }
            Func::Boolean => {
                let v = self.eval(&args[0], ctx)?;
                Value::Boolean(self.boolean(&v))
            }
            Func::Not => {
                let v = self.eval(&args[0], ctx)?;
                Value::Boolean(!self.boolean(&v))
            }
            Func::True => Value::Boolean(true),
            Func::False => Value::Boolean(false),
            Func::Lang => {
                let wanted = string_arg(self, 0)?.to_lowercase();
                Value::Boolean(self.lang(ctx.item).is_some_and(|l| {
                    let l = l.to_lowercase();
                    l == wanted
                        || l.strip_prefix(wanted.as_str())
                            .is_some_and(|rest| rest.starts_with('-'))
                }))
            }
            Func::Number => match args.first() {
                Some(a) => {
                    let v = self.eval(a, ctx)?;
                    Value::Number(self.number(&v))
                }
                None => {
                    let s = self.string_value(ctx.item)?;
                    Value::Number(string_to_number(&s))
                }
            },
            Func::Sum => {
                let mut total = 0.0;
                for n in self.nodes(&args[0], ctx, "`sum()`")? {
                    let s = self.string_value(n)?;
                    total += string_to_number(&s);
                }
                Value::Number(total)
            }
            Func::Floor | Func::Ceiling | Func::Round => {
                let v = self.eval(&args[0], ctx)?;
                let x = self.number(&v);
                Value::Number(match f {
                    Func::Floor => x.floor(),
                    Func::Ceiling => x.ceil(),
                    _ => round(x),
                })
            }
        })
    }

    /// The `xml:lang` in scope on a node (§4.3, `lang()`).
    fn lang(&self, n: Item) -> Option<String> {
        let mut cur = match n {
            Item::Node(id) => Some(Item::Node(id)),
            Item::Attribute(e, _) | Item::Namespace(e, _) | Item::Folded(e) => Some(Item::Node(e)),
        };
        while let Some(Item::Node(id)) = cur {
            if self.index.node(id).kind == NodeKind::Element
                && let Some((_, v)) = self
                    .index
                    .attribute_pairs(self.src, id)
                    .find(|(n, _)| *n == b"xml:lang")
            {
                return Some(unescape(v).into_owned());
            }
            cur = self.parent(Item::Node(id));
        }
        None
    }

    /// Elements whose `xml:id` is one of `ids`. Without a DTD nothing else is known to be an
    /// ID (§5.2.1), so `id()` uses `xml:id` (W3C xml:id).
    fn elements_with_id(&mut self, ids: &[String]) -> R<Vec<Item>> {
        let mut out = Vec::new();
        if ids.is_empty() {
            return Ok(out);
        }
        for j in 0..self.index.len() as NodeId {
            self.tick()?;
            if self.index.node(j).kind != NodeKind::Element {
                continue;
            }
            let found = self
                .index
                .attribute_pairs(self.src, j)
                .any(|(n, v)| n == b"xml:id" && ids.iter().any(|i| *i == unescape(v)));
            if found {
                out.push(Item::Node(j));
            }
        }
        Ok(out)
    }
}

/// A value to compare, borrowed.
enum Atom<'a> {
    Str(&'a str),
    Num(f64),
    Bool(bool),
}

enum OwnedAtom {
    Str(String),
    Num(f64),
    Bool(bool),
}

impl OwnedAtom {
    fn as_atom(&self) -> Atom<'_> {
        match self {
            OwnedAtom::Str(s) => Atom::Str(s),
            OwnedAtom::Num(x) => Atom::Num(*x),
            OwnedAtom::Bool(b) => Atom::Bool(*b),
        }
    }
}

impl Cmp {
    /// The same comparison with its sides swapped.
    fn flipped(self) -> Self {
        match self {
            Cmp::Lt => Cmp::Gt,
            Cmp::Le => Cmp::Ge,
            Cmp::Gt => Cmp::Lt,
            Cmp::Ge => Cmp::Le,
            other => other,
        }
    }
}

/// Two values that are not node-sets (§3.4): `=` and `!=` compare as booleans if either is
/// one, else as numbers if either is one, else as strings; the others compare numbers.
fn compare_atoms(op: Cmp, a: &Atom, b: &Atom) -> bool {
    let num = |x: &Atom| match x {
        Atom::Num(n) => *n,
        Atom::Bool(b) => f64::from(u8::from(*b)),
        Atom::Str(s) => string_to_number(s),
    };
    let boolean = |x: &Atom| match x {
        Atom::Num(n) => *n != 0.0 && !n.is_nan(),
        Atom::Bool(b) => *b,
        Atom::Str(s) => !s.is_empty(),
    };
    match op {
        Cmp::Eq | Cmp::Ne => {
            let equal = match (a, b) {
                (Atom::Bool(_), _) | (_, Atom::Bool(_)) => boolean(a) == boolean(b),
                (Atom::Num(_), _) | (_, Atom::Num(_)) => num(a) == num(b),
                (Atom::Str(x), Atom::Str(y)) => x == y,
            };
            equal == (op == Cmp::Eq)
        }
        Cmp::Lt => num(a) < num(b),
        Cmp::Le => num(a) <= num(b),
        Cmp::Gt => num(a) > num(b),
        Cmp::Ge => num(a) >= num(b),
    }
}

/// Merges two node-sets in document order, without doubles.
fn union(a: Vec<Item>, b: Vec<Item>) -> Vec<Item> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].order().cmp(&b[j].order()) {
            std::cmp::Ordering::Less => {
                out.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

/// A string as a number (§4.4): optional whitespace, an optional minus, digits with an
/// optional point; anything else is NaN.
pub(super) fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'));
    let digits = t.strip_prefix('-').unwrap_or(t);
    let mut parts = digits.splitn(2, '.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    let ok = whole.bytes().all(|b| b.is_ascii_digit())
        && fraction.is_none_or(|f| f.bytes().all(|b| b.is_ascii_digit()))
        && (!whole.is_empty() || fraction.is_some_and(|f| !f.is_empty()));
    if !ok {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

/// A number as a string (§4.2): `NaN`, `Infinity`, an integer without a point, otherwise
/// the decimal digits without an exponent.
pub(super) fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else if x == 0.0 {
        "0".into()
    } else {
        // Rust writes the shortest digits that read back the same, never with an exponent.
        format!("{x}")
    }
}

/// `round()` (§4.4): the nearest integer, halves up; -0.5 to 0 gives negative zero.
fn round(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        x
    } else if (-0.5..0.0).contains(&x) {
        -0.0
    } else {
        (x + 0.5).floor()
    }
}

/// `substring()` (§4.2) with its arguments already rounded: the characters at positions
/// from `start` (counted from 1), `len` of them, compared as numbers so NaN and infinities
/// select as the recommendation says.
fn substring(s: &str, start: f64, len: Option<f64>) -> String {
    s.chars()
        .enumerate()
        .filter(|&(i, _)| {
            let p = (i + 1) as f64;
            p >= start && len.is_none_or(|l| p < start + l)
        })
        .map(|(_, c)| c)
        .collect()
}

fn split_name(name: &[u8]) -> (Option<&[u8]>, &[u8]) {
    match memchr::memchr(b':', name) {
        Some(i) => (Some(&name[..i]), &name[i + 1..]),
        None => (None, name),
    }
}

/// `xmlns` or `xmlns:p`: a declaration, which XPath shows as a namespace node, not an
/// attribute.
fn is_namespace_declaration(name: &[u8]) -> bool {
    name == b"xmlns" || name.starts_with(b"xmlns:")
}

fn is_text(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Text | NodeKind::CData)
}

/// Whether XPath sees a node of this kind: not the XML declaration or the DOCTYPE.
fn xpath_node(kind: NodeKind) -> bool {
    !matches!(kind, NodeKind::Declaration | NodeKind::DocType)
}

fn principal_kind(p: Principal) -> ItemKind {
    match p {
        Principal::Element => ItemKind::Element,
        Principal::Attribute => ItemKind::Attribute,
        Principal::Namespace => ItemKind::Namespace,
    }
}

/// A processing instruction's target and content: `<?target content?>`.
fn pi_parts(raw: &[u8]) -> (&[u8], &[u8]) {
    let inner = raw
        .strip_prefix(b"<?")
        .map(|r| r.strip_suffix(b"?>").unwrap_or(r))
        .unwrap_or(raw);
    let end = inner
        .iter()
        .position(|b| b.is_ascii_whitespace())
        .unwrap_or(inner.len());
    let rest = &inner[end..];
    let start = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(rest.len());
    (&inner[..end], &rest[start..])
}
