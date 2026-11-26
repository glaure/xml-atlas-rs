//! The cells a view of a [`Grid`] has on screen, kept from frame to frame (`docs/spec/grid.md`,
//! GRID-9; AGENTS.md, rule 2).
//!
//! A row is worked out when it comes on screen, with one walk over its children for all its
//! columns, and again only when the document's nodes or the columns change. Rows that leave
//! the screen are dropped, so this holds a screenful, not the file.

use std::borrow::Cow;
use std::collections::HashMap;

use xml_atlas_xml::{NodeId, NodeKind};

use crate::command::holds_only_text;
use crate::grid::{Cell, Column, Grid, attribute_cell, child_cell, text};
use crate::{Document, NodesVersion};

/// A cell, and what a view asks of it in every frame it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub cell: Cell,
    /// The node it stands for, as [`Grid::cell_node`] gives it.
    pub node: Option<NodeId>,
    /// Whether [`Grid::edit_target`] has something to edit.
    pub editable: bool,
}

/// The cells of the rows on screen.
#[derive(Debug, Default)]
pub struct Cells {
    version: Option<NodesVersion>,
    columns: Vec<Column>,
    lookup: Lookup,
    rows: HashMap<NodeId, Row>,
    frame: u64,
    worked_out: u64,
}

#[derive(Debug)]
struct Row {
    cells: Vec<Shown>,
    /// The last frame that showed it.
    frame: u64,
}

impl Cells {
    /// Starts a frame that shows `grid`; what no longer holds is dropped.
    pub fn begin(&mut self, doc: &Document, grid: &Grid) {
        self.frame += 1;
        let version = doc.nodes_version();
        if self.columns != grid.columns {
            self.columns.clone_from(&grid.columns);
            self.lookup = Lookup::new(&self.columns);
            self.rows.clear();
        }
        if self.version != Some(version) {
            self.version = Some(version);
            self.rows.clear();
        }
    }

    /// The cells of `row`, one for each of the grid's columns, hidden ones included.
    pub fn row(&mut self, doc: &Document, row: NodeId) -> &[Shown] {
        let frame = self.frame;
        let kept = self.rows.entry(row).or_insert_with(|| {
            self.worked_out += 1;
            Row {
                cells: row_cells(doc, &self.columns, &self.lookup, row),
                frame,
            }
        });
        kept.frame = frame;
        &kept.cells
    }

    /// Ends the frame: rows it did not show are dropped.
    pub fn end(&mut self) {
        let frame = self.frame;
        self.rows.retain(|_, row| row.frame == frame);
    }

    /// How often a row was worked out, for tests: an idle view adds none.
    pub fn worked_out(&self) -> u64 {
        self.worked_out
    }
}

/// Where each attribute and child column is, by name, so a row finds its columns with one
/// lookup for each attribute and child it has, not one for each column.
#[derive(Debug, Default)]
struct Lookup {
    attributes: HashMap<String, usize>,
    children: HashMap<Vec<u8>, usize>,
}

impl Lookup {
    fn new(columns: &[Column]) -> Self {
        let mut lookup = Lookup::default();
        for (i, column) in columns.iter().enumerate() {
            match column {
                Column::Attribute(name) => {
                    lookup.attributes.entry(name.clone()).or_insert(i);
                }
                Column::Child(name) => {
                    lookup.children.entry(name.as_bytes().to_vec()).or_insert(i);
                }
                Column::Text => {}
            }
        }
        lookup
    }
}

