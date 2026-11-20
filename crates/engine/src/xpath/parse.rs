//! Reading an XPath 1.0 expression (W3C XPath 1.0, §2 and §3): the tokens of §3.7, then the
//! grammar, into an [`Expr`]. Names are resolved later, against the document.

use super::XPathError;

/// Expressions nested deeper than this are refused: the parser recurses once per level.
const MAX_DEPTH: usize = 200;

/// A parsed expression.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Compare(Cmp, Box<Expr>, Box<Expr>),
    Arith(Arith, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Union(Box<Expr>, Box<Expr>),
    Path(Path),
    /// A primary expression with predicates.
    Filter(Box<Expr>, Vec<Expr>),
    Literal(String),
    Number(f64),
    Call(Func, Vec<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Arith {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Path {
    pub start: Start,
    pub steps: Vec<Step>,
}

/// Where a path starts.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Start {
    /// `/...`: the root node.
    Root,
    /// A relative path: the context node.
    Context,
    /// `(...)/...` or `f()/...`: the node-set an expression gives.
    Filter(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Step {
    pub axis: Axis,
    pub test: Test,
    pub predicates: Vec<Expr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Axis {
    Ancestor,
    AncestorOrSelf,
    Attribute,
    Child,
    Descendant,
    DescendantOrSelf,
    Following,
    FollowingSibling,
    Namespace,
    Parent,
    Preceding,
    PrecedingSibling,
    /// `self::`, a keyword in Rust.
    Itself,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Test {
    /// `name` or `prefix:name`.
    Name {
        prefix: Option<String>,
        local: String,
    },
    /// `*` or `prefix:*`.
    Any {
        prefix: Option<String>,
    },
    Node,
    Text,
    Comment,
    /// `processing-instruction()`, or with a target.
    Pi(Option<String>),
}

/// The functions of XPath 1.0 (§4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Func {
    Last,
    Position,
    Count,
    Id,
    LocalName,
    NamespaceUri,
    Name,
    String,
    Concat,
    StartsWith,
    Contains,
    SubstringBefore,
    SubstringAfter,
    Substring,
    StringLength,
    NormalizeSpace,
    Translate,
    Boolean,
    Not,
    True,
    False,
    Lang,
    Number,
    Sum,
    Floor,
    Ceiling,
    Round,
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    LParen,
    RParen,
    LBracket,
    RBracket,
    Dot,
    DotDot,
    At,
    Comma,
    ColonColon,
    Slash,
    SlashSlash,
    Pipe,
    Plus,
    Minus,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// The multiplication operator; `*` as a name test is a [`Tok::Name`].
    Star,
    And,
    Or,
    Div,
    Mod,
    /// A name test: `name`, `p:name`, `*` (`local` is `None`) or `p:*`.
    Name {
        prefix: Option<String>,
        local: Option<String>,
    },
    NodeType(String),
    Function(String),
    Axis(String),
    Literal(String),
    Number(f64),
    Variable(String),
}

struct Token {
    tok: Tok,
    /// Character position, from 0.
    at: usize,
}

struct Parser {
    tokens: Vec<Token>,
    next: usize,
    /// Characters in the expression: where the end is, for messages.
    len: usize,
    depth: usize,
}

impl Axis {
    /// The reverse axes count positions from the context node backwards (§2.4).
    pub fn is_reverse(self) -> bool {
        matches!(
            self,
            Axis::Ancestor | Axis::AncestorOrSelf | Axis::Preceding | Axis::PrecedingSibling
        )
    }

    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "ancestor" => Axis::Ancestor,
            "ancestor-or-self" => Axis::AncestorOrSelf,
            "attribute" => Axis::Attribute,
            "child" => Axis::Child,
            "descendant" => Axis::Descendant,
            "descendant-or-self" => Axis::DescendantOrSelf,
            "following" => Axis::Following,
            "following-sibling" => Axis::FollowingSibling,
            "namespace" => Axis::Namespace,
            "parent" => Axis::Parent,
            "preceding" => Axis::Preceding,
            "preceding-sibling" => Axis::PrecedingSibling,
            "self" => Axis::Itself,
            _ => return None,
        })
    }
}

impl Func {
    /// The function and how many arguments it takes, at least and at most.
    fn named(name: &str) -> Option<(Self, usize, usize)> {
        use Func::*;
        Some(match name {
            "last" => (Last, 0, 0),
            "position" => (Position, 0, 0),
            "count" => (Count, 1, 1),
            "id" => (Id, 1, 1),
            "local-name" => (LocalName, 0, 1),
            "namespace-uri" => (NamespaceUri, 0, 1),
            "name" => (Name, 0, 1),
            "string" => (String, 0, 1),
            "concat" => (Concat, 2, usize::MAX),
            "starts-with" => (StartsWith, 2, 2),
            "contains" => (Contains, 2, 2),
            "substring-before" => (SubstringBefore, 2, 2),
            "substring-after" => (SubstringAfter, 2, 2),
            "substring" => (Substring, 2, 3),
            "string-length" => (StringLength, 0, 1),
            "normalize-space" => (NormalizeSpace, 0, 1),
            "translate" => (Translate, 3, 3),
            "boolean" => (Boolean, 1, 1),
            "not" => (Not, 1, 1),
            "true" => (True, 0, 0),
            "false" => (False, 0, 0),
            "lang" => (Lang, 1, 1),
            "number" => (Number, 0, 1),
            "sum" => (Sum, 1, 1),
            "floor" => (Floor, 1, 1),
            "ceiling" => (Ceiling, 1, 1),
            "round" => (Round, 1, 1),
            _ => return None,
        })
    }

    /// Whether the function gives a number.
    fn gives_number(self) -> bool {
        use Func::*;
        matches!(
            self,
            Last | Position | Count | StringLength | Number | Sum | Floor | Ceiling | Round
        )
    }
}

/// Reads `text` as an XPath 1.0 expression.
pub(super) fn parse(text: &str) -> Result<Expr, XPathError> {
    let tokens = tokenize(text)?;
    let mut p = Parser {
        tokens,
        next: 0,
        len: text.chars().count(),
        depth: 0,
    };
    if p.tokens.is_empty() {
        return Err(syntax("the expression is empty", 0));
    }
    let e = p.expr()?;
    if let Some(t) = p.tokens.get(p.next) {
        return Err(syntax(format!("unexpected {}", describe(&t.tok)), t.at));
    }
    Ok(e)
}

/// The prefixes of the name tests in `e`, each once, in the order they first appear.
pub(super) fn prefixes(e: &Expr) -> Vec<String> {
    fn walk(e: &Expr, out: &mut Vec<String>) {
        match e {
            Expr::Path(p) => {
                if let Start::Filter(f) = &p.start {
                    walk(f, out);
                }
                for s in &p.steps {
                    if let Test::Name {
                        prefix: Some(prefix),
                        ..
                    }
                    | Test::Any {
                        prefix: Some(prefix),
                    } = &s.test
                        && !out.contains(prefix)
                    {
                        out.push(prefix.clone());
                    }
                    for pred in &s.predicates {
                        walk(pred, out);
                    }
                }
            }
            Expr::Or(a, b)
            | Expr::And(a, b)
            | Expr::Compare(_, a, b)
            | Expr::Arith(_, a, b)
            | Expr::Union(a, b) => {
                walk(a, out);
                walk(b, out);
            }
            Expr::Neg(a) => walk(a, out),
            Expr::Filter(a, preds) => {
                walk(a, out);
                for p in preds {
                    walk(p, out);
                }
            }
            Expr::Call(_, args) => {
                for a in args {
                    walk(a, out);
                }
            }
            Expr::Literal(_) | Expr::Number(_) => {}
        }
    }
    let mut out = Vec::new();
    walk(e, &mut out);
    out
}

/// Whether a predicate could select by position: it may give a number, or it calls
/// `position()` or `last()`. Such a predicate keeps `//x[...]` from becoming one scan of the
/// descendants, as positions count per parent.
pub(super) fn positional(e: &Expr) -> bool {
    may_be_number(e) || calls_position(e)
}

fn may_be_number(e: &Expr) -> bool {
    match e {
        Expr::Number(_) | Expr::Arith(..) | Expr::Neg(_) => true,
        Expr::Call(f, _) => f.gives_number(),
        Expr::Filter(inner, _) => may_be_number(inner),
        _ => false,
    }
}

fn calls_position(e: &Expr) -> bool {
    match e {
        Expr::Call(Func::Position | Func::Last, _) => true,
        Expr::Call(_, args) => args.iter().any(calls_position),
        Expr::Or(a, b)
        | Expr::And(a, b)
        | Expr::Compare(_, a, b)
        | Expr::Arith(_, a, b)
        | Expr::Union(a, b) => calls_position(a) || calls_position(b),
        Expr::Neg(a) => calls_position(a),
        Expr::Filter(a, preds) => calls_position(a) || preds.iter().any(calls_position),
        Expr::Path(p) => {
            matches!(&p.start, Start::Filter(f) if calls_position(f))
                || p.steps
                    .iter()
                    .any(|s| s.predicates.iter().any(calls_position))
        }
        Expr::Literal(_) | Expr::Number(_) => false,
    }
}

fn syntax(message: impl Into<String>, at: usize) -> XPathError {
    XPathError::Syntax {
        message: message.into(),
        at: at + 1,
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Name {
            prefix,
            local: Some(l),
        } => match prefix {
            Some(p) => format!("`{p}:{l}`"),
            None => format!("`{l}`"),
        },
        Tok::Name {
            prefix,
            local: None,
        } => match prefix {
            Some(p) => format!("`{p}:*`"),
            None => "`*`".into(),
        },
        Tok::NodeType(n) | Tok::Function(n) => format!("`{n}(`"),
        Tok::Axis(n) => format!("`{n}::`"),
        Tok::Literal(s) => format!("\"{s}\""),
        Tok::Number(_) => "a number".into(),
        Tok::Variable(v) => format!("`${v}`"),
        other => format!("`{}`", symbol(other)),
    }
}

fn symbol(t: &Tok) -> &'static str {
    match t {
        Tok::LParen => "(",
        Tok::RParen => ")",
        Tok::LBracket => "[",
        Tok::RBracket => "]",
        Tok::Dot => ".",
        Tok::DotDot => "..",
        Tok::At => "@",
        Tok::Comma => ",",
        Tok::ColonColon => "::",
        Tok::Slash => "/",
        Tok::SlashSlash => "//",
        Tok::Pipe => "|",
        Tok::Plus => "+",
        Tok::Minus => "-",
        Tok::Eq => "=",
        Tok::Ne => "!=",
        Tok::Lt => "<",
        Tok::Le => "<=",
        Tok::Gt => ">",
        Tok::Ge => ">=",
        Tok::Star => "*",
        Tok::And => "and",
        Tok::Or => "or",
        Tok::Div => "div",
        Tok::Mod => "mod",
        _ => "?",
    }
}

fn is_name_start(c: char) -> bool {
    c == '_' || c.is_alphabetic() || (!c.is_ascii() && !c.is_whitespace() && !c.is_control())
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-' || c == '.' || c.is_numeric()
}

/// Whether the token before allows a name or `*` to be a name test (§3.7): at the start, or
/// after `@`, `::`, `(`, `[`, `,` or an operator.
fn operand_expected(prev: Option<&Tok>) -> bool {
    match prev {
        None => true,
        Some(t) => matches!(
            t,
            Tok::At
                | Tok::ColonColon
                | Tok::LParen
                | Tok::LBracket
                | Tok::Comma
                | Tok::Slash
                | Tok::SlashSlash
                | Tok::Pipe
                | Tok::Plus
                | Tok::Minus
                | Tok::Eq
                | Tok::Ne
                | Tok::Lt
                | Tok::Le
                | Tok::Gt
                | Tok::Ge
                | Tok::Star
                | Tok::And
                | Tok::Or
                | Tok::Div
                | Tok::Mod
        ),
    }
}

fn tokenize(text: &str) -> Result<Vec<Token>, XPathError> {
    let cs: Vec<char> = text.chars().collect();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0;
    let skip_ws = |mut j: usize| {
        while j < cs.len() && matches!(cs[j], ' ' | '\t' | '\r' | '\n') {
            j += 1;
        }
        j
    };
    while i < cs.len() {
        i = skip_ws(i);
        if i >= cs.len() {
            break;
        }
        let at = i;
        let c = cs[i];
        let next = cs.get(i + 1).copied();
        let operand = operand_expected(out.last().map(|t| &t.tok));
        let simple = |tok, n| (tok, n);
        let (tok, n) = match c {
            '(' => simple(Tok::LParen, 1),
            ')' => simple(Tok::RParen, 1),
            '[' => simple(Tok::LBracket, 1),
            ']' => simple(Tok::RBracket, 1),
            '@' => simple(Tok::At, 1),
            ',' => simple(Tok::Comma, 1),
            '|' => simple(Tok::Pipe, 1),
            '+' => simple(Tok::Plus, 1),
            '-' => simple(Tok::Minus, 1),
            '=' => simple(Tok::Eq, 1),
            '!' if next == Some('=') => simple(Tok::Ne, 2),
            '<' if next == Some('=') => simple(Tok::Le, 2),
            '<' => simple(Tok::Lt, 1),
            '>' if next == Some('=') => simple(Tok::Ge, 2),
            '>' => simple(Tok::Gt, 1),
            '/' if next == Some('/') => simple(Tok::SlashSlash, 2),
            '/' => simple(Tok::Slash, 1),
            ':' if next == Some(':') => simple(Tok::ColonColon, 2),
            '*' if operand => simple(
                Tok::Name {
                    prefix: None,
                    local: None,
                },
                1,
            ),
            '*' => simple(Tok::Star, 1),
            '.' if next == Some('.') => simple(Tok::DotDot, 2),
            '.' if !next.is_some_and(|n| n.is_ascii_digit()) => simple(Tok::Dot, 1),
            '"' | '\'' => {
                let end = cs[i + 1..]
                    .iter()
                    .position(|&q| q == c)
                    .ok_or_else(|| syntax("the string is not closed", at))?;
                let s: String = cs[i + 1..i + 1 + end].iter().collect();
                (Tok::Literal(s), end + 2)
            }
            '0'..='9' | '.' => {
                let mut j = i;
                while j < cs.len() && cs[j].is_ascii_digit() {
                    j += 1;
                }
                if j < cs.len() && cs[j] == '.' {
                    j += 1;
                    while j < cs.len() && cs[j].is_ascii_digit() {
                        j += 1;
                    }
                }
                let s: String = cs[i..j].iter().collect();
                let v = s
                    .parse::<f64>()
                    .map_err(|_| syntax(format!("`{s}` is not a number"), at))?;
                (Tok::Number(v), j - i)
            }
            '$' => {
                let (name, j) =
                    qname(&cs, i + 1).ok_or_else(|| syntax("expected a name after `$`", at))?;
                (Tok::Variable(name), j - i)
            }
            c if is_name_start(c) => {
                let mut j = i;
                while j < cs.len() && is_name_char(cs[j]) {
                    j += 1;
                }
                let first: String = cs[i..j].iter().collect();
                if !operand {
                    let tok = match first.as_str() {
                        "and" => Tok::And,
                        "or" => Tok::Or,
                        "div" => Tok::Div,
                        "mod" => Tok::Mod,
                        _ => {
                            return Err(syntax(format!("expected an operator, not `{first}`"), at));
                        }
                    };
                    (tok, j - i)
                } else {
                    // A prefix: `p:name` or `p:*`, but not `axis::`.
                    let (prefix, local, j) = if j + 1 < cs.len() && cs[j] == ':' && cs[j + 1] != ':'
                    {
                        if cs[j + 1] == '*' {
                            (Some(first), None, j + 2)
                        } else if is_name_start(cs[j + 1]) {
                            let mut k = j + 1;
                            while k < cs.len() && is_name_char(cs[k]) {
                                k += 1;
                            }
                            (
                                Some(first),
                                Some(cs[j + 1..k].iter().collect::<String>()),
                                k,
                            )
                        } else {
                            return Err(syntax("expected a name after `:`", j));
                        }
                    } else {
                        (None, Some(first), j)
                    };
                    let after = skip_ws(j);
                    let full = || match (&prefix, &local) {
                        (Some(p), Some(l)) => format!("{p}:{l}"),
                        (None, Some(l)) => l.clone(),
                        (Some(p), None) => format!("{p}:*"),
                        (None, None) => "*".into(),
                    };
                    let tok = if cs.get(after) == Some(&'(') && local.is_some() {
                        match (prefix.as_deref(), local.as_deref()) {
                            (
                                None,
                                Some(n @ ("comment" | "text" | "processing-instruction" | "node")),
                            ) => Tok::NodeType(n.into()),
                            _ => Tok::Function(full()),
                        }
                    } else if prefix.is_none()
                        && cs.get(after) == Some(&':')
                        && cs.get(after + 1) == Some(&':')
                    {
                        Tok::Axis(full())
                    } else {
                        Tok::Name { prefix, local }
                    };
                    (tok, j - i)
                }
            }
            other => return Err(syntax(format!("unexpected `{other}`"), at)),
        };
        out.push(Token { tok, at });
        i += n;
    }
    Ok(out)
}

/// A QName at `i`: the name and where it ends.
fn qname(cs: &[char], i: usize) -> Option<(String, usize)> {
    if !cs.get(i).is_some_and(|&c| is_name_start(c)) {
        return None;
    }
    let mut j = i;
    while j < cs.len() && is_name_char(cs[j]) {
        j += 1;
    }
    if j + 1 < cs.len() && cs[j] == ':' && is_name_start(cs[j + 1]) {
        j += 1;
        while j < cs.len() && is_name_char(cs[j]) {
            j += 1;
        }
    }
    Some((cs[i..j].iter().collect(), j))
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.next).map(|t| &t.tok)
    }

    /// Where the next token starts, or the end.
    fn here(&self) -> usize {
        self.tokens.get(self.next).map_or(self.len, |t| t.at)
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.next += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Tok) -> Result<(), XPathError> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(self.expected(&format!("`{}`", symbol(t))))
        }
    }

    fn expected(&self, what: &str) -> XPathError {
        let found = match self.tokens.get(self.next) {
            Some(t) => format!(", found {}", describe(&t.tok)),
            None => ", found the end".into(),
        };
        syntax(format!("expected {what}{found}"), self.here())
    }

    fn expr(&mut self) -> Result<Expr, XPathError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(syntax("the expression is nested too deeply", self.here()));
        }
        let e = self.or();
        self.depth -= 1;
        e
    }

    fn or(&mut self) -> Result<Expr, XPathError> {
        let mut e = self.and()?;
        while self.eat(&Tok::Or) {
            e = Expr::Or(Box::new(e), Box::new(self.and()?));
        }
        Ok(e)
    }

    fn and(&mut self) -> Result<Expr, XPathError> {
        let mut e = self.equality()?;
        while self.eat(&Tok::And) {
            e = Expr::And(Box::new(e), Box::new(self.equality()?));
        }
        Ok(e)
    }

    fn equality(&mut self) -> Result<Expr, XPathError> {
        let mut e = self.relational()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Eq) => Cmp::Eq,
                Some(Tok::Ne) => Cmp::Ne,
                _ => return Ok(e),
            };
            self.next += 1;
            e = Expr::Compare(op, Box::new(e), Box::new(self.relational()?));
        }
    }

    fn relational(&mut self) -> Result<Expr, XPathError> {
        let mut e = self.additive()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Lt) => Cmp::Lt,
                Some(Tok::Le) => Cmp::Le,
                Some(Tok::Gt) => Cmp::Gt,
                Some(Tok::Ge) => Cmp::Ge,
                _ => return Ok(e),
            };
            self.next += 1;
            e = Expr::Compare(op, Box::new(e), Box::new(self.additive()?));
        }
    }

    fn additive(&mut self) -> Result<Expr, XPathError> {
        let mut e = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => Arith::Add,
                Some(Tok::Minus) => Arith::Sub,
                _ => return Ok(e),
            };
            self.next += 1;
            e = Expr::Arith(op, Box::new(e), Box::new(self.multiplicative()?));
        }
    }

    fn multiplicative(&mut self) -> Result<Expr, XPathError> {
        let mut e = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => Arith::Mul,
                Some(Tok::Div) => Arith::Div,
                Some(Tok::Mod) => Arith::Mod,
                _ => return Ok(e),
            };
            self.next += 1;
            e = Expr::Arith(op, Box::new(e), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Expr, XPathError> {
        if self.eat(&Tok::Minus) {
            self.depth += 1;
            if self.depth > MAX_DEPTH {
                return Err(syntax("the expression is nested too deeply", self.here()));
            }
            let e = self.unary();
            self.depth -= 1;
            return Ok(Expr::Neg(Box::new(e?)));
        }
        self.union()
    }

    fn union(&mut self) -> Result<Expr, XPathError> {
        let mut e = self.path_expr()?;
        while self.eat(&Tok::Pipe) {
            e = Expr::Union(Box::new(e), Box::new(self.path_expr()?));
        }
        Ok(e)
    }

    fn path_expr(&mut self) -> Result<Expr, XPathError> {
        let filter = matches!(
            self.peek(),
            Some(
                Tok::LParen
                    | Tok::Literal(_)
                    | Tok::Number(_)
                    | Tok::Function(_)
                    | Tok::Variable(_)
            )
        );
        if !filter {
            return Ok(Expr::Path(self.location_path()?));
        }
        let primary = self.primary()?;
        let mut predicates = Vec::new();
        while self.peek() == Some(&Tok::LBracket) {
            predicates.push(self.predicate()?);
        }
        let e = if predicates.is_empty() {
            primary
        } else {
            Expr::Filter(Box::new(primary), predicates)
        };
        let mut steps = Vec::new();
        match self.peek() {
            Some(Tok::Slash) => {
                self.next += 1;
                self.relative(&mut steps)?;
            }
            Some(Tok::SlashSlash) => {
                self.next += 1;
                steps.push(descendant_or_self());
                self.relative(&mut steps)?;
            }
            _ => return Ok(e),
        }
        Ok(Expr::Path(optimize(Path {
            start: Start::Filter(Box::new(e)),
            steps,
        })))
    }

    fn primary(&mut self) -> Result<Expr, XPathError> {
        let at = self.here();
        let Some(t) = self.tokens.get(self.next).map(|t| t.tok.clone()) else {
            return Err(self.expected("an expression"));
        };
        self.next += 1;
        match t {
            Tok::LParen => {
                let e = self.expr()?;
                self.expect(&Tok::RParen)?;
                Ok(e)
            }
            Tok::Literal(s) => Ok(Expr::Literal(s)),
            Tok::Number(v) => Ok(Expr::Number(v)),
            Tok::Variable(v) => Err(syntax(
                format!("variables such as `${v}` are not supported"),
                at,
            )),
            Tok::Function(name) => {
                let (f, min, max) = Func::named(&name)
                    .ok_or_else(|| syntax(format!("there is no function `{name}()`"), at))?;
                self.expect(&Tok::LParen)?;
                let mut args = Vec::new();
                if !self.eat(&Tok::RParen) {
                    loop {
                        args.push(self.expr()?);
                        if self.eat(&Tok::RParen) {
                            break;
                        }
                        if !self.eat(&Tok::Comma) {
                            return Err(self.expected("`,` or `)`"));
                        }
                    }
                }
                if args.len() < min || args.len() > max {
                    let wanted = match (min, max) {
                        (a, b) if a == b => format!("{a}"),
                        (a, usize::MAX) => format!("at least {a}"),
                        (a, b) => format!("{a} to {b}"),
                    };
                    let s = if max == 1 && min == 1 { "" } else { "s" };
                    return Err(syntax(
                        format!("`{name}()` takes {wanted} argument{s}, not {}", args.len()),
                        at,
                    ));
                }
                Ok(Expr::Call(f, args))
            }
            _ => {
                self.next -= 1;
                Err(self.expected("an expression"))
            }
        }
    }

    fn predicate(&mut self) -> Result<Expr, XPathError> {
        self.expect(&Tok::LBracket)?;
        let e = self.expr()?;
        self.expect(&Tok::RBracket)?;
        Ok(e)
    }

    fn location_path(&mut self) -> Result<Path, XPathError> {
        let mut steps = Vec::new();
        let start = match self.peek() {
            Some(Tok::Slash) => {
                self.next += 1;
                if self.starts_step() {
                    self.relative(&mut steps)?;
                }
                Start::Root
            }
            Some(Tok::SlashSlash) => {
                self.next += 1;
                steps.push(descendant_or_self());
                self.relative(&mut steps)?;
                Start::Root
            }
            _ => {
                if !self.starts_step() {
                    return Err(self.expected("an expression"));
                }
                self.relative(&mut steps)?;
                Start::Context
            }
        };
        Ok(optimize(Path { start, steps }))
    }

    fn starts_step(&self) -> bool {
        matches!(
            self.peek(),
            Some(
                Tok::Name { .. }
                    | Tok::NodeType(_)
                    | Tok::Axis(_)
                    | Tok::At
                    | Tok::Dot
                    | Tok::DotDot
            )
        )
    }

    fn relative(&mut self, steps: &mut Vec<Step>) -> Result<(), XPathError> {
        steps.push(self.step()?);
        loop {
            match self.peek() {
                Some(Tok::Slash) => {
                    self.next += 1;
                }
                Some(Tok::SlashSlash) => {
                    self.next += 1;
                    steps.push(descendant_or_self());
                }
                _ => return Ok(()),
            }
            steps.push(self.step()?);
        }
    }

    fn step(&mut self) -> Result<Step, XPathError> {
        if self.eat(&Tok::Dot) {
            return Ok(Step {
                axis: Axis::Itself,
                test: Test::Node,
                predicates: Vec::new(),
            });
        }
        if self.eat(&Tok::DotDot) {
            return Ok(Step {
                axis: Axis::Parent,
                test: Test::Node,
                predicates: Vec::new(),
            });
        }
        let at = self.here();
        let axis = match self.peek().cloned() {
            Some(Tok::At) => {
                self.next += 1;
                Axis::Attribute
            }
            Some(Tok::Axis(name)) => {
                self.next += 1;
                self.expect(&Tok::ColonColon)?;
                Axis::named(&name)
                    .ok_or_else(|| syntax(format!("there is no axis `{name}`"), at))?
            }
            _ => Axis::Child,
        };
        let test = match self.tokens.get(self.next).map(|t| t.tok.clone()) {
            Some(Tok::Name { prefix, local }) => {
                self.next += 1;
                match local {
                    Some(local) => Test::Name { prefix, local },
                    None => Test::Any { prefix },
                }
            }
            Some(Tok::NodeType(kind)) => {
                self.next += 1;
                self.expect(&Tok::LParen)?;
                let test = match kind.as_str() {
                    "comment" => Test::Comment,
                    "text" => Test::Text,
                    "node" => Test::Node,
                    _ => match self.peek().cloned() {
                        Some(Tok::Literal(target)) => {
                            self.next += 1;
                            Test::Pi(Some(target))
                        }
                        _ => Test::Pi(None),
                    },
                };
                self.expect(&Tok::RParen)?;
                test
            }
            _ => return Err(self.expected("a step")),
        };
        let mut predicates = Vec::new();
        while self.peek() == Some(&Tok::LBracket) {
            predicates.push(self.predicate()?);
        }
        Ok(Step {
            axis,
            test,
            predicates,
        })
    }
}

