//! The grid: the child elements of one element as table rows (`docs/spec/grid.md`).
//!
//! [`Grid::build`] finds the rows and the columns once, when the selection changes; a view
//! uses [`Grid::start`] and [`Grid::scan`] to find the columns of a long list over several
//! frames instead (GRID-3). Cells are decoded on demand: by [`Grid::cell`] for one command,
//! which walks the row's children, and for a view by [`crate::grid_cells::Cells`], which
//! does so once for a row on screen and not in every frame. Sorting
//! (GRID-14) and hiding empty columns (GRID-15) decode a whole column once, on request.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::OnceLock;

use xml_atlas_xml::{Folded, NONE, NodeId, NodeKind, NodeSplice};

use crate::command::{as_parsed, holds_only_text};
use crate::{Document, truncate};

/// Rows [`Grid::start`] examines for columns before it returns. Every row is examined in the
/// end (GRID-3); these show at once.
pub const COLUMN_SAMPLE: usize = 1000;

/// Up to this many rows, a change to a row's attributes or children examines every row again,
/// so that a column no row has any more goes away. Above it only the changed rows are
/// examined: a new column is added at the end, and one that became empty stays until the grid
/// is built again. 10,000 rows take about a millisecond.
pub const FOLLOW_RESCAN_ROWS: usize = 10_000;

/// Fewest rows a thread is given when a wide parent's rows are examined on several threads.
const PARALLEL_ROWS: usize = 2048;

/// Threads that examine a wide parent's rows for columns: the processor's, up to 8.
pub fn scan_threads() -> usize {
    static THREADS: OnceLock<usize> = OnceLock::new();
    *THREADS.get_or_init(|| {
        std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(8)
    })
}

/// Columns found while scanning are searched one by one up to this many, then hashed.
const SEEN_LIST: usize = 16;

/// Cell text is cut at this many characters (GRID-4).
pub const CELL_CHARS: usize = 200;

/// One column of the grid, in the order it was first met (GRID-2).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Column {
    /// An attribute of the row element.
    Attribute(String),
    /// A child element of the row element, by name.
    Child(String),
    /// The row element's own text.
    Text,
}

/// What a cell shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    /// The row has no such attribute, child or text. Drawn unlike an empty value (GRID-13).
    Absent,
    /// Decoded text, cut at [`CELL_CHARS`]. May be empty.
    Value(String),
    /// One child element with attributes or children of its own: shown by name (GRID-12).
    Element(String),
    /// Several child elements of the same name: shown by name and count (GRID-12).
    Repeated { name: String, count: usize },
}

/// What editing a cell changes (GRID-10); see [`crate::command::set_cell`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellTarget {
    /// An attribute of the row element; added when the row does not have it.
    Attribute { element: NodeId, name: String },
    /// The text of an element that holds only text, or nothing: the row's own, or that of
    /// the child a cell shows.
    Text(NodeId),
}

/// What a clicked row or cell stands for, as the second pane shows it (GRID-16).
#[derive(Clone, Debug)]
pub enum Detail {
    /// One element: its attributes and its children, as the grid would show it selected.
    Element(Grid),
    /// Several child elements of the same name: a table of their own.
    Elements(Grid),
    /// An attribute value or a text, whole: not cut at [`CELL_CHARS`].
    Value { name: String, value: String },
    /// The row has no such attribute, child or text.
    Absent,
}

impl Detail {
    /// The value, when it is one.
    pub fn value(&self) -> Option<&str> {
        match self {
            Detail::Value { value, .. } => Some(value),
            _ => None,
        }
    }
}

/// What the rows are sorted by (GRID-14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SortBy {
    /// The element name of each row.
    Name,
    /// The cells of one column.
    Column(Column),
}

/// A sort order (GRID-14). Rows with equal values stay in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sort {
    pub by: SortBy,
    pub descending: bool,
}

/// How the grid is shown: its sort and hidden columns. Kept by column, not by index, so it
/// applies again after an edit rebuilds the grid ([`Grid::set_view`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    /// `None`: document order.
    pub sort: Option<Sort>,
    /// Hidden columns (GRID-15), in the order they were hidden.
    pub hidden: Vec<Column>,
}

/// Rows and columns for the children of one element.
#[derive(Clone, Debug)]
pub struct Grid {
    /// The element whose children are the rows.
    pub parent: NodeId,
    /// Path of `parent`, e.g. `/orders/order[2]`. Built once: [`xml_atlas_xml::Index::path`]
    /// walks siblings and is too slow to repeat every frame.
    pub path: String,
    /// Attributes of `parent` itself, decoded (GRID-11).
    pub attributes: Vec<(String, String)>,
    /// Child elements of `parent` in document order (GRID-1); `parent` itself when it has
    /// none (GRID-5).
    /// In the order shown: document order unless sorted (GRID-14).
    pub rows: Vec<NodeId>,
    pub columns: Vec<Column>,
    /// The sort and hidden columns.
    view: View,
    /// When sorted, the position of each row in document order, from 0; empty otherwise.
    positions: Vec<u32>,
    /// The next child of `parent` to examine for columns; `None` when all have been.
    scan: Option<NodeId>,
}

impl Grid {
    /// The grid of the children of `parent`, every row examined for columns.
    pub fn build(doc: &Document, parent: NodeId) -> Self {
        let mut grid = Self::start(doc, parent);
        grid.scan(doc, usize::MAX);
        grid
    }

    /// The grid of the children of `parent`, with the columns of the first
    /// [`COLUMN_SAMPLE`] rows; [`Grid::scan`] finds the rest.
    pub fn start(doc: &Document, parent: NodeId) -> Self {
        let idx = &doc.index;
        let mut rows: Vec<NodeId> = match idx.child_elements(&doc.src, parent) {
            Some(list) => list.ids.clone(),
            None => idx
                .children(parent)
                .filter(|&c| idx.node(c).kind == NodeKind::Element)
                .collect(),
        };
        let scan = if rows.is_empty() {
            rows.push(parent);
            None
        } else {
            Some(parent + 1)
        };
        let path = idx.path(&doc.src, parent);
        let attributes = idx.attributes(&doc.src, parent);
        Self::with_rows(doc, parent, path, attributes, rows, scan)
    }