/// The cells of `row` in `columns`, as [`Grid::cell`] gives them, with one walk over the
/// row's children and attributes: not one for each column.
fn row_cells(doc: &Document, columns: &[Column], lookup: &Lookup, row: NodeId) -> Vec<Shown> {
    let idx = &doc.index;
    let src = &doc.src[..];

    // Per child column: the first such child and how many there are.
    let mut children: Vec<(Option<NodeId>, usize)> = Vec::new();
    if !lookup.children.is_empty() {
        children.resize(columns.len(), (None, 0));
    }
    let mut only_text = true;
    for c in idx.children(row) {
        let kind = idx.node(c).kind;
        only_text &= matches!(kind, NodeKind::Text | NodeKind::CData);
        if kind == NodeKind::Element
            && let Some(&at) = lookup.children.get(idx.name(src, c))
        {
            let (first, count) = &mut children[at];
            first.get_or_insert(c);
            *count += 1;
        }
    }

    // Raw: a cell decodes only the start it shows. Per attribute column.
    let mut attributes: Vec<Option<Cow<'_, [u8]>>> = Vec::new();
    if !lookup.attributes.is_empty() {
        attributes.resize(columns.len(), None);
        for (name, value) in idx.attribute_parts(src, row) {
            if let Some(&at) = lookup.attributes.get(&*String::from_utf8_lossy(&name)) {
                // The first of two with one name, as `Grid::cell` finds.
                attributes[at].get_or_insert(value);
            }
        }
    }

    columns
        .iter()
        .enumerate()
        .map(|(i, column)| match column {
            Column::Attribute(_) => {
                let value = attributes[i].as_ref();
                Shown {
                    node: value.map(|_| row),
                    cell: value.map_or(Cell::Absent, |raw| Cell::Value(attribute_cell(raw))),
                    editable: true,
                }
            }
            Column::Child(name) => {
                let (node, count) = children[i];
                let cell = child_cell(doc, name, node, count);
                let editable = matches!(cell, Cell::Value(_))
                    && node.is_some_and(|child| holds_only_text(doc, child));
                Shown {
                    cell,
                    node,
                    editable,
                }
            }
            Column::Text => {
                let has = idx.has_text(row);
                Shown {
                    cell: if has {
                        Cell::Value(text(doc, row))
                    } else {
                        Cell::Absent
                    },
                    node: has.then_some(row),
                    editable: only_text,
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Edit;
    use crate::grid::CELL_CHARS;

    fn doc(src: &str) -> Document {
        Document::from_bytes(src.as_bytes().to_vec())
    }

    /// One frame showing `rows` of `grid`.
    fn frame(cells: &mut Cells, doc: &Document, grid: &Grid, rows: &[NodeId]) {
        cells.begin(doc, grid);
        for &row in rows {
            cells.row(doc, row);
        }
        cells.end();
    }

    /// What is kept is what the commands work out, for every element taken as a grid.
    #[test]
    fn kept_cells_are_the_grids_own() {
        let long = "ü".repeat(CELL_CHARS + 5);
        for src in [
            "\u{feff}<r a=\"1\">\r\n  <o id=\"&lt;7&gt;\" größe=\"\"><name>Zoë</name><total c=\"EUR\">12</total>\
             <n>a</n><n>b</n><x/></o>\r\n  <o><name></name><mix>a<b/>c</mix><note>t<!--c--></note></o>\r\n  \
             <o>own<![CDATA[ text]]></o>\r\n  <o><!--only--></o>\r\n</r>"
                .to_owned(),
            format!("<r><o v=\"{long}\"><t>{long}</t></o><o>plain</o></r>"),
            "<r><o a=\"1\" a=\"2\"><k>1</k></o><o/></r>".to_owned(),
            "<r><broken><k>1</k>".to_owned(),
        ] {
            let d = doc(&src);
            for parent in 0..d.index.len() as NodeId {
                if d.index.node(parent).kind != NodeKind::Element {
                    continue;
                }
                let grid = Grid::build(&d, parent);
                let mut cells = Cells::default();
                cells.begin(&d, &grid);
                for &row in &grid.rows {
                    let kept = cells.row(&d, row);
                    assert_eq!(kept.len(), grid.columns.len());
                    for (column, shown) in grid.columns.iter().zip(kept) {
                        let at = format!("{src}: row {row}, {column:?}");
                        assert_eq!(shown.cell, grid.cell(&d, row, column), "{at}");
                        assert_eq!(shown.node, grid.cell_node(&d, row, column), "{at}");
                        let editable = grid.edit_target(&d, row, column).is_some();
                        assert_eq!(shown.editable, editable, "{at}");
                    }
                }
            }
        }
    }

    /// AGENTS.md, rule 2: an idle frame works out nothing, and only a screenful is kept.
    #[test]
    fn only_rows_coming_on_screen_are_worked_out() {
        let d = doc(&format!("<r>{}</r>", "<o><k>1</k></o>".repeat(500)));
        let grid = Grid::build(&d, d.index.root_element().unwrap());
        let mut cells = Cells::default();
        for _ in 0..10 {
            frame(&mut cells, &d, &grid, &grid.rows[..30]);
        }
        assert_eq!(cells.worked_out(), 30);
        frame(&mut cells, &d, &grid, &grid.rows[1..31]);
        assert_eq!(cells.worked_out(), 31, "scrolled by one");
        for top in 2..470 {
            frame(&mut cells, &d, &grid, &grid.rows[top..top + 30]);
        }
        assert_eq!(cells.rows.len(), 30);
    }

    #[test]
    fn an_edit_and_new_columns_work_the_rows_out_again() {
        let src = "<r><o><k>1</k></o></r>";
        let mut d = doc(src);
        let root = d.index.root_element().unwrap();
        let mut grid = Grid::build(&d, root);
        let mut cells = Cells::default();
        cells.begin(&d, &grid);
        assert_eq!(cells.row(&d, grid.rows[0])[0].cell, Cell::Value("1".into()));
        cells.end();

        let at = src.find('1').unwrap();
        d.apply(Edit::new(crate::StepName::Other("Type".into())).replace(at..at + 1, "Größe"))
            .unwrap();
        cells.begin(&d, &grid);
        assert_eq!(
            cells.row(&d, grid.rows[0])[0].cell,
            Cell::Value("Größe".into())
        );
        cells.end();

        // Columns found later (GRID-3).
        grid.columns.push(Column::Text);
        cells.begin(&d, &grid);
        assert_eq!(cells.row(&d, grid.rows[0]).len(), 2);
    }

    /// Reading again after typing changes the nodes and not the revision (TEXT-22).
    #[test]
    fn reading_again_works_the_rows_out_again() {
        let src = "<r><o><k></k></o></r>";
        let mut d = doc(src);
        let grid = Grid::build(&d, d.index.root_element().unwrap());
        let at = src.find("</k>").unwrap();
        let part = "<![CDATA[hi";
        d.apply_typed(Edit::new(crate::StepName::Other("Type".into())).replace(at..at, part))
            .unwrap();
        let end = at + part.len();
        d.apply_typed(Edit::new(crate::StepName::Other("Type".into())).replace(end..end, "]]>"))
            .unwrap();
        assert!(d.stale().is_some());
        let mut cells = Cells::default();
        cells.begin(&d, &grid);
        assert_eq!(
            cells.row(&d, grid.rows[0])[0].cell,
            Cell::Value(String::new())
        );
        cells.end();
        let revision = d.revision();
        assert!(d.reparse());
        assert_eq!(d.revision(), revision);
        cells.begin(&d, &grid);
        assert_eq!(
            cells.row(&d, grid.rows[0])[0].cell,
            Cell::Value("hi".into())
        );
    }
}