fn descendant_or_self() -> Step {
    Step {
        axis: Axis::DescendantOrSelf,
        test: Test::Node,
        predicates: Vec::new(),
    }
}

/// `descendant-or-self::node()/child::x[p]` is `descendant::x[p]` when `p` does not count
/// positions: one scan of the descendants instead of a child step for every node (§2.5 notes
/// the difference only for positional predicates).
fn optimize(mut path: Path) -> Path {
    let mut out: Vec<Step> = Vec::with_capacity(path.steps.len());
    for step in path.steps.drain(..) {
        let merge = out.last().is_some_and(|prev| {
            prev.axis == Axis::DescendantOrSelf
                && prev.test == Test::Node
                && prev.predicates.is_empty()
        }) && step.axis == Axis::Child
            && !step.predicates.iter().any(positional);
        if merge {
            out.pop();
            out.push(Step {
                axis: Axis::Descendant,
                ..step
            });
        } else {
            out.push(step);
        }
    }
    path.steps = out;
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(text: &str) -> String {
        parse(text).unwrap_err().to_string()
    }

    #[test]
    fn star_and_names_by_what_comes_before() {
        // `*` after an operand multiplies; `div` after one is an operator, else a name.
        let e = parse("div * div").unwrap();
        let Expr::Arith(Arith::Mul, a, b) = e else {
            panic!("{e:?}")
        };
        assert!(matches!(*a, Expr::Path(_)));
        assert!(matches!(*b, Expr::Path(_)));
        assert!(matches!(parse("*").unwrap(), Expr::Path(_)));
        assert!(matches!(
            parse("2 mod 3").unwrap(),
            Expr::Arith(Arith::Mod, ..)
        ));
        assert!(matches!(parse("a and b").unwrap(), Expr::And(..)));
    }

    #[test]
    fn abbreviations() {
        let Expr::Path(p) = parse("//order/@id").unwrap() else {
            panic!()
        };
        assert_eq!(p.start, Start::Root);
        // `//order` became one descendant step.
        assert_eq!(p.steps.len(), 2);
        assert_eq!(p.steps[0].axis, Axis::Descendant);
        assert_eq!(p.steps[1].axis, Axis::Attribute);
        // A positional predicate keeps the child step: `//a[1]` is each parent's first `a`.
        let Expr::Path(p) = parse("//a[1]").unwrap() else {
            panic!()
        };
        assert_eq!(p.steps[0].axis, Axis::DescendantOrSelf);
        assert_eq!(p.steps[1].axis, Axis::Child);
        let Expr::Path(p) = parse("//a[@x = 'y']").unwrap() else {
            panic!()
        };
        assert_eq!(p.steps.len(), 1);
        let Expr::Path(p) = parse("//a[last()]").unwrap() else {
            panic!()
        };
        assert_eq!(p.steps.len(), 2);
        let Expr::Path(p) = parse("../.").unwrap() else {
            panic!()
        };
        assert_eq!(p.steps[0].axis, Axis::Parent);
        assert_eq!(p.steps[1].axis, Axis::Itself);
        assert!(
            matches!(parse("/").unwrap(), Expr::Path(Path { start: Start::Root, ref steps }) if steps.is_empty())
        );
    }

    #[test]
    fn names_tests_and_axes() {
        let Expr::Path(p) =
            parse("ancestor-or-self :: p:x / child::p:* / processing-instruction('t') / größe")
                .unwrap()
        else {
            panic!()
        };
        assert_eq!(p.steps[0].axis, Axis::AncestorOrSelf);
        assert_eq!(
            p.steps[0].test,
            Test::Name {
                prefix: Some("p".into()),
                local: "x".into()
            }
        );
        assert_eq!(
            p.steps[1].test,
            Test::Any {
                prefix: Some("p".into())
            }
        );
        assert_eq!(p.steps[2].test, Test::Pi(Some("t".into())));
        assert_eq!(
            p.steps[3].test,
            Test::Name {
                prefix: None,
                local: "größe".into()
            }
        );
    }

    #[test]
    fn numbers_strings_and_calls() {
        assert_eq!(parse(".5").unwrap(), Expr::Number(0.5));
        assert_eq!(parse("12.").unwrap(), Expr::Number(12.0));
        assert_eq!(
            parse("'it''s'").unwrap_err().to_string(),
            "unexpected \"s\" at 5"
        );
        assert_eq!(parse("\"it's\"").unwrap(), Expr::Literal("it's".into()));
        assert!(matches!(
            parse("concat('a', 'b', 'c')").unwrap(),
            Expr::Call(Func::Concat, ref a) if a.len() == 3
        ));
        assert!(matches!(parse("count(//a)[1]").unwrap(), Expr::Filter(..)));
        assert!(matches!(parse("(//a | //b)[2]/c").unwrap(), Expr::Path(_)));
        assert!(matches!(parse("- - 1").unwrap(), Expr::Neg(_)));
    }

    #[test]
    fn errors_say_where() {
        assert_eq!(
            err("//order[@id = '1'"),
            "expected `]`, found the end at 18"
        );
        assert_eq!(err(""), "the expression is empty at 1");
        assert_eq!(
            err("//a[@id='1' and]"),
            "expected an expression, found `]` at 16"
        );
        assert_eq!(err("foo(1)"), "there is no function `foo()` at 1");
        assert_eq!(err("count()"), "`count()` takes 1 argument, not 0 at 1");
        assert_eq!(
            err("substring('a')"),
            "`substring()` takes 2 to 3 arguments, not 1 at 1"
        );
        assert_eq!(err("$x"), "variables such as `$x` are not supported at 1");
        assert_eq!(err("sideways::a"), "there is no axis `sideways` at 1");
        assert_eq!(err("a b"), "expected an operator, not `b` at 3");
        assert_eq!(err("'open"), "the string is not closed at 1");
        assert_eq!(err("a ! b"), "unexpected `!` at 3");
        assert_eq!(err("/a/"), "expected a step, found the end at 4");
        assert_eq!(err("a)"), "unexpected `)` at 2");
        assert_eq!(err("größe[) ]"), "expected an expression, found `)` at 7");
        let deep = format!("{}1{}", "(".repeat(300), ")".repeat(300));
        assert!(err(&deep).contains("nested too deeply"));
        let negs = format!("{}1", "-".repeat(300));
        assert!(err(&negs).contains("nested too deeply"));
    }
}