    /// Examines up to `children` more children of `parent` for columns (GRID-3). New columns
    /// are added at the end. Returns `true` once every row has been examined.
    pub fn scan(&mut self, doc: &Document, children: usize) -> bool {
        let Some(from) = self.scan else { return true };
        let idx = &doc.index;
        if let Some(list) = idx.child_elements(&doc.src, self.parent) {
            // A wide parent (D24): its child elements are listed, and the rows can be
            // examined on several threads.
            let at = list.ids.partition_point(|&r| r < from);
            let to = at.saturating_add(children).min(list.ids.len());
            self.add_columns_parallel(doc, &list.ids[at..to]);
            self.scan = list.ids.get(to).copied();
            return self.scan.is_none();
        }
        let end = self.parent as u64 + 1 + idx.node(self.parent).descendants as u64;
        let mut next = from as u64;
        let mut left = children;
        let rows = std::iter::from_fn(|| {
            while next < end && left > 0 {
                let id = next as NodeId;
                next += 1 + idx.node(id).descendants as u64;
                left -= 1;
                if idx.node(id).kind == NodeKind::Element {
                    return Some(id);
                }
            }
            None
        });
        add_columns(doc, &mut self.columns, rows);
        self.scan = (next < end).then_some(next as NodeId);
        self.scan.is_none()
    }

    /// [`add_columns`] for `rows`, split over up to [`scan_threads`] threads. The columns
    /// come out in the order one thread would find them: each chunk's new columns follow
    /// those of the chunks before it.
    fn add_columns_parallel(&mut self, doc: &Document, rows: &[NodeId]) {
        let threads = scan_threads().min(rows.len() / PARALLEL_ROWS).max(1);
        if threads == 1 {
            add_columns(doc, &mut self.columns, rows.iter().copied());
            return;
        }
        let known = &self.columns;
        let found: Vec<Vec<Column>> = std::thread::scope(|s| {
            let chunks: Vec<_> = rows
                .chunks(rows.len().div_ceil(threads))
                .map(|chunk| {
                    s.spawn(move || {
                        let mut columns = known.clone();
                        add_columns(doc, &mut columns, chunk.iter().copied());
                        columns.split_off(known.len())
                    })
                })
                .collect();
            chunks
                .into_iter()
                .map(|h| h.join().expect("column scan"))
                .collect()
        });
        let mut have: HashSet<Column> = self.columns.iter().cloned().collect();
        for c in found.into_iter().flatten() {
            if have.insert(c.clone()) {
                self.columns.push(c);
            }
        }
    }

    /// Whether every row has been examined for columns.
    pub fn scanned(&self) -> bool {
        self.scan.is_none()
    }

    /// A grid of the given rows, children of `parent`. With `scan`, the child of `parent` to
    /// examine from, the columns come from the first [`COLUMN_SAMPLE`] children on;
    /// without, from all the rows.
    fn with_rows(
        doc: &Document,
        parent: NodeId,
        path: String,
        attributes: Vec<(String, String)>,
        rows: Vec<NodeId>,
        scan: Option<NodeId>,
    ) -> Self {
        let mut grid = Self {
            parent,
            path,
            attributes,
            rows,
            columns: Vec::new(),
            view: View::default(),
            positions: Vec::new(),
            scan,
        };
        if scan.is_some() {
            grid.scan(doc, COLUMN_SAMPLE);
        } else {
            add_columns(doc, &mut grid.columns, grid.rows.iter().copied());
        }
        grid
    }

    /// Follows a change that replaced nodes ([`NodeSplice`], D19) instead of building the
    /// grid again: ids move, and the columns are found again when the change touches a row's
    /// attributes or children (see [`FOLLOW_RESCAN_ROWS`]). Rows are not sorted again, so a
    /// row does not jump away while it is typed in. Returns `false` when the grid must be
    /// built again: the element it shows was replaced, or rows were added or removed while it
    /// is sorted, or it shows only its element (GRID-5) and that changed.
    pub fn follow(&mut self, doc: &Document, ns: &NodeSplice) -> bool {
        let Some(parent) = ns.map(self.parent) else {
            return false;
        };
        let idx = &doc.index;
        let own = self.rows == [self.parent];
        if own && ns.parent == parent {
            return false;
        }
        // The rows whose attributes or children changed.
        let mut changed = Vec::new();
        if ns.parent == parent && ns.removed != ns.inserted {
            // Rows were replaced, added or removed. In document order the rows are the child
            // elements by id: put the new ones in place of the replaced ones. A sorted grid
            // is built again.
            if !self.positions.is_empty() {
                return false;
            }
            let lo = self.rows.partition_point(|&r| r < ns.at);
            let hi = self.rows.partition_point(|&r| r < ns.at + ns.removed);
            for r in &mut self.rows[hi..] {
                *r = ns.map(*r).expect("rows after the replaced nodes stay");
            }
            changed = ns
                .new_ids()
                .filter(|&id| {
                    let n = idx.node(id);
                    n.parent == parent && n.kind == NodeKind::Element
                })
                .collect();
            self.rows.splice(lo..hi, changed.iter().copied());
            if self.rows.is_empty() {
                return false;
            }
        } else {
            // A replaced row came back as the same number of nodes: it has the same id.
            for r in &mut self.rows {
                *r = ns.map(*r).unwrap_or(*r);
            }
            if ns.parent == parent {
                changed = ns
                    .new_ids()
                    .filter(|&id| {
                        let n = idx.node(id);
                        n.parent == parent && n.kind == NodeKind::Element
                    })
                    .collect();
            } else if !own {
                // The change lies in `ns.parent`, whose own tag may have changed too: a row's
                // columns change when it is the row, or one of the row's children.
                let up = idx.node(ns.parent).parent;
                if up == parent {
                    changed.push(ns.parent);
                } else if up != NONE && idx.node(up).parent == parent {
                    changed.push(up);
                }
            }
        }
        // The next child to examine moves with the ids; one that was replaced is examined
        // again from the first new node.
        self.scan = self.scan.map(|s| ns.map(s).unwrap_or(ns.at));
        self.parent = parent;
        self.attributes = idx.attributes(&doc.src, parent);
        let touched = ns.parent == parent || !changed.is_empty();
        if touched && self.rows.len() <= FOLLOW_RESCAN_ROWS {
            self.columns.clear();
            self.scan = Some(parent + 1);
            self.scan(doc, usize::MAX);
            let columns = &self.columns;
            self.view.hidden.retain(|c| columns.contains(c));
            if let Some(Sort {
                by: SortBy::Column(c),
                ..
            }) = &self.view.sort
                && !columns.contains(c)
            {
                self.view.sort = None;
            }
        } else {
            add_columns(doc, &mut self.columns, changed.into_iter());
        }
        true
    }

