//! Agents in the open window (`docs/decisions.md`, D28): the MCP tools of `xml-atlas-mcp` on
//! the tabs, while Settings > Allow AI agents (MCP) is on.
//!
//! The calls arrive through `xml_atlas_mcp::bridge` and run here between two frames, on the
//! window's thread. A change goes through the tab's own edit path, as a menu command's does:
//! tree, grid and text follow, it is one undo step, and what it changed is selected.

use std::path::Path;
use std::sync::Arc;

use xml_atlas_engine::Document;
use xml_atlas_engine::rules_folder::Library;
use xml_atlas_mcp::{Documents, ToolError};

use crate::app::Open;
use crate::tabs::Tabs;

/// The tabs, as the tools see them: each tab under its number.
pub struct WindowDocs<'a> {
    pub docs: &'a mut Tabs<Open>,
    pub next_id: &'a mut u64,
    /// View > Attributes in the tree, for a tab an agent opens.
    pub tree_attributes: bool,
    /// The rules files while Tools > References > Resolve references is on (REF-12), for the same.
    pub references: Option<Arc<Library>>,
}

impl WindowDocs<'_> {
    fn index(&self, id: u64) -> Option<usize> {
        self.docs.position(|o| o.number == id)
    }
}

impl Documents for WindowDocs<'_> {
    /// Shows the file's tab when it is open already; else opens it in a new tab, on this
    /// thread (about a second per 100 MiB, D28).
    fn open(&mut self, path: &Path) -> std::io::Result<u64> {
        let open_already = self.docs.position(|o| {
            o.doc
                .path
                .as_deref()
                .is_some_and(|p| crate::app::same_file(p, path))
        });
        if let Some(i) = open_already {
            self.docs.select(i);
            return Ok(self.docs.get(i).map_or(0, |o| o.number));
        }
        let doc = Document::open(path)?;
        Ok(add_tab(
            self.docs,
            self.next_id,
            self.tree_attributes,
            self.references.as_ref(),
            doc,
        ))
    }

    /// Refused for a tab with unsaved changes: only the user may discard them (D28).
    fn close(&mut self, id: u64) -> Result<(), ToolError> {
        let i = self.index(id).ok_or(ToolError::NoDocument(id))?;
        let modified = self.docs.get(i).is_some_and(|o| o.doc.is_modified());
        if modified {
            return Err(ToolError::Refused(
                "the document has unsaved changes; only the user can discard them".into(),
            ));
        }
        self.docs.close(i);
        Ok(())
    }

    fn ids(&self) -> Vec<u64> {
        self.docs.iter().map(|o| o.number).collect()
    }

    fn get(&self, id: u64) -> Option<&Document> {
        self.docs.get(self.index(id)?).map(|o| &o.doc)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Document> {
        let i = self.index(id)?;
        self.docs.get_mut(i).map(|o| &mut o.doc)
    }

    /// Runs the change through the tab's edit path, shows the tab, and selects the element
    /// the change landed in.
    fn change<R>(&mut self, id: u64, f: impl FnOnce(&mut Document) -> R) -> Option<R> {
        let i = self.index(id)?;
        self.docs.select(i);
        let open = self.docs.get_mut(i)?;
        let before = open.doc.revision();
        let mut out = None;
        let _ = open.edit(|doc| {
            out = Some(f(doc));
            Ok(doc.revision() != before)
        });
        if open.doc.revision() != before
            && let Some(c) = open.doc.last_change()
            && let Some(element) = open.element_at(c.at as u64)
        {
            open.select(element);
        }
        out
    }
}

/// Shows `doc` in a new tab, its root selected, and returns the tab's number.
pub fn add_tab(
    docs: &mut Tabs<Open>,
    next_id: &mut u64,
    tree_attributes: bool,
    references: Option<&Arc<Library>>,
    doc: Document,
) -> u64 {
    *next_id += 1;
    let mut open = Open::new(doc, egui::Id::unique(("document", *next_id)));
    open.number = *next_id;
    open.tree.set_attributes(tree_attributes);
    open.set_references(references);
    if let Some(root) = open.doc.index.root_element() {
        open.select(root);
    }
    docs.push(open);
    *next_id
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn call(w: &mut WindowDocs, tool: &str, a: Value) -> Result<Value, ToolError> {
        xml_atlas_mcp::call(w, tool, a.as_object().unwrap())
    }

    /// An agent's edit shows in the window as the user's would: one undo step, the element
    /// selected, the tab shown; it may save, but not close a tab with unsaved changes.
    #[test]
    fn an_agent_edits_a_tab_as_the_user_would() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-agent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Größe.xml");
        std::fs::write(&path, "\u{feff}<r>\r\n  <a n=\"1\"/>\r\n  <b/>\r\n</r>\r\n").unwrap();
        let mut docs = Tabs::default();
        let mut next_id = 0;
        add_tab(&mut docs, &mut next_id, true, None, Document::untitled());
        let mut w = WindowDocs {
            docs: &mut docs,
            next_id: &mut next_id,
            tree_attributes: true,
            references: Some(Arc::default()),
        };

        let doc = call(&mut w, "open", json!({"path": path})).unwrap()["document"].clone();
        assert_eq!(doc, 2);
        // Opening it again shows the same tab.
        let again = call(&mut w, "open", json!({"path": path})).unwrap();
        assert_eq!(again["document"], 2);
        assert_eq!(w.docs.iter().count(), 2);
        // The agent's tab follows Tools > References > Resolve references (REF-12).
        assert!(w.docs.active().unwrap().resolves_references());

        w.docs.select(0);
        call(
            &mut w,
            "rename",
            json!({"document": doc, "path": "/r/b", "name": "c"}),
        )
        .unwrap();
        let open = w.docs.active().unwrap();
        assert_eq!(open.number, 2, "the changed tab is shown");
        let selected = open.selected.unwrap();
        assert_eq!(open.doc.index.path(&open.doc.src, selected), "/r/c");
        assert_eq!(open.doc.history.undo_label(), Some("Rename <b> to <c>"));
        // XPath reads the tab as it is now, after the rename (D35, XP-7).
        let found = call(&mut w, "xpath", json!({"document": doc, "expr": "/r/*"})).unwrap();
        assert_eq!(found["total"], 2);
        assert_eq!(found["nodes"][1]["path"], "/r/c");

        let refused = call(&mut w, "close", json!({"document": doc})).unwrap_err();
        assert!(refused.to_string().contains("unsaved"), "{refused}");
        call(&mut w, "save", json!({"document": doc})).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "\u{feff}<r>\r\n  <a n=\"1\"/>\r\n  <c/>\r\n</r>\r\n"
        );
        call(&mut w, "close", json!({"document": doc})).unwrap();
        assert_eq!(w.ids(), [1]);
    }
}
