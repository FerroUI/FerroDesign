//! Rigid world-space rotation of editable roots. Child geometry, paint and
//! layout properties are untouched. Samples are absolute from pointer-down.
use super::*;

#[derive(Clone, Copy)]
pub struct RotationBounds {
    pub corners: [Point; 4],
    pub anchor_layer: NodeId,
    pub free_origin: bool,
}

impl RotationBounds {
    pub fn center(self) -> Point {
        (self.corners[0] + self.corners[2]) * 0.5
    }
}

#[derive(Clone, Copy)]
struct Pose {
    position: Point,
    degrees: f64,
    affine: Option<Transform>,
}

struct Layer {
    id: NodeId,
    original: Pose,
    flips: Transform,
    center: Point,
    parent: Transform,
    inverse: Transform,
    world: Transform,
}

// Keep the original flips and any authored affine translation. Under a
// sheared/nonuniform parent the conjugated rotation is itself affine.
impl Layer {
    fn pose(&self, orbit: Transform, degrees: f64) -> Option<Pose> {
        if degrees == 0.0 {
            return Some(self.original);
        }
        let local = self.inverse * orbit * self.world;
        let axis = (self.inverse * orbit * self.parent * Transform::rotation(self.original.degrees))
            .apply_vector(Point::new(1.0, 0.0));
        let angle = axis.y.atan2(axis.x).to_degrees();
        let rigid = Transform::rotation(angle) * self.flips;
        let previous = self.original.affine.unwrap_or(Transform::IDENTITY);
        let affine = Transform {
            e: previous.e,
            f: previous.f,
            ..rigid.inverse()? * local
        };
        let similar = [
            affine.a - previous.a,
            affine.b - previous.b,
            affine.c - previous.c,
            affine.d - previous.d,
        ]
        .into_iter()
        .all(|v| v.abs() <= 1e-12);
        let retained = if similar {
            self.original.affine
        } else {
            Some(affine)
        };
        let offset = self.center + rigid.apply_vector(Point::new(previous.e, previous.f) - self.center);
        let position = Point::new(local.e, local.f) - offset;
        [
            position.x, position.y, angle, affine.a, affine.b, affine.c, affine.d,
        ]
        .into_iter()
        .all(f64::is_finite)
        .then_some(Pose {
            position,
            degrees: angle,
            affine: retained,
        })
    }
}

/// A live collective rotation. The transaction and original transforms belong
/// to this session; foreign editor replacements and selection changes invalidate it.
pub struct SelectionRotation {
    owner: EditorTransactionToken,
    version: EditorRenderToken,
    selected: Vec<NodeId>,
    page: NodeId,
    ids: Vec<NodeId>,
    layers: Vec<Layer>,
    poses: Vec<Pose>,
    pivot: Point,
    last: f64,
}

impl Editor {
    // Rotation and multi-resize share the same definition of an editable
    // transform root. A single layout child may rotate around its own center.
    pub(super) fn transform_root(
        &self,
        id: NodeId,
        allow_flow: bool,
    ) -> Option<(Transform, Transform, bool)> {
        let n = self.document.nodes.get(&id)?;
        let parent = n.parent?;
        let flow = self
            .document
            .nodes
            .get(&parent)
            .is_some_and(|p| p.layout.is_some() && !n.layout_child.absolute);
        if (flow && !allow_flow)
            || !n.visible
            || self.document.page_of(id) != Some(self.active_page)
            || crate::layout_drag::validate_layout_reorder_ancestry(&self.document, parent).is_err()
        {
            return None;
        }
        let world = *self.scene.transforms.get(&id)?;
        world.inverse()?;
        let parent = if parent == self.active_page {
            Transform::IDENTITY
        } else {
            *self.scene.transforms.get(&parent)?
        };
        Some((world, parent, !flow))
    }

    fn rotation_roots(&self) -> Vec<NodeId> {
        let mut ids = self.root_selection();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn bounds_for_rotation(&self, ids: &[NodeId]) -> Option<RotationBounds> {
        let &first = ids.first()?;
        if ids.len() == 1 {
            let (world, _, free_origin) = self.transform_root(first, true)?;
            let corners = self.document.nodes[&first]
                .local_bounds()
                .corners()
                .map(|p| world.apply(p));
            if corners.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
                return None;
            }
            return Some(RotationBounds {
                corners,
                anchor_layer: first,
                free_origin,
            });
        }
        let mut bounds = None;
        for &id in ids {
            let (world, _, _) = self.transform_root(id, false)?;
            let b = world.transform_rect(self.document.nodes[&id].local_bounds());
            if !b.is_finite() || !b.right().is_finite() || !b.bottom().is_finite() {
                return None;
            }
            bounds = Some(bounds.map_or(b, |prior: Rect| prior.union(b)));
        }
        Some(RotationBounds {
            corners: bounds?.corners(),
            anchor_layer: first,
            free_origin: true,
        })
    }

