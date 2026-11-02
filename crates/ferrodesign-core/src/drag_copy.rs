//! Validate and build a complete drop before exposing the new hierarchy to history.
use super::*;

impl LayoutTransferSession {
    /// A free move inside an ordinary group keeps its hierarchy. An actual
    /// frame under the pointer may adopt the copies, including an auto-layout frame.
    pub fn preserves_drag_parents(&self, document: &Document, target: LayoutTransferTarget) -> bool {
        target.kind == LayoutTransferKind::Restore
            || (target.kind == LayoutTransferKind::Free
                && (self
                    .nodes
                    .iter()
                    .all(|source| source.geometry.parent == Some(target.parent))
                    || (target.parent == self.page
                        && self.source_parents.iter().all(|id| {
                            *id == self.page
                                || document
                                    .nodes
                                    .get(id)
                                    .is_some_and(|n| n.kind == NodeKind::Group && n.layout.is_none())
                        }))))
    }
}

impl Editor {
    pub fn commit_drag_copy(
        &mut self,
        session: &LayoutTransferSession,
        target: LayoutTransferTarget,
    ) -> Result<Vec<NodeId>, DocumentError> {
        if !session.copy_slots {
            return Err(drag_error("A duplicate requires copy insertion slots"));
        }
        if self.has_active_transaction()
            || self.active_page != session.page
            || self.revision != session.revision
        {
            return Err(drag_error("The document changed during the drag"));
        }
        let destination = session.validate_current(&self.document, target)?;
        if target.kind != LayoutTransferKind::Restore
            && (target.kind == LayoutTransferKind::Flow) != destination.layout.is_some()
        {
            return Err(drag_error(
                "The insertion mode does not match the destination layout",
            ));
        }
        let preserve = session.preserves_drag_parents(&self.document, target);
        let delta = target.position - session.origin;
        let mut candidate = self.document.clone();
        let ordinary: Vec<_> = session
            .source_ids()
            .filter(|id| self.document.nodes[id].kind != NodeKind::Component)
            .collect();
        // Clone ordinary trees in one pass so references between selected siblings remap together.
        let roots = crate::editor::clone_tree_into(
            &mut candidate,
            &self.document.nodes,
            &self.document.guides,
            &ordinary,
            session.page,
            Point::default(),
            false,
        );
        if roots.len() != ordinary.len() {
            return Err(drag_error("The duplicate exceeds the document node limit"));
        }
        let mut copies: BTreeMap<_, _> = ordinary.into_iter().zip(roots).collect();
        for source in &session.nodes {
            let id = source.geometry.id;
            let old = &self.document.nodes[&id];
            if old.kind == NodeKind::Component {
                let instance = candidate.create_component_instance(id, session.page, old.x, old.y)?;
                copies.insert(id, instance);
            }
        }
        let new_roots: Vec<_> = session.source_ids().map(|id| copies[&id]).collect();
        let staged: BTreeSet<_> = new_roots.iter().copied().collect();
        candidate
            .children_mut(session.page)
            .unwrap()
            .retain(|id| !staged.contains(id));
        for source in &session.nodes {
            let id = source.geometry.id;
            let copy = copies[&id];
            let old_parent = source.geometry.parent.unwrap();
            let parent = if preserve { old_parent } else { target.parent };
            let inverse = self
                .document
                .world_transform(parent)
                .and_then(Transform::inverse)
                .ok_or_else(|| drag_error("Cannot position a copy in a singular frame"))?;
            let node = candidate.nodes.get_mut(&copy).unwrap();
            node.parent = Some(parent);
            if parent == old_parent {
                let local = inverse.apply_vector(delta);
                let old = &self.document.nodes[&id];
                node.x = old.x + local.x;
                node.y = old.y + local.y;
            } else {
                node.set_local_transform(inverse * Transform::translation(delta.x, delta.y) * source.world);
            }
            if !node.layout_child.absolute && target.kind == LayoutTransferKind::Flow {
                if let Some(grid) = &mut node.layout_child.grid {
                    grid.row = None;
                    grid.column = None;
                }
            }
            if preserve {
                let siblings = candidate
                    .children_mut(parent)
                    .ok_or(DocumentError::InvalidParent(parent))?;
                let index = siblings
                    .iter()
                    .position(|sibling| *sibling == id)
                    .ok_or(DocumentError::MissingNode(id))?;
                siblings.insert(index + 1, copy);
            }
        }
        if !preserve {
            let children = candidate
                .children_mut(target.parent)
                .ok_or(DocumentError::InvalidParent(target.parent))?;
            if target.kind == LayoutTransferKind::Flow {
                let layout = destination
                    .layout
                    .as_ref()
                    .ok_or_else(|| drag_error("Missing auto layout"))?;
                if target.index > destination.flow.len() {
                    return Err(drag_error("Invalid layout slot"));
                }
                let next = destination.flow.get(target.index);
                let neighbor = next.or_else(|| destination.flow.last());
                let index = neighbor
                    .and_then(|id| children.iter().position(|sibling| sibling == id))
                    .map_or(children.len(), |i| {
                        i + usize::from(next.is_some() == layout.reverse)
                    });
                let mut order = new_roots.clone();
                if layout.reverse {
                    order.reverse();
                }
                children.splice(index..index, order);
            } else {
                children.extend_from_slice(&new_roots);
            }
        }
        candidate.validate()?;
        let result = candidate.layout_all();
        if !result.errors.is_empty() {
            return Err(drag_error(result.errors.join("\n")));
        }
        candidate.validate()?;
        let first = session
            .nodes
            .first()
            .ok_or_else(|| drag_error("Select a layer to duplicate"))?;
        let displacement = candidate
            .world_origin(new_roots[0])
            .ok_or(DocumentError::MissingNode(new_roots[0]))?
            - first.world.apply(Point::default());
        // Keep the grab relative to the first copied root; layout may change
        // its actual displacement from the pointer's free-canvas translation.
        let grab = candidate
            .world_bounds(new_roots[0])
            .ok_or(DocumentError::MissingNode(new_roots[0]))?
            .center();
        self.edit("Duplicate layers", |editor| {
            editor.document = candidate;
            editor.selection = new_roots.clone();
        });
        self.remember_duplicate_placement(grab, displacement);
        Ok(new_roots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_copy_preserves_affine_subtrees_and_cross_root_links_in_a_single_undo() {
        let mut doc = Document::new("Affine duplicate");
        let mut frame = Node::new(NodeKind::Frame, "Frame", 120.0, 50.0, 280.0, 220.0);
        frame.rotation = 28.0;
        frame.affine_transform = Some(Transform {
            c: 0.3,
            ..Transform::IDENTITY
        });
        let frame = doc.insert_node(1, frame).unwrap();
        let group = doc
            .insert_node(frame, Node::new(NodeKind::Group, "Group", 18.0, 16.0, 80.0, 60.0))
            .unwrap();
        let child = doc
            .insert_node(
                group,
                Node::new(NodeKind::Rectangle, "Leaf", 8.0, 6.0, 32.0, 18.0),
            )
            .unwrap();
        let peer = doc
            .insert_node(frame, Node::new(NodeKind::Frame, "Peer", 106.0, 15.0, 72.0, 60.0))
            .unwrap();
        doc.nodes.get_mut(&child).unwrap().interactions.push(Interaction {
            action: PrototypeAction::NavigateTo { target: peer },
            trigger: Trigger::Click,
            transition: Transition::default(),
            delay_ms: 0,
            key: None,
        });
        let origin = doc.world_bounds(child).unwrap().center();
        let delta = Point::new(5.0, 8.0);
        let mut editor = Editor::new(doc);
        editor.selection = vec![group, child, peer];
        let before = editor.document.to_json().unwrap();
        let session =
            LayoutTransferSession::for_copy(&editor.document, &editor.selection, group, origin).unwrap();
        let target = session.target(origin + delta).unwrap();
        let copies = editor.commit_drag_copy(&session, target).unwrap();
        assert_eq!(copies.len(), 2);
        let new_child = editor.document.nodes[&copies[0]].children[0];
        assert_eq!(
            editor.document.nodes[&frame].children,
            [group, copies[0], peer, copies[1]]
        );
        assert!(
            editor
                .document
                .world_bounds(new_child)
                .unwrap()
                .center()
                .distance(origin + delta)
                < 1e-8
        );
        assert_eq!(
            editor.document.nodes[&new_child].interactions[0].action,
            PrototypeAction::NavigateTo { target: copies[1] }
        );
        assert_eq!(editor.selection, copies);
        assert_eq!(editor.history_len(), 1);
        let after = editor.document.to_json().unwrap();
        assert!(editor.undo());
        assert_eq!(editor.selection, [group, child, peer]);
        assert_eq!(editor.document.to_json().unwrap(), before);
        assert!(editor.redo());
        assert_eq!(editor.selection, copies);
        assert_eq!(editor.document.to_json().unwrap(), after);
    }

    #[test]
    fn drag_copy_main_component_creates_instance_at_flow_slot_and_rejects_stale_drop() {
        let mut doc = Document::new("Drag main component into layout");
        let component = doc
            .insert_node(
                1,
                Node::new(NodeKind::Component, "Button", 10.0, 10.0, 50.0, 36.0),
            )
            .unwrap();
        let label = doc
            .insert_node(
                component,
                Node::new(NodeKind::Text, "Label", 5.0, 6.0, 30.0, 16.0),
            )
            .unwrap();
        let mut row = Node::new(NodeKind::Frame, "Row", 250.0, 50.0, 200.0, 80.0);
        row.layout = Some(AutoLayout {
            axis: LayoutAxis::Row,
            ..Default::default()
        });
        let row = doc.insert_node(1, row).unwrap();
        let sibling = doc
            .insert_node(row, Node::new(NodeKind::Rectangle, "First", 0.0, 0.0, 40.0, 30.0))
            .unwrap();
        let mut editor = Editor::new(doc);
        editor.selection = vec![component];
        let session = LayoutTransferSession::for_copy(
            &editor.document,
            &[component],
            component,
            Point::new(30.0, 28.0),
        )
        .unwrap();
        let target = session.target(Point::new(395.0, 90.0)).unwrap();
        assert_eq!(target.kind, LayoutTransferKind::Flow);
        let id = editor.commit_drag_copy(&session, target).unwrap()[0];
        assert_eq!(editor.document.nodes[&row].children, [sibling, id]);
        let instance = &editor.document.nodes[&id];
        assert_eq!(instance.kind, NodeKind::Instance);
        assert_eq!(instance.component_id, Some(component));
        assert_eq!(
            editor.document.nodes[&instance.children[0]].component_source,
            Some(label)
        );
        assert_eq!(editor.document.nodes[&component].children, [label]);
        assert!(editor.commit_drag_copy(&session, target).is_err());
        assert_eq!(editor.history_len(), 1);
    }
    #[test]
    fn duplicating_inside_a_reversed_layout_keeps_the_original_in_slot_hit_testing() {
        let mut doc = Document::new("Reversed slots");
        let mut frame = Node::new(NodeKind::Frame, "Toolbar", 0.0, 0.0, 260.0, 80.0);
        frame.layout = Some(AutoLayout {
            reverse: true,
            ..Default::default()
        });
        let frame = doc.insert_node(1, frame).unwrap();
        let left = doc
            .insert_node(frame, Node::new(NodeKind::Rectangle, "A", 0.0, 0.0, 58.0, 34.0))
            .unwrap();
        let right = doc
            .insert_node(frame, Node::new(NodeKind::Rectangle, "B", 0.0, 0.0, 64.0, 34.0))
            .unwrap();
        doc.layout_all();
        let mut editor = Editor::new(doc);
        let rect = editor.document.world_bounds(left).unwrap();
        let origin = rect.center();
        let session = LayoutTransferSession::for_copy(&editor.document, &[left], left, origin).unwrap();
        // Drop just inside the visual right edge of A, after A in screen order.
        let target = session.target(Point::new(rect.right() - 1.0, origin.y)).unwrap();
        assert_eq!(target.kind, LayoutTransferKind::Flow);
        let copy = editor.commit_drag_copy(&session, target).unwrap()[0];
        assert_eq!(editor.document.nodes[&frame].children, [copy, left, right]);
        assert!(editor.document.nodes[&copy].x > editor.document.nodes[&left].x);
    }
    #[test]
    fn duplicate_command_repeats_free_drag_spacing_until_another_edit() {
        let mut doc = Document::new("Repeat duplicate");
        let id = doc
            .insert_node(1, Node::new(NodeKind::Rectangle, "Tile", 10.0, 20.0, 12.0, 10.0))
            .unwrap();
        let backdrop = doc.insert_node(1, Node::new(NodeKind::Frame, "Overlaps second copy", 58.0, 10.0, 40.0, 20.0)).unwrap();
        let mut editor = Editor::new(doc);
        editor.selection = vec![id];
        let origin = Point::new(16.0, 25.0);
        let delta = Point::new(22.5, -3.0);
        let session = LayoutTransferSession::for_copy(&editor.document, &[id], id, origin).unwrap();
        let first = editor
            .commit_drag_copy(&session, session.target(origin + delta).unwrap())
            .unwrap()[0];
        assert_eq!(editor.document.nodes[&first].bounds().center(), origin + delta);
        for step in [2.0, 3.0] {
            let next = editor.duplicate_selection()[0];
            assert_eq!(
                editor.document.nodes[&next].bounds().center(),
                origin + delta * step
            );
        }
        assert_eq!(editor.history_len(), 3);
        assert!(editor.document.nodes[&backdrop].children.is_empty());
        editor.move_selection(1.0, 0.0);
        let prior = editor.document.nodes[&editor.selection[0]].bounds().center();
        let other = editor.duplicate_selection()[0];
        assert_eq!(
            editor.document.nodes[&other].bounds().center(),
            prior + Point::new(24.0, 24.0)
        );
    }
}
