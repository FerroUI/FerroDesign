//! Explicit tree slots are independent of canvas hit testing and clipping.
use crate::*;
use std::collections::{BTreeSet, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerRowSlot {
    Above,
    Inside,
    Below,
}

impl Node {
    /// In the Layers panel free artwork reads front to back, automatic
    /// layouts read in flow order. Children are stored back to front.
    pub fn layer_rows_follow_child_order(&self) -> bool {
        self.layout.as_ref().is_some_and(|layout| !layout.reverse)
    }
}

#[derive(Debug)]
pub struct LayerTreeMove {
    token: EditorRenderToken,
    page: NodeId,
    original_selection: Vec<NodeId>,
    selection: Vec<NodeId>,
    roots: Vec<NodeId>, // top to bottom in the panel, independent of selection order
    root_set: HashSet<NodeId>,
}

fn editable_parent(document: &Document, mut id: NodeId) -> bool {
    while let Some(node) = document.nodes.get(&id) {
        if node.locked || node.kind == NodeKind::Instance || document.is_component_set(id) {
            return false;
        }
        let Some(parent) = node.parent else {
            return false;
        };
        id = parent;
    }
    document.page(id).is_some()
}

impl LayerTreeMove {
    pub fn new(editor: &Editor, source: NodeId) -> Option<Self> {
        if editor.has_active_transaction()
            || editor.document.page_of(source) != Some(editor.active_page)
        {
            return None;
        }
        let selection = if editor.selection.contains(&source) {
            editor.selection.clone()
        } else {
            vec![source]
        };
        let doc = &editor.document;
        let root_set: HashSet<_> = editor.root_selection_for(&selection).into_iter().collect();
        if !root_set.contains(&source) && !root_set.iter().any(|&id| doc.is_ancestor(id, source)) {
            return None;
        }
        if root_set.iter().any(|id| {
            !doc.nodes
                .get(id)
                .and_then(|n| n.parent)
                .is_some_and(|parent| editable_parent(doc, parent))
        }) {
            return None;
        }
        let mut roots = Vec::with_capacity(root_set.len());
        let mut pending = doc.page(editor.active_page)?.children.clone();
        while let Some(id) = pending.pop() {
            if root_set.contains(&id) {
                roots.push(id);
            } else {
                let n = doc.nodes.get(&id)?;
                if n.layer_rows_follow_child_order() {
                    pending.extend(n.children.iter().rev());
                } else {
                    pending.extend(&n.children);
                }
            }
            if roots.len() == root_set.len() {
                break;
            }
        }
        if roots.is_empty() || roots.len() != root_set.len() {
            return None;
        }
        Some(Self {
            token: editor.render_change_token(),
            page: editor.active_page,
            original_selection: editor.selection.clone(),
            selection,
            roots,
            root_set,
        })
    }

    pub fn target_parent(
        &self,
        editor: &Editor,
        row: NodeId,
        slot: LayerRowSlot,
    ) -> Option<NodeId> {
        let doc = &editor.document;
        if editor.active_page != self.page
            || self.token != editor.render_change_token()
            || self.root_set.contains(&row)
            || doc.page_of(row) != Some(self.page)
        {
            return None;
        }
        let node = doc.nodes.get(&row)?;
        let parent = if slot == LayerRowSlot::Inside {
            if !node.kind.is_container() || node.kind == NodeKind::Boolean {
                return None;
            }
            row
        } else {
            node.parent?
        };
        if !editable_parent(doc, parent) {
            return None;
        }
        let mut ancestor = Some(parent);
        while let Some(id) = ancestor {
            if self.root_set.contains(&id) {
                return None;
            }
            ancestor = doc.nodes.get(&id).and_then(|n| n.parent);
        }
        Some(parent)
    }

    /// Return whether the sibling arrays changed. Failed or stationary drops
    /// leave the document, selection and both history stacks untouched.
    pub fn commit(
        &self,
        editor: &mut Editor,
        row: NodeId,
        slot: LayerRowSlot,
    ) -> Result<bool, DocumentError> {
        let invalid = || {
            DocumentError::Invalid("Choose an editable layer outside the moving selection.".into())
        };
        if editor.selection != self.original_selection || editor.has_active_transaction() {
            return Err(invalid());
        }
        let parent = self.target_parent(editor, row, slot).ok_or_else(invalid)?;
        let doc = &editor.document;
        let old = doc
            .children(parent)
            .ok_or(DocumentError::InvalidParent(parent))?;
        let forward = doc
            .nodes
            .get(&parent)
            .is_some_and(Node::layer_rows_follow_child_order);
        let mut order: Vec<_> = old
            .iter()
            .copied()
            .filter(|id| !self.root_set.contains(id))
            .collect();
        let index = if slot == LayerRowSlot::Inside {
            if forward {
                0
            } else {
                order.len()
            }
        } else {
            order.iter().position(|id| *id == row).ok_or_else(invalid)?
                + usize::from((slot == LayerRowSlot::Below) == forward)
        };
        let mut insert = self.roots.clone();
        if !forward {
            insert.reverse();
        }
        order.splice(index..index, insert);
        if order == old {
            return Ok(false);
        }
        let mut candidate = doc.clone();
        let mut former_parents = BTreeSet::new();
        for &id in &self.roots {
            let node = &doc.nodes[&id];
            if node.parent == Some(parent) {
                continue;
            }
            let old_parent = node.parent.ok_or_else(invalid)?;
            former_parents.insert(old_parent);
            let local = doc
                .world_transform(parent)
                .and_then(Transform::inverse)
                .and_then(|inv| doc.world_transform(id).map(|world| inv * world))
                .ok_or_else(invalid)?;
            let moved = candidate.nodes.get_mut(&id).unwrap();
            moved.set_local_transform(local);
            moved.parent = Some(parent);
            if let Some(grid) = &mut moved.layout_child.grid {
                grid.row = None;
                grid.column = None;
            }
        }
        for &id in &former_parents {
            candidate
                .children_mut(id)
                .unwrap()
                .retain(|child| !self.root_set.contains(child));
        }
        *candidate.children_mut(parent).unwrap() = order;
        candidate.repair_component_references();
        for id in former_parents.into_iter().chain([parent]) {
            if candidate.nodes.get(&id).is_some_and(|n| n.layout.is_some()) {
                candidate.validate_layout_node_settings(id)?;
            }
        }
        editor.commit_layout_with_selection("Move layers", candidate, Some(&self.selection))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_drop_reparents_in_panel_order_preserves_affine_pose_and_undo_selection() {
        let mut doc = Document::new("Hierarchy");
        let page = doc.pages[0].id;
        let add = |d: &mut Document, p, kind, name| {
            d.insert_node(p, Node::new(kind, name, 12., 14., 40., 50.))
                .unwrap()
        };
        let frame = add(&mut doc, page, NodeKind::Group, "Receiving group");
        doc.nodes.get_mut(&frame).unwrap().rotation = 41.;
        let first = add(&mut doc, page, NodeKind::Rectangle, "Back");
        let second = add(&mut doc, page, NodeKind::Frame, "Front");
        let nested = add(&mut doc, second, NodeKind::Ellipse, "Nested");
        let mut e = Editor::new(doc);
        e.selection = vec![second, nested, first];
        let before = e.document.clone();
        let pose = before
            .world_transform(nested)
            .unwrap()
            .apply(Point::new(3., 9.));
        let plan = LayerTreeMove::new(&e, second).unwrap();
        assert!(plan
            .target_parent(&e, nested, LayerRowSlot::Above)
            .is_none());
        assert!(plan.commit(&mut e, frame, LayerRowSlot::Inside).unwrap());
        assert_eq!(e.document.nodes[&frame].children, [first, second]);
        assert_eq!(e.document.nodes[&nested].parent, Some(second));
        assert!(
            e.document
                .world_transform(nested)
                .unwrap()
                .apply(Point::new(3., 9.))
                .distance(pose)
                < 1e-8
        );
        assert_eq!(e.history_len(), 1);
        let after = e.document.clone();
        assert!(e.undo());
        assert_eq!(e.document.to_json().unwrap(), before.to_json().unwrap());
        assert!(plan.commit(&mut e, frame, LayerRowSlot::Inside).is_err());
        assert!(e.redo());
        assert_eq!(e.document.to_json().unwrap(), after.to_json().unwrap());
        let again = LayerTreeMove::new(&e, second).unwrap();
        assert!(!again.commit(&mut e, frame, LayerRowSlot::Inside).unwrap());
        assert_eq!(e.history_len(), 1);
    }

    #[test]
    fn tree_drop_preserves_hidden_sources_and_rejects_locked_parents() {
        let mut doc = Document::new("Layer staging");
        let page = doc.pages[0].id;
        let mut hidden = Node::new(NodeKind::Rectangle, "Hidden", 5., 6., 10., 10.);
        hidden.visible = false;
        let source = doc.insert_node(page, hidden).unwrap();
        let anchor = doc
            .insert_node(
                page,
                Node::new(NodeKind::Rectangle, "Anchor", 0., 0., 10., 10.),
            )
            .unwrap();
        let frame = doc
            .insert_node(page, Node::new(NodeKind::Frame, "Locked", 0., 0., 30., 30.))
            .unwrap();
        doc.nodes.get_mut(&frame).unwrap().locked = true;
        let mut e = Editor::new(doc);
        e.selection = vec![anchor];
        let plan = LayerTreeMove::new(&e, source).unwrap();
        assert!(plan
            .target_parent(&e, frame, LayerRowSlot::Inside)
            .is_none());
        assert!(LayerTreeMove::new(&e, frame).is_none());
        assert!(plan.commit(&mut e, anchor, LayerRowSlot::Above).unwrap());
        assert_eq!(e.selection, [source]);
        assert_eq!(
            e.document.page(page).unwrap().children,
            [anchor, source, frame]
        );
        assert!(!e.document.nodes[&source].visible);
        assert!(e.undo());
        assert_eq!(e.selection, [anchor]);
    }
}