    pub fn rotation_bounds(&self) -> Option<RotationBounds> {
        self.bounds_for_rotation(&self.rotation_roots())
    }

    pub fn begin_selection_rotation(&mut self, pivot: Option<Point>) -> Option<SelectionRotation> {
        if self.has_active_transaction() {
            return None;
        }
        let ids = self.rotation_roots();
        let bounds = self.bounds_for_rotation(&ids)?;
        let pivot = pivot
            .filter(|_| bounds.free_origin)
            .unwrap_or_else(|| bounds.center());
        if !pivot.x.is_finite() || !pivot.y.is_finite() {
            return None;
        }
        let mut layers = Vec::with_capacity(ids.len());
        for &id in &ids {
            let n = &self.document.nodes[&id];
            let (world, parent, _) = self.transform_root(id, ids.len() == 1)?;
            layers.push(Layer {
                id,
                world,
                parent,
                inverse: parent.inverse()?,
                center: n.local_bounds().center(),
                original: Pose {
                    position: Point::new(n.x, n.y),
                    degrees: n.rotation,
                    affine: n.affine_transform,
                },
                flips: Transform::scale(
                    if n.flip_x { -1.0 } else { 1.0 },
                    if n.flip_y { -1.0 } else { 1.0 },
                ),
            });
        }
        self.begin_scoped_node_transaction(&ids, "Rotate selection");
        Some(SelectionRotation {
            owner: self.active_transaction_token()?,
            version: self.render_change_token(),
            selected: self.selection.clone(),
            page: self.active_page,
            poses: Vec::with_capacity(ids.len()),
            ids,
            layers,
            pivot,
            last: 0.0,
        })
    }

    pub fn rotate_selection_about(&mut self, pivot: Option<Point>, degrees: f64) -> bool {
        if !degrees.is_finite() || degrees % 360.0 == 0.0 {
            return false;
        }
        let Some(mut rotation) = self.begin_selection_rotation(pivot) else {
            return false;
        };
        if rotation.preview(self, degrees) {
            rotation.commit(self)
        } else {
            rotation.cancel(self);
            false
        }
    }
}

