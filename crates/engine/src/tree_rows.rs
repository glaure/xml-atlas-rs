//! What the tree shows for the rows on screen, kept from frame to frame
//! (`docs/spec/tree.md`, TREE-7; AGENTS.md, rule 2).
//!
//! A row is worked out when it comes on screen, and again only when the document's nodes or
//! the options change. Rows that leave the screen are dropped, so this holds a screenful,
//! not the file.

use std::collections::HashMap;

use crate::tree::{self, AttributeWalk, Label, Row, RowKind, TreeOptions};
use crate::{Document, NodesVersion};

/// What a row shows, and what a view asks of it in every frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    /// [`Row::depth`].
    pub depth: u16,
    /// [`tree::has_children`].
    pub expandable: bool,
    /// [`tree::kind`].
    pub kind: RowKind,
    /// [`tree::label`].
    pub label: Label,
}

/// The rows on screen.
#[derive(Debug, Default)]
pub struct Rows {
    state: Option<(NodesVersion, TreeOptions)>,
    rows: Vec<Row>,
    shown: Vec<Shown>,
    worked_out: u64,
}

impl Rows {
    /// What `rows`, the ones on screen in tree order, show: one for each, in their order.
    pub fn show(&mut self, doc: &Document, options: TreeOptions, rows: &[Row]) -> &[Shown] {
        let state = Some((doc.nodes_version(), options));
        if self.state != state {
            self.state = state;
            self.rows.clear();
            self.shown.clear();
        } else if self.rows == rows {
            return &self.shown;
        }
        let mut kept: HashMap<Row, Shown> = self.rows.drain(..).zip(self.shown.drain(..)).collect();
        // An element's attribute rows follow one another: one walk along its start tag.
        let mut walk = AttributeWalk::default();
        for &row in rows {
            let shown = kept.remove(&row).unwrap_or_else(|| {
                self.worked_out += 1;
                work_out(doc, options, row, &mut walk)
            });
            self.rows.push(row);
            self.shown.push(shown);
        }
        &self.shown
    }

    /// How often a row was worked out, for tests: an idle view adds none.
    pub fn worked_out(&self) -> u64 {
        self.worked_out
    }
}

fn work_out<'d>(
    doc: &'d Document,
    options: TreeOptions,
    row: Row,
    walk: &mut AttributeWalk<'d>,
) -> Shown {
    let kind = tree::kind(doc, row);
    Shown {
        depth: row.depth(doc),
        // With the kind, so that the children are walked once.
        expandable: tree::expandable(doc, row, kind, options),
        kind,
        label: tree::label_in(doc, row, walk),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Edit;

    const OPTIONS: TreeOptions = TreeOptions { attributes: true };

    fn doc(src: &str) -> Document {
        Document::from_bytes(src.as_bytes().to_vec())
    }

    fn under_root(doc: &Document) -> Vec<Row> {
        tree::children(doc, doc.index.root_element().unwrap(), OPTIONS)
    }

    /// AGENTS.md, rule 2: an idle frame works out nothing, and only a screenful is kept.
    #[test]
    fn only_rows_coming_on_screen_are_worked_out() {
        let d = doc(&format!("<r>{}</r>", "<o><k>1</k></o>".repeat(500)));
        let all = under_root(&d);
        let mut rows = Rows::default();
        for _ in 0..10 {
            rows.show(&d, OPTIONS, &all[..30]);
        }
        assert_eq!(rows.worked_out(), 30);
        rows.show(&d, OPTIONS, &all[1..31]);
        assert_eq!(rows.worked_out(), 31, "scrolled by one");
        for top in 2..470 {
            rows.show(&d, OPTIONS, &all[top..top + 30]);
        }
        assert_eq!((rows.rows.len(), rows.shown.len()), (30, 30));
    }

    /// The one walk along a start tag also serves rows that begin in its middle, that skip
    /// some, and those of the next element.
    #[test]
    fn one_walk_gives_each_attribute_its_own_label() {
        let attributes: String = (0..50).map(|i| format!(" a{i}=\"Größe {i}\"")).collect();
        let inner = attributes.replace("Größe", "Innen");
        let d = doc(&format!("\u{feff}<r{attributes}>\r\n<e{inner}/></r>"));
        let root = d.index.root_element().unwrap();
        let mut all = under_root(&d);
        let inner = all.last().unwrap().id();
        all.extend(tree::children(&d, inner, OPTIONS));
        assert_eq!(all.len(), 101);
        let mut rows = Rows::default();
        for (from, to) in [(20, 60), (10, 60), (10, 90), (0, 101), (7, 8)] {
            for (row, shown) in all[from..to]
                .iter()
                .zip(rows.show(&d, OPTIONS, &all[from..to]))
            {
                assert_eq!(shown.label, tree::label(&d, *row), "{row:?} of {root}");
            }
        }
        // From one element straight to a later attribute of the next, as when the rows
        // between them are kept.
        let apart = [all[3], all[51 + 10]];
        for (row, shown) in apart.iter().zip(Rows::default().show(&d, OPTIONS, &apart)) {
            assert_eq!(shown.label, tree::label(&d, *row), "{row:?} apart");
        }
        // Not in tree order either.
        let turned: Vec<Row> = all.iter().rev().copied().collect();
        for (row, shown) in turned
            .iter()
            .zip(Rows::default().show(&d, OPTIONS, &turned))
        {
            assert_eq!(shown.label, tree::label(&d, *row), "{row:?} turned");
        }
    }

    #[test]
    fn an_edit_and_other_options_work_the_rows_out_again() {
        let src = "<r><o a=\"1\"/></r>";
        let mut d = doc(src);
        let all = under_root(&d);
        let mut rows = Rows::default();
        let shown = &rows.show(&d, OPTIONS, &all)[0];
        assert!(shown.expandable);
        let hidden = TreeOptions { attributes: false };
        assert!(!rows.show(&d, hidden, &all)[0].expandable);

        let at = src.find('1').unwrap();
        d.apply(Edit::new(crate::StepName::Other("Type".into())).replace(at..at + 1, "Größe"))
            .unwrap();
        assert_eq!(
            rows.show(&d, hidden, &all)[0].label,
            Label::Element {
                name: "o".into(),
                hint: Some("a=\"Größe\"".into())
            }
        );
    }

    /// Reading again after typing changes the nodes and not the revision (TEXT-22).
    #[test]
    fn reading_again_works_the_rows_out_again() {
        let src = "<r><k></k></r>";
        let mut d = doc(src);
        let all = under_root(&d);
        let at = src.find("</k>").unwrap();
        let part = "<![CDATA[hi";
        d.apply_typed(Edit::new(crate::StepName::Other("Type".into())).replace(at..at, part))
            .unwrap();
        let end = at + part.len();
        d.apply_typed(Edit::new(crate::StepName::Other("Type".into())).replace(end..end, "]]>"))
            .unwrap();
        assert!(d.stale().is_some());
        let mut rows = Rows::default();
        assert_eq!(rows.show(&d, OPTIONS, &all)[0].kind, RowKind::Empty);
        let revision = d.revision();
        assert!(d.reparse());
        assert_eq!(d.revision(), revision);
        assert_eq!(rows.show(&d, OPTIONS, &all)[0].kind, RowKind::TextElement);
    }
}
