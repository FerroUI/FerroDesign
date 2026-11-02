//! Keyboard moves use page coordinates. Children owned by a layout keep their
//! computed position and move one place in their row or column instead.
use crate::{Document, Editor, LayoutAxis, NodeId, Point, Transform};
use std::collections::{BTreeMap, BTreeSet};

impl Editor {
    pub fn nudge_selection(&mut self, delta: Point) -> bool {
        if self.has_active_transaction() || !delta.x.is_finite() || !delta.y.is_finite() {
            return false;
        }
        let doc = &self.document;
        let mut positions = Vec::new();
        let mut groups = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
        for id in self.root_selection() {
            let Some(n) = doc.nodes.get(&id) else {
                continue;
            };
            let Some(parent) = n.parent else { continue };
            if !doc.effective_visible(id) || doc.page_of(id) != Some(self.active_page) {
                continue;
            }
            if !n.layout_child.absolute
                && doc.nodes.get(&parent).is_some_and(|p| p.layout.is_some())
            {
                groups.entry(parent).or_default().insert(id);
                continue;
            }
            let transform = if parent == self.active_page {
                Some(Transform::IDENTITY)
            } else {
                doc.world_transform(parent).and_then(Transform::inverse)
            };
            let Some(local) = transform.map(|t| t.apply_vector(delta)) else {
                continue;
            };
            let point = Point::new(n.x + local.x, n.y + local.y);
            if point.x.is_finite() && point.y.is_finite() && (point.x != n.x || point.y != n.y) {
                positions.push((id, point));
            }
        }
        let set_positions = |doc: &mut Document| {
            for &(id, p) in &positions {
                let node = doc.nodes.get_mut(&id).unwrap();
                node.x = p.x;
                node.y = p.y;
                if node.component_source.is_some() {
                    node.overridden_properties.insert("position".into());
                }
            }
        };
        let orders: Vec<_> = groups
            .into_iter()
            .filter_map(|(id, selected)| {
                nudge_flow(doc, id, &selected, delta).map(|order| (id, order))
            })
            .collect();
        if orders.is_empty() {
            if positions.is_empty() {
                return false;
            }
            let ids: Vec<_> = positions.iter().map(|&(id, _)| id).collect();
            self.edit_nodes("Nudge layers", &ids, |e| set_positions(&mut e.document));
        } else {
            self.transact("Nudge layers", |doc| {
                for (id, order) in orders {
                    doc.nodes.get_mut(&id).unwrap().children = order;
                }
                set_positions(doc);
                doc.layout_all();
            });
        }
        true
    }
}

fn nudge_flow(
    doc: &Document,
    parent: NodeId,
    selected: &BTreeSet<NodeId>,
    delta: Point,
) -> Option<Vec<NodeId>> {
    crate::layout_drag::validate_layout_reorder_ancestry(doc, parent).ok()?;
    let n = doc.nodes.get(&parent)?;
    let layout = n.layout.as_ref()?;
    let local = doc.world_transform(parent)?.inverse()?.apply_vector(delta);
    let along = match layout.axis {
        LayoutAxis::Row if local.x.abs() > local.y.abs() => local.x,
        LayoutAxis::Column if local.y.abs() > local.x.abs() => local.y,
        _ => return None,
    };
    let forward = (along > 0.0) != layout.reverse;
    let mut children = n.children.clone();
    // Absolute and invisible children keep their authored stacking slots.
    let slots: Vec<_> = children
        .iter()
        .enumerate()
        .filter_map(|(i, id)| {
            let child = doc.nodes.get(id)?;
            (child.visible && !child.layout_child.absolute).then_some(i)
        })
        .collect();
    let mut changed = false;
    for offset in 1..slots.len() {
        let i = if forward {
            slots.len() - offset
        } else {
            offset
        };
        let (from, to) = if forward {
            (slots[i - 1], slots[i])
        } else {
            (slots[i], slots[i - 1])
        };
        if selected.contains(&children[from])
            && !selected.contains(&children[to])
            && !doc.nodes[&children[to]].locked
        {
            children.swap(from, to);
            changed = true;
        }
    }
    changed.then_some(children)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AutoLayout, Node, NodeKind};

    #[test]
    fn fractional_page_nudges_preserve_nested_axes_selection_locks_and_exact_history() {
        let mut d = Document::new("Nudge coordinates");
        let mut parent = Node::new(NodeKind::Frame, "Rotated", 9., 7., 200., 160.);
        parent.rotation = 30.;
        parent.affine_transform = Some(Transform::scale(1.5, 0.75));
        let p = d.insert_node(1, parent).unwrap();
        let a = d
            .insert_node(p, Node::new(NodeKind::Rectangle, "A", 0.125, 12.75, 4., 6.))
            .unwrap();
        let b = d
            .insert_node(p, Node::new(NodeKind::Rectangle, "B", 5., 16., 16., 19.))
            .unwrap();
        d.nodes.get_mut(&b).unwrap().locked = true;
        let mut e = Editor::new(d);
        e.selection = vec![a, b];
        let before = e.document.to_json().unwrap();
        let origin = e
            .document
            .world_transform(a)
            .unwrap()
            .apply(Point::default());
        let held = e.document.nodes[&b].bounds();
        let delta = Point::new(-0.125, 0.);
        assert!(e.nudge_selection(delta));
        assert!(
            e.document
                .world_transform(a)
                .unwrap()
                .apply(Point::default())
                .distance(origin + delta)
                < 1e-8
        );
        assert_eq!(e.document.nodes[&b].bounds(), held);
        assert!(e.undo());
        assert_eq!(e.document.to_json().unwrap(), before);
        assert_eq!(e.selection, [a, b]);
        e.selection = vec![b];
        assert!(!e.nudge_selection(delta));
        assert!(e.redo());
    }

    #[test]
    fn mixed_flow_and_free_roots_nudge_once_and_preserve_fixed_stacking_slots() {
        let mut d = Document::new("Flow keys");
        let mut row = Node::new(NodeKind::Frame, "Row", 0., 0., 240., 48.);
        row.layout = Some(AutoLayout {
            axis: LayoutAxis::Row,
            reverse: true,
            ..Default::default()
        });
        let row = d.insert_node(1, row).unwrap();
        let add = |d: &mut Document, name, parent| {
            d.insert_node(
                parent,
                Node::new(NodeKind::Rectangle, name, 5., 0., 20., 20.),
            )
            .unwrap()
        };
        let a = add(&mut d, "A", row);
        let absolute = add(&mut d, "Pinned", row);
        d.nodes.get_mut(&absolute).unwrap().layout_child.absolute = true;
        let b = add(&mut d, "B", row);
        let c = add(&mut d, "C", row);
        let free = add(&mut d, "Free", 1);
        d.layout_all();
        let mut e = Editor::new(d);
        e.selection = vec![a, b, free];
        let before = e.document.to_json().unwrap();
        assert!(e.nudge_selection(Point::new(-8., 0.)));
        assert_eq!(e.document.nodes[&row].children, [c, absolute, a, b]);
        assert_eq!(e.document.nodes[&free].x, -3.);
        assert_eq!(e.history_len(), 1);
        assert!(e.undo());
        assert_eq!(e.document.to_json().unwrap(), before);
        e.selection = vec![a, b];
        assert!(!e.nudge_selection(Point::new(0., 8.)));
        assert!(e.redo());
    }
}