impl SelectionRotation {
    fn owns(&self, editor: &Editor) -> bool {
        editor.active_transaction_token().as_ref() == Some(&self.owner)
    }
    fn valid(&self, editor: &Editor) -> bool {
        self.owns(editor)
            && editor.selection == self.selected
            && editor.active_page == self.page
            && editor.render_change_token() == self.version
    }
    pub fn preview(&mut self, editor: &mut Editor, degrees: f64) -> bool {
        if !self.valid(editor) || !degrees.is_finite() {
            return false;
        }
        let degrees = degrees % 360.0;
        if degrees == self.last {
            return true;
        }
        let orbit = Transform::translation(self.pivot.x, self.pivot.y)
            * Transform::rotation(degrees)
            * Transform::translation(-self.pivot.x, -self.pivot.y);
        self.poses.clear();
        for layer in &self.layers {
            let Some(pose) = layer.pose(orbit, degrees) else {
                return false;
            };
            self.poses.push(pose);
        }
        for (layer, pose) in self.layers.iter().zip(&self.poses) {
            let n = editor.document.nodes.get_mut(&layer.id).unwrap();
            n.x = pose.position.x;
            n.y = pose.position.y;
            n.rotation = pose.degrees;
            n.affine_transform = pose.affine;
        }
        editor.refresh_preview_nodes(&self.ids);
        self.version = editor.render_change_token();
        self.last = degrees;
        true
    }
    pub fn commit(self, editor: &mut Editor) -> bool {
        if !self.valid(editor) {
            self.cancel(editor);
            return false;
        }
        editor.commit_transaction()
    }
    pub fn cancel(self, editor: &mut Editor) {
        if self.owns(editor) {
            let selected = editor.selection.clone();
            let page = editor.active_page;
            editor.cancel_transaction();
            if page != editor.active_page {
                editor.set_active_page(page);
            }
            editor.selection = selected;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corners(d: &Document, id: NodeId) -> [Point; 4] {
        let t = d.world_transform(id).unwrap();
        d.nodes[&id].local_bounds().corners().map(|p| t.apply(p))
    }

    #[test]
    fn selection_rotation_orbits_affine_roots_rigidly_and_restores_authored_representations() {
        let mut d = Document::new("Rigid ensemble");
        let mut parent = Node::new(NodeKind::Frame, "Reflected parent", 124., 56., 420., 370.);
        parent.rotation = 17.;
        parent.flip_y = true;
        parent.affine_transform = Some(Transform {
            a: 1.6,
            b: 0.23,
            c: -0.28,
            d: 0.8,
            e: 8.,
            f: -3.,
        });
        let parent = d.insert_node(1, parent).unwrap();
        let mut layer = Node::new(NodeKind::Frame, "Flipped", 37., 71., 102., 60.);
        layer.flip_x = true;
        layer.rotation = 28.;
        layer.affine_transform = Some(Transform {
            c: 0.22,
            e: 7.,
            f: 3.,
            ..Transform::IDENTITY
        });
        let layer = d.insert_node(parent, layer).unwrap();
        let child = d
            .insert_node(layer, Node::new(NodeKind::Rectangle, "Child", 6., 5., 20., 10.))
            .unwrap();
        let peer = d
            .insert_node(1, Node::new(NodeKind::Text, "Peer", 500., 90., 80., 44.))
            .unwrap();
        let pivot = Point::new(90.5, -22.25);
        let mut e = Editor::new(d);
        e.selection = vec![peer, child, layer];
        let before = e.document.to_json().unwrap();
        let original = [layer, child, peer].map(|id| corners(&e.document, id));
        let mut s = e.begin_selection_rotation(Some(pivot)).unwrap();
        assert!(s.preview(&mut e, -67.));
        assert!(s.preview(&mut e, 360.));
        assert_eq!(e.document.to_json().unwrap(), before);
        assert!(s.preview(&mut e, 118.5));
        let orbit = Transform::translation(pivot.x, pivot.y)
            * Transform::rotation(118.5)
            * Transform::translation(-pivot.x, -pivot.y);
        for (id, old) in [layer, child, peer].into_iter().zip(original) {
            for (p, q) in old.into_iter().zip(corners(&e.document, id)) {
                assert!(orbit.apply(p).distance(q) < 1e-8);
            }
        }
        assert!(e.document.nodes[&layer].flip_x);
        assert_eq!(e.document.nodes[&child].rotation, 0.);
        assert!(s.commit(&mut e));
        let after = e.document.to_json().unwrap();
        assert_eq!(e.history_len(), 1);
        assert!(e.undo());
        assert_eq!(e.document.to_json().unwrap(), before);
        assert!(e.redo());
        assert_eq!(e.document.to_json().unwrap(), after);
    }

    #[test]
    fn selection_rotation_handles_locked_layers_changed_selection_and_replaced_editors() {
        let mut d = Document::new("Ownership");
        let a = d
            .insert_node(1, Node::new(NodeKind::Rectangle, "A", 0., 0., 40., 30.))
            .unwrap();
        let mut locked = Node::new(NodeKind::Rectangle, "Locked", 380., 420., 100., 100.);
        locked.locked = true;
        let b = d.insert_node(1, locked).unwrap();
        let mut e = Editor::new(d);
        e.selection = vec![a, b];
        assert_eq!(e.rotation_bounds().unwrap().center(), Point::new(20., 15.));
        let before = e.document.to_json().unwrap();
        let mut s = e.begin_selection_rotation(None).unwrap();
        assert!(s.preview(&mut e, 14.));
        e.selection = vec![b];
        assert!(!s.commit(&mut e));
        assert_eq!(e.selection, [b]);
        assert_eq!(e.document.to_json().unwrap(), before);
        e.selection = vec![a];
        let mut old = e.begin_selection_rotation(None).unwrap();
        e = Editor::new(e.document.clone());
        e.selection = vec![a];
        e.begin_transaction("Inspector");
        let foreign = e.active_transaction_token();
        assert!(!old.preview(&mut e, 35.));
        old.cancel(&mut e);
        assert_eq!(e.active_transaction_token(), foreign);
        e.cancel_transaction();
    }
}
