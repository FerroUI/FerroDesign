//! Page duplication remaps all of its roots together. Shared styles, variables
//! and references to layers outside that page retain their original identities.
use crate::*;
use std::collections::BTreeMap;

impl Node {
    pub(crate) fn clone_for_copy(&self, id: NodeId, ids: &BTreeMap<NodeId, NodeId>) -> Node {
        let mut copy = self.clone();
        copy.id = id;
        copy.revision = 0;
        copy.parent = self.parent.and_then(|id| ids.get(&id).copied());
        copy.children = self
            .children
            .iter()
            .filter_map(|id| ids.get(id).copied())
            .collect();
        for interaction in &mut copy.interactions {
            match &mut interaction.action {
                PrototypeAction::NavigateTo { target }
                | PrototypeAction::OpenOverlay { target, .. }
                | PrototypeAction::ScrollTo { target } => {
                    if let Some(mapped) = ids.get(target) {
                        *target = *mapped;
                    }
                }
                _ => {}
            }
        }
        copy
    }
}

impl Editor {
    pub fn move_page_to(&mut self, from: usize, to: usize) -> bool {
        let len = self.document.pages.len();
        if self.has_active_transaction() || from >= len || to >= len || from == to {
            return false;
        }
        self.transact("Reorder pages", |d| {
            let page = d.pages.remove(from);
            d.pages.insert(to, page);
        });
        true
    }

    pub fn duplicate_active_page(&mut self) -> Result<NodeId, DocumentError> {
        if self.has_active_transaction() {
            return Err(DocumentError::Invalid(
                "Finish the current edit before duplicating a page.".into(),
            ));
        }
        let source = self.active_page;
        let doc = &self.document;
        let index = doc
            .pages
            .iter()
            .position(|p| p.id == source)
            .ok_or(DocumentError::MissingNode(source))?;
        let ordered = doc.draw_order(source);
        if doc.nodes.len() + ordered.len() > MAX_DOCUMENT_NODES {
            return Err(DocumentError::Invalid(
                "Duplicating this page would exceed the layer limit.".into(),
            ));
        }

        // Reserve a contiguous range in one pass. Neither exhaustion nor the
        // node budget should leave a partial page or consume an Undo step.
        let last = doc
            .nodes
            .keys()
            .next_back()
            .into_iter()
            .chain(doc.styles.keys().next_back())
            .chain(doc.variables.keys().next_back())
            .copied()
            .chain(doc.pages.iter().map(|p| p.id))
            .chain(doc.comments.iter().map(|c| c.id))
            .max()
            .unwrap_or(0);
        let exhausted = || DocumentError::Invalid("ID space exhausted".into());
        let copy_id = doc.next_id.max(last.checked_add(1).ok_or_else(exhausted)?);
        let next_id = copy_id
            .checked_add(1 + ordered.len() as u64)
            .ok_or_else(exhausted)?;
        let ids: BTreeMap<_, _> = std::iter::once(source)
            .chain(ordered.iter().copied())
            .zip(copy_id..next_id)
            .collect();

        Ok(self.edit("Duplicate page", |e| {
            let d = &mut e.document;
            let mut page = d.pages[index].clone();
            page.id = copy_id;
            page.name = format!("Copy of {}", page.name);
            page.children = page.children.iter().map(|id| ids[id]).collect();
            page.prototype_start = page.prototype_start.and_then(|id| ids.get(&id).copied());
            for old in ordered {
                let mut node = d.nodes[&old].clone_for_copy(ids[&old], &ids);
                crate::components::remap_component_references(&mut node, &ids);
                d.nodes.insert(node.id, node);
            }
            for (&old, &new) in &ids {
                if let Some(guides) = d.guides.get(&old).cloned() {
                    d.guides.insert(new, guides);
                }
            }
            d.pages.insert(index + 1, page);
            d.next_id = next_id;
            e.active_page = copy_id;
            e.selection.clear();
            copy_id
        }))
    }