    /// The sort and hidden columns.
    pub fn view(&self) -> &View {
        &self.view
    }

    /// Applies a view kept from an earlier grid of the same element. Hidden columns this grid
    /// does not have are dropped, and so is a sort by such a column.
    pub fn set_view(&mut self, doc: &Document, view: View) {
        self.view.hidden = view
            .hidden
            .into_iter()
            .filter(|c| self.columns.contains(c))
            .collect();
        let sort = view.sort.filter(|s| match &s.by {
            SortBy::Name => true,
            SortBy::Column(c) => self.columns.contains(c),
        });
        self.sort(doc, sort);
    }

    /// Sorts the rows (GRID-14); `None` puts them back in document order. Values that are all
    /// numbers compare as numbers and come before text; text compares ignoring case, letter by code point. Cells
    /// with child elements come after values, by count, and absent cells last in either
    /// direction. Rows with equal values keep their document order.
    pub fn sort(&mut self, doc: &Document, sort: Option<Sort>) {
        if !self.positions.is_empty() {
            let mut rows: Vec<(u32, NodeId)> = self
                .positions
                .iter()
                .copied()
                .zip(self.rows.iter().copied())
                .collect();
            rows.sort_unstable_by_key(|&(p, _)| p);
            self.rows = rows.into_iter().map(|(_, r)| r).collect();
            self.positions.clear();
        }
        self.view.sort = sort;
        let Some(sort) = &self.view.sort else { return };
        let mut keyed: Vec<(SortKey, u32, NodeId)> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, &row)| (self.sort_key(doc, row, &sort.by), i as u32, row))
            .collect();
        // A stable sort keeps document order among equal values in both directions.
        keyed.sort_by(|a, b| a.0.cmp_in(&b.0, sort.descending));
        self.positions = keyed.iter().map(|&(_, p, _)| p).collect();
        self.rows = keyed.into_iter().map(|(_, _, r)| r).collect();
    }

    /// Where the `i`th row shown stands in document order, from 1: its row number.
    pub fn number(&self, i: usize) -> usize {
        self.positions.get(i).map_or(i, |&p| p as usize) + 1
    }

    /// Whether `column` is hidden (GRID-15).
    pub fn is_hidden(&self, column: &Column) -> bool {
        self.view.hidden.contains(column)
    }

    /// Hides `column` (GRID-15).
    pub fn hide(&mut self, column: &Column) {
        if self.columns.contains(column) && !self.is_hidden(column) {
            self.view.hidden.push(column.clone());
        }
    }

    /// Shows a hidden `column` again.
    pub fn show(&mut self, column: &Column) {
        self.view.hidden.retain(|c| c != column);
    }

    /// Shows every hidden column again.
    pub fn show_all(&mut self) {
        self.view.hidden.clear();
    }

    /// Hides the columns that are empty in every row: absent, or an empty value (GRID-15).
    /// Returns how many it hid.
    pub fn hide_empty(&mut self, doc: &Document) -> usize {
        let mut empty: Vec<Column> = self
            .columns
            .iter()
            .filter(|c| !self.is_hidden(c))
            .cloned()
            .collect();
        for &row in &self.rows {
            if empty.is_empty() {
                break;
            }
            empty.retain(|c| match self.cell(doc, row, c) {
                Cell::Absent => true,
                Cell::Value(v) => v.is_empty(),
                Cell::Element(_) | Cell::Repeated { .. } => false,
            });
        }
        let n = empty.len();
        self.view.hidden.extend(empty);
        n
    }

    /// The cell of `row` in `column`.
    pub fn cell(&self, doc: &Document, row: NodeId, column: &Column) -> Cell {
        let idx = &doc.index;
        let src = &doc.src;
        match column {
            Column::Attribute(attr) => idx
                .attributes(src, row)
                .into_iter()
                .find(|(k, _)| k == attr)
                .map_or(Cell::Absent, |(_, v)| Cell::Value(truncate(&v, CELL_CHARS))),
            Column::Child(child) => {
                let mut matching = idx.children(row).filter(|&c| {
                    idx.node(c).kind == NodeKind::Element && idx.name(src, c) == child.as_bytes()
                });
                let first = matching.next();
                child_cell(doc, child, first, 1 + matching.count())
            }
            Column::Text => {
                if idx.has_text(row) {
                    Cell::Value(text(doc, row))
                } else {
                    Cell::Absent
                }
            }
        }
    }

    /// The node a cell stands for: the attribute's element, or the first matching child.
    /// `None` when the cell is [`Cell::Absent`].
    pub fn cell_node(&self, doc: &Document, row: NodeId, column: &Column) -> Option<NodeId> {
        match column {
            Column::Child(child) => doc.index.children(row).find(|&c| {
                doc.index.node(c).kind == NodeKind::Element
                    && doc.index.name(&doc.src, c) == child.as_bytes()
            }),
            Column::Attribute(_) | Column::Text => {
                (self.cell(doc, row, column) != Cell::Absent).then_some(row)
            }
        }
    }

    /// What the cell of `row` in `column` stands for, or the row itself when `column` is
    /// `None` (GRID-16). Builds a grid for an element, so it is meant for a click, not for
    /// every frame.
    pub fn detail(&self, doc: &Document, row: NodeId, column: Option<&Column>) -> Detail {
        let idx = &doc.index;
        let src = &doc.src;
        let whole = |id| as_parsed(&idx.element_text(src, id));
        let Some(column) = column else {
            return Detail::Element(Grid::build(doc, row));
        };
        match column {
            Column::Attribute(attr) => idx
                .attributes(src, row)
                .into_iter()
                .find(|(k, _)| k == attr)
                .map_or(Detail::Absent, |(k, v)| Detail::Value {
                    name: format!("@{k}"),
                    value: v,
                }),
            Column::Text if idx.has_text(row) => Detail::Value {
                name: "text()".into(),
                value: whole(row),
            },
            Column::Text => Detail::Absent,
            Column::Child(child) => {
                let matching: Vec<NodeId> = idx
                    .children(row)
                    .filter(|&c| {
                        idx.node(c).kind == NodeKind::Element
                            && idx.name(src, c) == child.as_bytes()
                    })
                    .collect();
                match matching[..] {
                    [] => Detail::Absent,
                    [one] if !has_structure(doc, one) => Detail::Value {
                        name: child.clone(),
                        value: whole(one),
                    },
                    [one] => Detail::Element(Grid::build(doc, one)),
                    _ => {
                        let path = format!("{}/{child}", idx.path(src, row));
                        Detail::Elements(Grid::with_rows(
                            doc,
                            row,
                            path,
                            Vec::new(),
                            matching,
                            None,
                        ))
                    }
                }
            }
        }
    }

    /// What editing the cell of `row` in `column` would change, and its whole current value
    /// (not cut at [`CELL_CHARS`]). `None` when the cell cannot be edited: it stands for child
    /// elements (GRID-12), the child is missing, or the element holds more than text.
    pub fn edit_target(
        &self,
        doc: &Document,
        row: NodeId,
        column: &Column,
    ) -> Option<(CellTarget, String)> {
        let idx = &doc.index;
        let src = &doc.src;
        let text = |id| {
            holds_only_text(doc, id)
                .then(|| (CellTarget::Text(id), as_parsed(&idx.element_text(src, id))))
        };
        match column {
            Column::Attribute(name) => {
                let value = idx
                    .attributes(src, row)
                    .into_iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v)
                    .unwrap_or_default();
                let target = CellTarget::Attribute {
                    element: row,
                    name: name.clone(),
                };
                Some((target, value))
            }
            Column::Text => text(row),
            Column::Child(_) => match self.cell(doc, row, column) {
                Cell::Value(_) => text(self.cell_node(doc, row, column)?),
                _ => None,
            },
        }
    }
}