    pub fn delete_active_page(&mut self) -> bool {
        if self.has_active_transaction() || self.document.pages.len() < 2 {
            return false;
        }
        let page = self.active_page;
        let Some(index) = self.document.pages.iter().position(|p| p.id == page) else {
            return false;
        };
        self.edit("Delete page", |e| {
            let d = &mut e.document;
            let mut ids = d.draw_order(page);
            ids.push(page);
            // Unanchored comments from older files belong to the first page.
            if index == 0 {
                d.comments.retain(|thread| thread.node.is_some());
            }
            d.pages.remove(index);
            d.remove_nodes_and_references(ids);
            e.active_page = d.pages[index.min(d.pages.len() - 1)].id;
            e.selection.clear();
            true
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(target: NodeId) -> Interaction {
        Interaction {
            trigger: Trigger::Click,
            action: PrototypeAction::NavigateTo { target },
            transition: Transition::default(),
            delay_ms: 0,
            key: None,
        }
    }

    #[test]
    fn copying_a_page_remaps_internal_artwork_preserves_external_references_and_leaves_review_on_source(
    ) {
        let mut d = Document::new("Pages");
        let src = d.pages[0].id;
        let other = d.add_page("Library");
        let shared = d
            .insert_node(
                other,
                Node::new(NodeKind::Component, "Shared", 0., 0., 80., 60.),
            )
            .unwrap();
        let card = d
            .insert_node(
                src,
                Node::new(NodeKind::Component, "Card", 54., -30., 100., 60.),
            )
            .unwrap();
        let nested = d
            .insert_node(
                card,
                Node::new(NodeKind::Rectangle, "Body", 2., 4., 20., 33.),
            )
            .unwrap();
        d.nodes.get_mut(&card).unwrap().rotation = 28.;
        d.nodes.get_mut(&card).unwrap().flip_x = true;
        let style = d.allocate_id();
        d.styles.insert(
            style,
            Style {
                id: style,
                name: "Surface".into(),
                description: String::new(),
                value: StyleValue::Fill {
                    fills: vec![Fill::solid(Color::WHITE)],
                },
            },
        );
        let child = d.nodes.get_mut(&nested).unwrap();
        child.fill_style = Some(style);
        child.locked = true;
        let own = d.create_component_instance(card, src, 310., 20.).unwrap();
        let imported = d.create_component_instance(shared, src, -80., 40.).unwrap();
        let external = d.create_component_instance(card, other, 15., 20.).unwrap();
        d.nodes.get_mut(&card).unwrap().interactions = vec![link(own), link(shared)];
        d.nodes.get_mut(&external).unwrap().interactions = vec![link(card)];
        d.pages[0].background = Color::rgb(45, 78, 10);
        d.pages[0].prototype_start = Some(card);
        let guide = Guide {
            horizontal: false,
            position: 23.,
        };
        d.guides.insert(card, vec![guide]);
        d.guides.insert(src, vec![guide]);
        for node in [Some(src), Some(nested), None, Some(other)] {
            let id = d.allocate_id();
            d.comments.push(CommentThread {
                id,
                node,
                position: Point::new(15., 30.),
                resolved: false,
                messages: vec![],
            });
        }
        let mut e = Editor::new(d);
        e.selection = vec![own];
        let original = e.document.to_json().unwrap();
        let copy = e.duplicate_active_page().unwrap();
        let cp = &e.document.pages[1];
        assert_eq!(
            (cp.id, cp.name.as_str(), cp.background),
            (copy, "Copy of Page 1", e.document.pages[0].background)
        );
        let (a, b, c) = (cp.children[0], cp.children[1], cp.children[2]);
        assert!(a != card && b != own && c != imported);
        assert_eq!(cp.prototype_start, Some(a));
        assert_eq!(e.document.nodes[&b].component_id, Some(a));
        let new_nested = e.document.nodes[&a].children[0];
        assert!(e.document.nodes[&new_nested].locked);
        assert_eq!(e.document.nodes[&new_nested].fill_style, Some(style));
        assert_eq!(
            e.document.nodes[&e.document.nodes[&b].children[0]].component_source,
            Some(new_nested)
        );
        assert_eq!(e.document.nodes[&c].component_id, Some(shared));
        assert_eq!(e.document.nodes[&a].interactions, [link(b), link(shared)]);
        assert_eq!(e.document.nodes[&external].interactions, [link(card)]);
        assert_eq!(e.document.guides[&a], [guide]);
        assert_eq!(e.document.guides[&copy], [guide]);
        assert_eq!(e.document.comments.len(), 4);
        assert_eq!(
            e.document.world_transform(a),
            e.document.world_transform(card)
        );
        assert_eq!(e.active_page, copy);
        assert!(e.selection.is_empty());
        assert!(e.document.validate().is_ok());
        let copied = e.document.to_json().unwrap();
        assert!(e.undo());
        assert_eq!(e.active_page, src);
        assert_eq!(e.selection, [own]);
        assert_eq!(e.document.to_json().unwrap(), original);
        assert!(e.redo());
        assert_eq!(e.active_page, copy);
        assert_eq!(e.document.to_json().unwrap(), copied);

        assert!(e.set_active_page(src));
        e.selection = vec![own];
        assert!(e.delete_active_page());
        assert_eq!(e.active_page, copy);
        assert_eq!(e.document.nodes[&external].kind, NodeKind::Frame);
        assert!(e.document.nodes[&external].interactions.is_empty());
        assert_eq!(e.document.nodes[&b].component_id, Some(a));
        assert_eq!(e.document.comments.len(), 1);
        assert_eq!(e.document.comments[0].node, Some(other));
        assert!(!e.document.guides.contains_key(&src) && !e.document.guides.contains_key(&card));
        assert!(e.document.validate().is_ok());
        let deleted = e.document.to_json().unwrap();
        assert!(e.undo());
        assert_eq!(e.active_page, src);
        assert_eq!(e.selection, [own]);
        assert_eq!(e.document.to_json().unwrap(), copied);
        assert!(e.redo());
        assert_eq!(e.document.to_json().unwrap(), deleted);
    }

    #[test]
    fn page_commands_reject_the_last_page_an_open_gesture_and_id_exhaustion_atomically() {
        let mut d = Document::new("Last page");
        d.next_id = u64::MAX;
        let mut e = Editor::new(d);
        assert!(!e.delete_active_page());
        let before = e.document.to_json().unwrap();
        assert!(e.duplicate_active_page().is_err());
        assert_eq!(e.document.to_json().unwrap(), before);
        assert!(!e.can_undo());
        e.document.next_id = 2;
        let other = e.add_page("Other");
        e.begin_transaction("Unfinished drag");
        assert!(!e.delete_active_page());
        assert!(e.duplicate_active_page().is_err());
        e.cancel_transaction();
        assert_eq!(e.active_page, other);
        assert!(e.delete_active_page());
        assert_eq!(e.active_page, 1);
    }
}