impl Grid {
    fn sort_key(&self, doc: &Document, row: NodeId, by: &SortBy) -> SortKey {
        let value = |v: String| match v.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => SortKey::Number(n),
            _ => SortKey::Text(v.to_lowercase(), v),
        };
        match by {
            SortBy::Name => value(name(doc, row)),
            SortBy::Column(c) => match self.cell(doc, row, c) {
                Cell::Absent => SortKey::Absent,
                Cell::Value(v) => value(v),
                Cell::Element(_) => SortKey::Nested(1),
                Cell::Repeated { count, .. } => SortKey::Nested(count),
            },
        }
    }
}

/// What a row sorts by (GRID-14). The variants are in ascending order.
#[derive(Debug)]
enum SortKey {
    Number(f64),
    /// Lower-cased for comparing, then as written to order `a` and `A` the same way each time.
    Text(String, String),
    /// Child elements, by count.
    Nested(usize),
    Absent,
}

impl SortKey {
    fn rank(&self) -> u8 {
        match self {
            Self::Number(_) => 0,
            Self::Text(..) => 1,
            Self::Nested(_) => 2,
            Self::Absent => 3,
        }
    }

    /// Ascending or descending, with absent values last either way.
    fn cmp_in(&self, other: &Self, descending: bool) -> Ordering {
        let order = match (self, other) {
            (Self::Absent, Self::Absent) => return Ordering::Equal,
            (Self::Absent, _) => return Ordering::Greater,
            (_, Self::Absent) => return Ordering::Less,
            (Self::Number(a), Self::Number(b)) => a.total_cmp(b),
            (Self::Text(a, x), Self::Text(b, y)) => a.cmp(b).then_with(|| x.cmp(y)),
            (Self::Nested(a), Self::Nested(b)) => a.cmp(b),
            _ => self.rank().cmp(&other.rank()),
        };
        if descending { order.reverse() } else { order }
    }
}

fn name(doc: &Document, id: NodeId) -> String {
    String::from_utf8_lossy(doc.index.name(&doc.src, id)).into_owned()
}

/// The cell for the `count` children called `name`, the first of them `first` (GRID-12).
pub(crate) fn child_cell(doc: &Document, name: &str, first: Option<NodeId>, count: usize) -> Cell {
    let Some(first) = first else {
        return Cell::Absent;
    };
    if count > 1 {
        Cell::Repeated {
            name: name.to_owned(),
            count,
        }
    } else if has_structure(doc, first) {
        Cell::Element(name.to_owned())
    } else {
        Cell::Value(text(doc, first))
    }
}

/// Whether an element has attributes or child elements, so that its text alone would hide
/// something.
fn has_structure(doc: &Document, id: NodeId) -> bool {
    let idx = &doc.index;
    idx.children(id)
        .any(|c| idx.node(c).kind == NodeKind::Element)
        || !idx.attributes(&doc.src, id).is_empty()
}

/// The text and CDATA children of an element, decoded, joined and cut at [`CELL_CHARS`].
pub(crate) fn text(doc: &Document, id: NodeId) -> String {
    truncate(&doc.index.element_text(&doc.src, id), CELL_CHARS)
}

/// Adds the columns of `rows` that `columns` lacks: their attributes, child element names and
/// text, in the order first met (GRID-3). Reads names as bytes and allocates only per new
/// column, not per row, so it can look at every row of a large list.
fn add_columns(doc: &Document, columns: &mut Vec<Column>, rows: impl Iterator<Item = NodeId>) {
    let idx = &doc.index;
    let src = &doc.src[..];
    let mut found = Vec::new();
    let mut seen = Seen::default();
    let mut text = false;
    for c in columns.iter() {
        match c {
            Column::Attribute(n) => seen.list.push((false, n.as_bytes())),
            Column::Child(n) => seen.list.push((true, n.as_bytes())),
            Column::Text => text = true,
        }
    }
    for row in rows {
        for name in idx.attribute_names(src, row) {
            seen.add(&mut found, false, name);
        }
        for c in idx.children(row) {
            match idx.node(c).kind {
                NodeKind::Element => seen.add(&mut found, true, idx.name(src, c)),
                NodeKind::Text | NodeKind::CData if !text => {
                    text = true;
                    found.push(Column::Text);
                }
                _ => {}
            }
        }
        if !text && idx.node(row).folded != Folded::No {
            text = true;
            found.push(Column::Text);
        }
    }
    columns.extend(found);
}

/// Columns met so far while scanning, by kind and raw name. Searched in order while few, the
/// common case, which is twice as fast as hashing; hashed when many, so that a list whose
/// rows all differ stays linear.
#[derive(Default)]
struct Seen<'s> {
    list: Vec<(bool, &'s [u8])>,
    set: HashSet<(bool, &'s [u8])>,
}

impl<'s> Seen<'s> {
    /// Adds an attribute (`child` false) or child element column, unless it is there already.
    fn add(&mut self, columns: &mut Vec<Column>, child: bool, name: &'s [u8]) {
        let key = (child, name);
        let new = if self.list.len() < SEEN_LIST {
            !self.list.contains(&key)
        } else {
            if self.set.is_empty() {
                self.set.extend(self.list.iter().copied());
            }
            self.set.insert(key)
        };
        if new {
            self.list.push(key);
            let name = String::from_utf8_lossy(name).into_owned();
            columns.push(if child {
                Column::Child(name)
            } else {
                Column::Attribute(name)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDERS: &str = r#"<orders>
  <order id="1" status="open">
    <customer>Müller</customer>
    <total currency="EUR">12.50</total>
    <items><item sku="A1"/><item sku="B2"/></items>
  </order>
  <order id="2">
    <customer></customer>
    <note>first</note>
    <note>second</note>
  </order>
  <order id="3" status="closed" extra="late">
    <customer>Ø &amp; Co</customer>
    <gift/>
  </order>
</orders>"#;

    fn doc(src: &str) -> Document {
        Document::from_bytes(src.as_bytes().to_vec())
    }

    fn grid_of(doc: &Document, element: &str) -> Grid {
        let id = (0..doc.index.len() as NodeId)
            .find(|&id| {
                doc.index.node(id).kind == NodeKind::Element
                    && doc.index.name(&doc.src, id) == element.as_bytes()
            })
            .unwrap();
        Grid::build(doc, id)
    }

    fn cells(doc: &Document, grid: &Grid, column: &Column) -> Vec<Cell> {
        grid.rows
            .iter()
            .map(|&r| grid.cell(doc, r, column))
            .collect()
    }

    fn value(s: &str) -> Cell {
        Cell::Value(s.into())
    }

    fn attr(s: &str) -> Column {
        Column::Attribute(s.into())
    }

    fn child(s: &str) -> Column {
        Column::Child(s.into())
    }

    #[test]
    fn columns_in_order_of_first_appearance() {
        let d = doc(ORDERS);
        let g = grid_of(&d, "orders");
        assert_eq!(g.rows.len(), 3);
        assert_eq!(
            g.columns,
            [
                attr("id"),
                attr("status"),
                child("customer"),
                child("total"),
                child("items"),
                child("note"),
                attr("extra"),
                child("gift"),
            ]
        );
    }

    /// GRID-12: a child with structure shows its name, a repeated child its name and count.
    #[test]
    fn nested_and_repeated_children() {
        let d = doc(ORDERS);
        let g = grid_of(&d, "orders");
        assert_eq!(
            cells(&d, &g, &child("total")),
            [Cell::Element("total".into()), Cell::Absent, Cell::Absent]
        );
        assert_eq!(
            cells(&d, &g, &child("items")),
            [Cell::Element("items".into()), Cell::Absent, Cell::Absent]
        );
        assert_eq!(
            cells(&d, &g, &child("note")),
            [
                Cell::Absent,
                Cell::Repeated {
                    name: "note".into(),
                    count: 2
                },
                Cell::Absent
            ]
        );
    }

    /// GRID-13: an absent attribute or child differs from an empty one.
    #[test]
    fn absent_differs_from_empty() {
        let d = doc(ORDERS);
        let g = grid_of(&d, "orders");
        assert_eq!(
            cells(&d, &g, &attr("status")),
            [value("open"), Cell::Absent, value("closed")]
        );
        assert_eq!(
            cells(&d, &g, &child("customer")),
            [value("Müller"), value(""), value("Ø & Co")]
        );
        // `<gift/>` has no text, attributes or children: an empty value, not absent.
        assert_eq!(
            cells(&d, &g, &child("gift")),
            [Cell::Absent, Cell::Absent, value("")]
        );
    }

    /// GRID-11: the selected element's own attributes, decoded.
    #[test]
    fn attributes_of_the_selected_element() {
        let d = doc(r#"<shop name="Zentrale" city="Gr&#xE4;z"><a/></shop>"#);
        let g = grid_of(&d, "shop");
        assert_eq!(
            g.attributes,
            [
                ("name".to_string(), "Zentrale".to_string()),
                ("city".to_string(), "Gräz".to_string())
            ]
        );
        assert_eq!(grid_of(&d, "a").attributes, []);
    }

    /// GRID-5: an element without child elements is its own single row.
    #[test]
    fn leaf_element_is_its_own_row() {
        let d = doc(r#"<r><price currency="EUR">12.50</price></r>"#);
        let g = grid_of(&d, "price");
        assert_eq!(g.rows, [g.parent]);
        assert_eq!(g.columns, [attr("currency"), Column::Text]);
        assert_eq!(g.cell(&d, g.parent, &Column::Text), value("12.50"));
    }

    #[test]
    fn text_column_and_mixed_content() {
        let d = doc("<r><p>Hello <b>big</b> world</p><p><b>only</b></p></r>");
        let g = grid_of(&d, "r");
        assert_eq!(g.columns, [Column::Text, child("b")]);
        assert_eq!(
            cells(&d, &g, &Column::Text),
            [value("Hello  world"), Cell::Absent]
        );
        assert_eq!(cells(&d, &g, &child("b")), [value("big"), value("only")]);
    }

    #[test]
    fn long_values_are_cut() {
        let long = "ä".repeat(CELL_CHARS + 50);
        let d = doc(&format!("<r><a v=\"{long}\"/></r>"));
        let g = grid_of(&d, "r");
        let Cell::Value(v) = g.cell(&d, g.rows[0], &attr("v")) else {
            panic!()
        };
        assert_eq!(v.chars().count(), CELL_CHARS + 1);
        assert!(v.ends_with('…'));
    }

    #[test]
    fn cell_node_points_at_the_child() {
        let d = doc(ORDERS);
        let g = grid_of(&d, "orders");
        let first = g.rows[0];
        let total = g.cell_node(&d, first, &child("total")).unwrap();
        assert_eq!(d.index.name(&d.src, total), b"total");
        assert_eq!(g.cell_node(&d, first, &attr("id")), Some(first));
        assert_eq!(g.cell_node(&d, g.rows[1], &child("total")), None);
        assert_eq!(g.cell_node(&d, g.rows[1], &attr("status")), None);
    }

    #[test]
    fn edit_targets_name_what_a_cell_changes_with_its_whole_value() {
        let long = "ä".repeat(CELL_CHARS + 50);
        let src = format!(
            "\u{feff}<r>\r\n  <a id=\"1\" t=\"x &amp; y\"><c>{long}</c><m>p<b/>q</m><n><!-- c --></n><e k=\"v\"/></a>\r\n  <a/>\r\n</r>\r\n"
        );
        let doc = doc(&src);
        let grid = grid_of(&doc, "r");
        let (a1, a2) = (grid.rows[0], grid.rows[1]);
        let child = |row, name: &str| {
            grid.cell_node(&doc, row, &Column::Child(name.into()))
                .unwrap()
        };
        let target = |row, c: Column| grid.edit_target(&doc, row, &c);
        assert_eq!(
            target(a1, Column::Attribute("t".into())),
            Some((
                CellTarget::Attribute {
                    element: a1,
                    name: "t".into()
                },
                "x & y".into()
            ))
        );
        // A missing attribute can be added: its value starts empty.
        assert_eq!(
            target(a2, Column::Attribute("t".into())),
            Some((
                CellTarget::Attribute {
                    element: a2,
                    name: "t".into()
                },
                String::new()
            ))
        );
        // The whole text, not the 200 characters the cell shows.
        assert_eq!(
            target(a1, Column::Child("c".into())),
            Some((CellTarget::Text(child(a1, "c")), long))
        );
        // Mixed content, a comment, a child with attributes, a missing child: not editable.
        assert_eq!(target(a1, Column::Child("m".into())), None);
        assert_eq!(target(a1, Column::Child("n".into())), None);
        assert_eq!(target(a1, Column::Child("e".into())), None);
        assert_eq!(target(a2, Column::Child("c".into())), None);
        // An empty row element can take text; one with children cannot.
        let grid_a = grid_of(&doc, "c");
        assert_eq!(grid_a.rows, [child(a1, "c")]);
        assert_eq!(
            Grid::build(&doc, a2).edit_target(&doc, a2, &Column::Text),
            Some((CellTarget::Text(a2), String::new()))
        );
        assert_eq!(target(a1, Column::Text), None);
    }

    fn sorted(doc: &Document, grid: &mut Grid, by: SortBy, descending: bool) -> Vec<String> {
        grid.sort(doc, Some(Sort { by, descending }));
        let id = attr("id");
        grid.rows
            .iter()
            .map(|&r| match grid.cell(doc, r, &id) {
                Cell::Value(v) => v,
                other => panic!("{other:?}"),
            })
            .collect()
    }

    const PRICES: &str = "\u{feff}<list>\r\n<p id=\"a\" v=\"10\" n=\"Zoë\"/>\r\n<p id=\"b\" v=\"9.5\" n=\"äpfel\"/>\r\n<p id=\"c\" n=\"Apfel\"/>\r\n<p id=\"d\" v=\"n/a\" n=\"zoë\"/>\r\n<p id=\"e\" v=\"-2\" n=\"apfel\"/>\r\n<q id=\"f\" v=\"10\"/>\r\n</list>\r\n";

    /// GRID-14: numbers as numbers and before text, absent last both ways, ties in document
    /// order; the row number stays the document position.
    #[test]
    fn sort_by_column() {
        let d = doc(PRICES);
        let mut g = grid_of(&d, "list");
        let v = || SortBy::Column(attr("v"));
        assert_eq!(
            sorted(&d, &mut g, v(), false),
            ["e", "b", "a", "f", "d", "c"]
        );
        assert_eq!(g.number(0), 5);
        assert_eq!(g.number(5), 3);
        assert_eq!(
            sorted(&d, &mut g, v(), true),
            ["d", "a", "f", "b", "e", "c"]
        );
        // Text ignores case, then "A" before "a"; letters compare by code point, so "ä" comes
        // after "z".
        let n = || SortBy::Column(attr("n"));
        assert_eq!(
            sorted(&d, &mut g, n(), false),
            ["c", "e", "a", "d", "b", "f"]
        );
        assert_eq!(
            sorted(&d, &mut g, SortBy::Name, true),
            ["f", "a", "b", "c", "d", "e"]
        );
        g.sort(&d, None);
        assert_eq!(g.rows, grid_of(&d, "list").rows);
        assert_eq!(g.number(2), 3);
    }

    /// GRID-14: cells with child elements sort after values, by count.
    #[test]
    fn sort_nested_cells() {
        let d = doc(ORDERS);
        let mut g = grid_of(&d, "orders");
        let note = || SortBy::Column(child("note"));
        assert_eq!(sorted(&d, &mut g, note(), false), ["2", "1", "3"]);
        let customer = || SortBy::Column(child("customer"));
        // Empty text first, then "Müller", then "Ø & Co".
        assert_eq!(sorted(&d, &mut g, customer(), false), ["2", "1", "3"]);
    }

    /// GRID-15: hiding, showing, and hiding the columns empty in every row.
    #[test]
    fn hide_columns() {
        let d = doc("<r><a x=\"\" y=\"1\"><e/></a><a x=\"\"><e></e><f>ü</f></a></r>");
        let mut g = grid_of(&d, "r");
        assert_eq!(g.columns, [attr("x"), attr("y"), child("e"), child("f")]);
        assert_eq!(g.hide_empty(&d), 2);
        assert!(g.is_hidden(&attr("x")) && g.is_hidden(&child("e")));
        assert!(!g.is_hidden(&attr("y")) && !g.is_hidden(&child("f")));
        assert_eq!(g.hide_empty(&d), 0);
        g.hide(&attr("y"));
        g.show(&attr("x"));
        assert_eq!(g.view().hidden, [child("e"), attr("y")]);
        g.show_all();
        assert_eq!(g.view().hidden, []);
    }

    /// The view applies again to a rebuilt grid, without columns it no longer has.
    #[test]
    fn view_applies_to_a_rebuilt_grid() {
        let d = doc(PRICES);
        let mut g = grid_of(&d, "list");
        g.hide(&attr("n"));
        g.sort(
            &d,
            Some(Sort {
                by: SortBy::Column(attr("v")),
                descending: true,
            }),
        );
        let view = g.view().clone();
        let mut again = grid_of(&d, "list");
        again.set_view(&d, view.clone());
        assert_eq!(again.rows, g.rows);
        assert!(again.is_hidden(&attr("n")));
        let other = doc("<list><p id=\"1\"/></list>");
        let mut rebuilt = grid_of(&other, "list");
        rebuilt.set_view(&other, view);
        assert_eq!(rebuilt.view(), &View::default());
    }

    /// The second pane shows what a click stands for, whole (GRID-16).
    #[test]
    fn detail_of_a_row_or_cell() {
        let long = "ä".repeat(CELL_CHARS + 10);
        let src = format!(
            "\u{feff}<orders>\r\n  <order id=\"1\" memo=\"{long}\">\r\n    \
             <customer>Ø &amp; Co</customer>\r\n    <total currency=\"EUR\">12.50</total>\r\n    \
             <note>a\r\nb</note><note>c</note>\r\n  </order>\r\n</orders>"
        );
        let d = doc(&src);
        let grid = grid_of(&d, "orders");
        let row = grid.rows[0];

        // A row: the element with its attributes and children.
        let Detail::Element(g) = grid.detail(&d, row, None) else {
            panic!("an element")
        };
        assert_eq!(g.parent, row);
        assert_eq!(g.attributes.len(), 2);
        assert_eq!(g.rows.len(), 4);

        // A value cell: the whole value, decoded, with line ends as parsed.
        let memo = grid.detail(&d, row, Some(&attr("memo")));
        assert!(
            matches!(&memo, Detail::Value { name, value } if name == "@memo" && *value == long)
        );
        let customer = grid.detail(&d, row, Some(&child("customer")));
        assert!(
            matches!(&customer, Detail::Value { name, value } if name == "customer" && value == "Ø & Co")
        );
        assert!(matches!(
            grid.detail(&d, row, Some(&attr("status"))),
            Detail::Absent
        ));
        assert!(matches!(
            grid.detail(&d, row, Some(&Column::Text)),
            Detail::Absent
        ));

        // A child with an attribute: that element.
        let Detail::Element(total) = grid.detail(&d, row, Some(&child("total"))) else {
            panic!("an element")
        };
        assert_eq!(
            total.attributes,
            [("currency".to_string(), "EUR".to_string())]
        );
        assert_eq!(cells(&d, &total, &Column::Text), [value("12.50")]);

        // Several of the same name: a table of them, nothing else of the row.
        let Detail::Elements(notes) = grid.detail(&d, row, Some(&child("note"))) else {
            panic!("a table")
        };
        assert_eq!(notes.rows.len(), 2);
        assert_eq!(notes.columns, [Column::Text]);
        assert_eq!(notes.path, "/orders/order/note");
        assert_eq!(
            notes.detail(&d, notes.rows[0], Some(&Column::Text)).value(),
            Some("a\nb")
        );
    }

    /// Random typed changes in a grid's rows: when the grid follows, it equals a grid built
    /// afresh, cell for cell; it follows most of them (D19).
    #[test]
    fn following_a_change_equals_a_rebuild() {
        let mut state = 7u64;
        let mut below = |n: usize| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as usize) % n.max(1)
        };
        let typed = ["x", "ü", "<", "\"", "<total/>", " a=\"1\"", "\r\n", "</"];
        let (mut followed, mut total) = (0, 0);
        for _ in 0..40 {
            let mut doc = Document::from_bytes(ORDERS.replace('\n', "\r\n").into_bytes());
            for _ in 0..30 {
                let elements: Vec<NodeId> = (0..doc.index.len() as NodeId)
                    .filter(|&id| doc.index.node(id).kind == NodeKind::Element)
                    .collect();
                if elements.is_empty() {
                    break;
                }
                let shown = elements[below(elements.len())];
                let mut grid = Grid::build(&doc, shown);
                let at = crate::text::snap(&doc.src, below(doc.src.len() + 1));
                let sel = crate::text::Selection::cursor(at);
                let change = if below(4) == 0 {
                    crate::text::backspace(&doc, sel, false)
                } else {
                    crate::text::insert(&doc, sel, typed[below(typed.len())])
                };
                let Some(change) = change else { continue };
                doc.apply_typed(change.edit).unwrap();
                let ns = doc.node_splice().expect("local");
                total += 1;
                if !grid.follow(&doc, &ns) {
                    continue;
                }
                followed += 1;
                let fresh = Grid::build(&doc, grid.parent);
                assert_eq!(grid.rows, fresh.rows, "{ns:?}");
                assert_eq!(grid.columns, fresh.columns, "{ns:?}");
                assert_eq!(grid.attributes, fresh.attributes);
                for &r in &grid.rows {
                    for c in &grid.columns {
                        assert_eq!(grid.cell(&doc, r, c), fresh.cell(&doc, r, c));
                    }
                }
                if below(4) == 0 {
                    doc.reparse();
                }
            }
        }
        assert!(followed * 2 > total, "followed only {followed} of {total}");
    }

    /// A list of `n` rows with a byte order mark and CRLF; row `late` alone has an attribute
    /// and a child the others lack.
    fn long_list(n: usize, late: usize) -> Document {
        let mut src = String::from("\u{feff}<liste>\r\n");
        for i in 0..n {
            if i == late {
                src.push_str(&format!(
                    "<zeile nr=\"{i}\" größe=\"groß\"><später>ä</später></zeile>\r\n"
                ));
            } else {
                src.push_str(&format!("<zeile nr=\"{i}\"/>\r\n"));
            }
        }
        src.push_str("</liste>\r\n");
        Document::from_bytes(src.into_bytes())
    }

    /// A column only a row far down has is there (GRID-3): at once when built, and after
    /// scanning when started; at the end, as it was met last.
    #[test]
    fn a_column_far_down_is_found() {
        let doc = long_list(3 * COLUMN_SAMPLE, 2 * COLUMN_SAMPLE + 7);
        let list = doc.index.root_element().unwrap();
        let all = [attr("nr"), attr("größe"), child("später")];
        assert_eq!(Grid::build(&doc, list).columns, all);

        let mut grid = Grid::start(&doc, list);
        assert_eq!(grid.columns, [attr("nr")]);
        assert!(!grid.scanned());
        let mut steps = 0;
        while !grid.scan(&doc, 100) {
            steps += 1;
        }
        assert!(steps > 10, "scanned in {steps} steps");
        assert!(grid.scanned());
        assert_eq!(grid.columns, all);
        let late = grid.rows[2 * COLUMN_SAMPLE + 7];
        assert_eq!(grid.cell(&doc, late, &attr("größe")), value("groß"));
        assert_eq!(grid.cell(&doc, late, &child("später")), value("ä"));
    }

    /// A wide list is examined on several threads; the columns are those one pass in document
    /// order finds, in the same order, also when a column is first met in a later chunk and
    /// again in an earlier one.
    #[test]
    fn a_wide_list_is_examined_like_one_pass() {
        let n: usize = 40_000;
        let mut src = String::from("\u{feff}<liste>\r\n");
        for i in 0..n {
            let a = (i * 7919) % 97;
            let c = (i * 104_729) % 61;
            // Rows near the end bring columns no earlier row has.
            let late = if i > n - 50 {
                format!(" spät{i}=\"\"")
            } else {
                String::new()
            };
            let text = if i % 5000 == 4999 { "Grüße" } else { "" };
            src.push_str(&format!(
                "<zeile nr=\"{i}\" ä{a}=\"x\"{late}><kind{c}>ö</kind{c}>{text}</zeile>\r\n"
            ));
        }
        src.push_str("</liste>\r\n");
        let doc = Document::from_bytes(src.into_bytes());
        let list = doc.index.root_element().unwrap();
        assert!(doc.index.child_elements(&doc.src, list).is_some(), "wide");
        let rows: Vec<NodeId> = doc
            .index
            .children(list)
            .filter(|&c| doc.index.node(c).kind == NodeKind::Element)
            .collect();
        let mut one_pass = Vec::new();
        add_columns(&doc, &mut one_pass, rows.iter().copied());
        assert!(one_pass.len() > 97 + 61 + 40, "{}", one_pass.len());
        assert_eq!(Grid::build(&doc, list).columns, one_pass);
        let mut grid = Grid::start(&doc, list);
        while !grid.scan(&doc, 7000) {}
        assert_eq!(grid.columns, one_pass);
    }

    /// In a list longer than [`FOLLOW_RESCAN_ROWS`], typing a new attribute or child into a
    /// row adds its column at the end, also while the rest is still being scanned; a scan
    /// that is under way goes on from where it was.
    #[test]
    fn a_long_list_follows_new_columns() {
        let n = FOLLOW_RESCAN_ROWS + 10;
        let mut doc = long_list(n, n - 1);
        let list = doc.index.root_element().unwrap();
        let mut grid = Grid::start(&doc, list);
        assert_eq!(grid.columns, [attr("nr")]);

        let type_at = |doc: &mut Document, at: &str, text: &str| {
            let i = doc
                .src
                .windows(at.len())
                .position(|w| w == at.as_bytes())
                .unwrap();
            let sel = crate::text::Selection::cursor(i + at.len());
            let change = crate::text::insert(doc, sel, text).unwrap();
            doc.apply_typed(change.edit).unwrap();
            doc.node_splice().expect("local")
        };
        // An attribute in row 5's start tag.
        let ns = type_at(&mut doc, "<zeile nr=\"5\"", " neu=\"ja\"");
        assert!(grid.follow(&doc, &ns));
        assert_eq!(grid.columns, [attr("nr"), attr("neu")]);
        // A child element in row 6, pasted over the whole empty row.
        let row = b"<zeile nr=\"6\"/>";
        let i = doc.src.windows(row.len()).position(|w| w == row).unwrap();
        let sel = crate::text::Selection {
            anchor: i,
            head: i + row.len(),
        };
        let change = crate::text::paste(&doc, sel, "<zeile nr=\"6\"><kind/></zeile>").unwrap();
        doc.apply_typed(change.edit).unwrap();
        let ns = doc.node_splice().expect("local");
        assert!(grid.follow(&doc, &ns));
        assert_eq!(grid.columns, [attr("nr"), attr("neu"), child("kind")]);

        while !grid.scan(&doc, 1000) {}
        assert_eq!(
            grid.columns,
            [
                attr("nr"),
                attr("neu"),
                child("kind"),
                attr("größe"),
                child("später")
            ]
        );
        let fresh = Grid::build(&doc, list);
        assert_eq!(grid.rows, fresh.rows);
        let mut a = grid.columns.clone();
        let mut b = fresh.columns.clone();
        a.sort_by_key(|c| format!("{c:?}"));
        b.sort_by_key(|c| format!("{c:?}"));
        assert_eq!(a, b);
    }
}
