//! Move-tool resize of freely positioned roots in their shared world bounds.
use super::*;

struct Root {
    id: NodeId,
    world: Transform,
    parent_inverse: Transform,
    size: Point,
    in_group: bool,
}

#[derive(Clone, Copy)]
struct Placement {
    size: Point,
    local: Transform,
}

impl Root {
    fn placement(&self, pivot: Point, factors: Point, parent_inverse: Transform) -> Option<Placement> {
        // Absorb the change of each local basis vector's length into its
        // authored dimension. Only the remaining shear/rotation goes in the
        // affine. This keeps editable dimensions and ordinary stroke/text sizes.
        let ratio = |x: f64, y: f64| {
            let raw = (factors.x * x).hypot(factors.y * y) / x.hypot(y);
            if (raw - 1.0).abs() < 1e-12 {
                1.0
            } else {
                raw
            }
        };
        let rx = ratio(self.world.a, self.world.b);
        let ry = ratio(self.world.c, self.world.d);
        let size = Point::new(self.size.x * rx, self.size.y * ry);
        let stretch = Transform::translation(pivot.x, pivot.y)
            * Transform::scale(factors.x, factors.y)
            * Transform::translation(-pivot.x, -pivot.y);
        let local = parent_inverse * stretch * self.world * Transform::scale(1.0 / rx, 1.0 / ry);
        ([local.a, local.b, local.c, local.d, local.e, local.f]
            .into_iter()
            .all(f64::is_finite)
            && [size.x, size.y].into_iter().all(|v| (0.0..=1e7).contains(&v)))
        .then_some(Placement { size, local })
    }
}

/// An undoable world-space resize. A preview is always evaluated from the
/// original geometry, even after an earlier sample clamped a constrained child.
pub struct SelectionResize {
    token: EditorTransactionToken,
    version: EditorRenderToken,
    bounds: Rect,
    roots: Vec<Root>,
    affected: Vec<NodeId>,
    selection: Vec<NodeId>,
    page: NodeId,
    last: Option<(ResizeTarget, bool)>,
}

#[derive(Clone, Copy, PartialEq)]
enum ResizeTarget {
    Layer(Rect),
    Selection { pivot: Point, factors: Point },
}

impl ResizeTarget {
    fn unchanged(self, node: &Node) -> bool {
        match self {
            Self::Layer(bounds) => bounds == node.bounds(),
            Self::Selection { factors, .. } => factors == Point::new(1.0, 1.0),
        }
    }
}

// Keep authored rotation, flips and affine residuals for a pure translation.
fn place(node: &mut Node, local: Transform) {
    let current = node.local_transform();
    if [local.a - current.a, local.b - current.b, local.c - current.c, local.d - current.d]
        .into_iter().all(|v| v.abs() < 1e-12)
    {
        let (dx, dy) = (local.e - current.e, local.f - current.f);
        if dx.abs() > 1e-12 { node.x += dx; }
        if dy.abs() > 1e-12 { node.y += dy; }
    } else {
        node.set_local_transform(local);
    }
}

impl Node {
    pub fn can_resize_around_contents(&self) -> bool {
        matches!(self.kind, NodeKind::Frame | NodeKind::Component | NodeKind::Instance | NodeKind::Section)
            || (self.kind == NodeKind::Group && self.layout.is_some())
    }
}

impl Editor {
    fn free_resize_roots(&self) -> Option<Vec<Root>> {
        if self.selection.len() < 2 {
            return None;
        }
        let mut ids = self.root_selection();
        ids.sort_unstable();
        ids.dedup();
        if ids.len() < 2 {
            return None;
        }
        ids.into_iter()
            .map(|id| {
                let node = self.document.nodes.get(&id)?;
                let (world, parent, _) = self.transform_root(id, false)?;
                let parent_inverse = parent.inverse()?;
                Some(Root {
                    id,
                    world,
                    parent_inverse,
                    size: Point::new(node.width, node.height),
                    in_group: false,
                })
            })
            .collect()
    }

    /// Start a Move-tool resize in a single layer's own axes.
    pub fn begin_single_resize(&mut self, id: NodeId) -> Option<SelectionResize> {
        if self.has_active_transaction() || self.selection.as_slice() != [id]
            || self.document.effective_locked(id) || !self.document.effective_visible(id) {
            return None;
        }
        let node = self.document.nodes.get(&id)?;
        let world = *self.scene.transforms.get(&id)?;
        let root = Root {
            id, world, parent_inverse: node.local_transform() * world.inverse()?,
            size: Point::new(node.width, node.height), in_group: false,
        };
        let bounds = world.transform_rect(node.local_bounds());
        self.start_resize(vec![root], bounds, "Resize")
    }

    /// Shared authored bounds of at least two distinct, editable free roots.
    pub fn multi_resize_bounds(&self) -> Option<Rect> {
        bounds(&self.free_resize_roots()?)
    }

    /// Capture exactly the nodes whose geometry can change during this gesture.
    pub fn begin_multi_resize(&mut self) -> Option<SelectionResize> {
        if self.has_active_transaction() {
            return None;
        }
        let mut roots = self.free_resize_roots()?;
        let bounds = bounds(&roots)?;
        // A plain group has no layout of its own. Resize its children together
        // in world space, including nested groups. Frames instead run their own
        // child constraints and layout once, exactly as an individual resize does.
        let mut index = 0;
        while index < roots.len() {
            let root = &roots[index];
            let node = &self.document.nodes[&root.id];
            if node.kind == NodeKind::Group && node.layout.is_none() {
                let parent_inverse = root.world.inverse()?;
                for id in &node.children {
                    let child = &self.document.nodes[id];
                    let world = *self.scene.transforms.get(id)?;
                    world.inverse()?;
                    roots.push(Root {
                        id: *id,
                        world,
                        parent_inverse,
                        size: Point::new(child.width, child.height),
                        in_group: true,
                    });
                }
            }
            index += 1;
        }
        self.start_resize(roots, bounds, "Resize selection")
    }

    fn start_resize(&mut self, roots: Vec<Root>, bounds: Rect, label: &str) -> Option<SelectionResize> {
        // apply_resize changes a root and its immediate constrained/arranged children.
        let affected: Vec<_> = roots
            .iter()
            .flat_map(|r| std::iter::once(r.id).chain(self.document.nodes[&r.id].children.iter().copied()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.begin_scoped_node_transaction(&affected, label);
        Some(SelectionResize {
            token: self.active_transaction_token()?,
            version: self.render_change_token(),
            bounds,
            roots,
            affected,
            page: self.active_page,
            selection: self.selection.clone(),
            last: None,
        })
    }
}

fn bounds(roots: &[Root]) -> Option<Rect> {
    roots
        .iter()
        .map(|r| r.world.transform_rect(Rect::new(0.0, 0.0, r.size.x, r.size.y)))
        .reduce(Rect::union)
        .filter(|b| b.is_finite())
}

impl SelectionResize {
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    fn owns(&self, editor: &Editor) -> bool {
        editor.active_transaction_token().as_ref() == Some(&self.token)
    }

    fn valid(&self, editor: &Editor) -> bool {
        self.owns(editor)
            && self.page == editor.active_page
            && editor.selection == self.selection
            && self.version == editor.render_change_token()
    }

    // Read the one history snapshot captured at press. Copy back only fields
    // the resize can touch. Unrelated artwork and all paint stacks stay put.
    fn restore_geometry(&self, editor: &mut Editor) {
        let original = match &editor.pending.as_ref().unwrap().before {
            PendingSnapshot::Document(doc) => &doc.nodes,
            PendingSnapshot::Nodes { nodes, .. } => nodes,
        };
        for id in &self.affected {
            let old = &original[id];
            let n = editor.document.nodes.get_mut(id).unwrap();
            (n.x, n.y, n.width, n.height) = (old.x, old.y, old.width, old.height);
            (n.rotation, n.flip_x, n.flip_y, n.affine_transform) =
                (old.rotation, old.flip_x, old.flip_y, old.affine_transform);
            n.text_auto_resize = old.text_auto_resize;
            if let (Some(layout), Some(before)) = (&mut n.layout, &old.layout) {
                layout.width = before.width;
                layout.height = before.height;
            }
            if n.kind == NodeKind::Vector {
                n.path.clone_from(&old.path);
                n.vector_network.clone_from(&old.vector_network);
            }
        }
    }

    fn apply(&self, editor: &mut Editor, target: ResizeTarget, keep_contents: bool) -> bool {
        if target.unchanged(&editor.document.nodes[&self.roots[0].id]) {
            return true;
        }
        for root in &self.roots {
            let parent_inverse = if root.in_group {
                let Some(inverse) = editor.document.nodes[&root.id]
                    .parent
                    .and_then(|id| editor.document.world_transform(id))
                    .and_then(Transform::inverse)
                else {
                    return false;
                };
                inverse
            } else {
                root.parent_inverse
            };
            let Placement { size, local } = match target {
                ResizeTarget::Layer(bounds) => Placement {
                    size: Point::new(bounds.width, bounds.height),
                    local: editor.document.nodes[&root.id].local_transform_for_bounds(bounds),
                },
                ResizeTarget::Selection { pivot, factors } => {
                    let Some(placement) = root.placement(pivot, factors, parent_inverse) else { return false; };
                    placement
                }
            };
            let n = editor.document.nodes.get_mut(&root.id).unwrap();
            if n.kind == NodeKind::Text && size != root.size {
                // Manual bounds wrap auto-width text at its new width.
                n.text_auto_resize = TextAutoResize::Fixed;
            }
            if let Some(layout) = &mut n.layout {
                if size.x != root.size.x {
                    layout.width = SizingMode::Fixed;
                }
                if size.y != root.size.y {
                    layout.height = SizingMode::Fixed;
                }
            }
            let keep_children = keep_contents && n.can_resize_around_contents();
            let group = matches!(target, ResizeTarget::Selection { .. }) && n.kind == NodeKind::Group && n.layout.is_none();
            if keep_children || group {
                n.width = size.x;
                n.height = size.y;
            } else if !editor.apply_resize(root.id, size.x, size.y, None) {
                return false;
            }
            place(editor.document.nodes.get_mut(&root.id).unwrap(), local);
            if keep_children {
                // A moving left/top edge changes the frame origin. Undo its complete
                // change of basis so each child's world geometry stays in place, even
                // when nonuniform selection resizing shears a rotated nested frame.
                let Some(inverse) = local.inverse() else { return false; };
                let restore = inverse * parent_inverse * root.world;
                for id in editor.document.nodes[&root.id].children.clone() {
                    let child = editor.document.nodes.get_mut(&id).unwrap();
                    place(child, restore * child.local_transform());
                }
            }
        }
        true
    }

    /// Apply positive axis factors around a world-space pivot. Invalid samples
    /// leave the previous valid preview in place.
    pub fn preview(&mut self, editor: &mut Editor, pivot: Point, factors: Point) -> bool {
        self.preview_selection(editor, pivot, factors, false)
    }

    pub fn preview_selection(&mut self, editor: &mut Editor, pivot: Point, factors: Point, keep_contents: bool) -> bool {
        if !pivot.x.is_finite() || !pivot.y.is_finite() || ![factors.x, factors.y].into_iter().all(|v| v.is_finite() && v > 0.0) {
            return false;
        }
        self.preview_target(editor, ResizeTarget::Selection { pivot, factors }, keep_contents)
    }

    pub fn preview_layer(&mut self, editor: &mut Editor, bounds: Rect, keep_contents: bool) -> bool {
        if self.roots.len() != 1 || !bounds.is_finite()
            || ![bounds.width, bounds.height].into_iter().all(|v| (0.0..=1e7).contains(&v)) {
            return false;
        }
        self.preview_target(editor, ResizeTarget::Layer(bounds), keep_contents)
    }

    fn preview_target(&mut self, editor: &mut Editor, target: ResizeTarget, keep_contents: bool) -> bool {
        if !self.valid(editor) { return false; }
        if self.last == Some((target, keep_contents))
            || (self.last.is_none() && target.unchanged(&editor.document.nodes[&self.roots[0].id])) {
            return true;
        }
        self.restore_geometry(editor);
        if !self.apply(editor, target, keep_contents) {
            self.restore_geometry(editor);
            if let Some((previous, keep)) = self.last {
                self.apply(editor, previous, keep);
            }
            return false;
        }
        editor.refresh_preview_nodes(&self.affected);
        self.version = editor.render_change_token();
        self.last = Some((target, keep_contents));
        true
    }

    pub fn commit(self, editor: &mut Editor) -> bool {
        if !self.valid(editor) {
            self.cancel(editor);
            return false;
        }
        let originals = match &editor.pending.as_ref().unwrap().before {
            PendingSnapshot::Document(doc) => &doc.nodes,
            PendingSnapshot::Nodes { nodes, .. } => nodes,
        };
        for root in &self.roots {
            let n = editor.document.nodes.get_mut(&root.id).unwrap();
            if n.component_source.is_some() && n.text_auto_resize != originals[&n.id].text_auto_resize {
                n.overridden_properties.insert("text_auto_resize".into());
            }
            if n.component_source.is_some() && n.layout != originals[&n.id].layout {
                n.overridden_properties.insert("layout".into());
            }
            for (key, changed) in [
                ("width", n.width != root.size.x),
                ("height", n.height != root.size.y),
            ] {
                if changed {
                    n.variable_bindings.remove(key);
                    if n.component_source.is_some() {
                        n.overridden_properties.insert(key.into());
                    }
                }
            }
        }
        editor.commit_transaction()
    }

    pub fn cancel(self, editor: &mut Editor) {
        if self.owns(editor) {
            let selection = editor.selection.clone();
            let page = editor.active_page;
            editor.cancel_transaction();
            if page != editor.active_page && editor.document.page(page).is_some() {
                editor.set_active_page(page);
            }
            editor.selection = selection;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(a: Point, b: Point) {
        assert!(a.distance(b) < 1e-7, "{a:?} != {b:?}");
    }

    #[test]
    fn multi_resize_transforms_all_corners_across_parents_without_scaling_strokes_or_text() {
        let mut d = Document::new("Mixed transforms");
        let mut parent = Node::new(NodeKind::Frame, "Parent", -40.0, 60.0, 400.0, 280.0);
        parent.rotation = -14.0;
        parent.affine_transform = Some(Transform {
            c: 0.35,
            ..Transform::IDENTITY
        });
        let parent = d.insert_node(1, parent).unwrap();
        let mut shape = Node::new(NodeKind::Rectangle, "Reflected", 31.0, 98.0, 94.0, 47.0);
        shape.flip_x = true;
        shape.strokes.push(Stroke::new(Color::BLACK, 7.5));
        let shape = d.insert_node(parent, shape).unwrap();
        let group = d
            .insert_node(1, Node::new(NodeKind::Group, "Group", 310.0, 40.0, 130.0, 84.0))
            .unwrap();
        let mut text =
            Node::new(NodeKind::Text, "Title", 30.0, 10.0, 68.0, 28.0).with_text("Title", 19.0, Color::BLACK);
        text.rotation = 31.0;
        text.text_auto_resize = TextAutoResize::WidthAndHeight;
        let text = d.insert_node(group, text).unwrap();
        let mut e = Editor::new(d);
        e.selection = vec![shape, group, text];
        let before = e.document.to_json().unwrap();
        let corners = |d: &Document, id| {
            d.nodes[&id]
                .local_bounds()
                .corners()
                .map(|p| d.world_transform(id).unwrap().apply(p))
        };
        let original = [shape, group, text].map(|id| corners(&e.document, id));
        let mut resize = e.begin_multi_resize().unwrap();
        let pivot = resize.bounds().center();
        let factors = Point::new(1.75, 0.6);
        assert!(resize.preview(&mut e, pivot, Point::new(0.24, 0.3)));
        assert_eq!(e.document.nodes[&text].text_auto_resize, TextAutoResize::Fixed);
        assert!(resize.preview(&mut e, pivot, Point::new(1.0, 1.0)));
        assert_eq!(e.document.nodes[&text].text_auto_resize, TextAutoResize::WidthAndHeight);
        assert!(resize.preview(&mut e, pivot, factors));
        assert!(resize.commit(&mut e));
        for (id, old) in [shape, group, text].into_iter().zip(original) {
            for (p, q) in old.into_iter().zip(corners(&e.document, id)) {
                same(
                    q,
                    Point::new(
                        pivot.x + (p.x - pivot.x) * factors.x,
                        pivot.y + (p.y - pivot.y) * factors.y,
                    ),
                );
            }
        }
        assert_eq!(e.document.nodes[&shape].strokes[0].width, 7.5);
        assert_eq!(e.document.nodes[&text].font_size, 19.0);
        let after = e.document.to_json().unwrap();
        assert_eq!(e.history_len(), 1);
        assert!(e.undo());
        assert_eq!(e.document.to_json().unwrap(), before);
        assert!(e.redo());
        assert_eq!(e.document.to_json().unwrap(), after);
    }

    #[test]
    fn multi_resize_recomputes_clamped_frame_constraints_and_preserves_external_selection() {
        let mut d = Document::new("Constraint baseline");
        let frame = d
            .insert_node(1, Node::new(NodeKind::Frame, "Frame", 5.0, 6.0, 200.0, 100.0))
            .unwrap();
        let mut child = Node::new(NodeKind::Rectangle, "Flexible", 80.0, 12.0, 20.0, 18.0);
        child.constraints.horizontal = Constraint::Stretch;
        let child = d.insert_node(frame, child).unwrap();
        let peer = d
            .insert_node(1, Node::new(NodeKind::Rectangle, "Peer", 240.0, 6.0, 30.0, 30.0))
            .unwrap();
        let mut e = Editor::new(d);
        e.selection = vec![frame, peer];
        let mut s = e.begin_multi_resize().unwrap();
        assert!(s.preview(&mut e, Point::default(), Point::new(0.2, 1.0)));
        assert_eq!(e.document.nodes[&child].width, 0.0);
        assert!(s.preview(&mut e, Point::default(), Point::new(1.1, 1.0)));
        assert!((e.document.nodes[&child].width - 40.0).abs() < 1e-8);
        e.selection = vec![peer];
        assert!(!s.commit(&mut e));
        assert_eq!(e.document.nodes[&child].width, 20.0);
        assert_eq!(e.selection, [peer]);
        assert_eq!(e.history_len(), 0);
    }
    #[test]
    fn bypass_keeps_content_and_descendants_in_world_space_under_rotated_and_skewed_frames() {
        let mut doc = Document::new("Resize a viewport around its content");
        let mut parent = Node::new(NodeKind::Frame, "Board", 10., 25., 500., 400.);
        parent.rotation = 18.;
        let parent = doc.insert_node(1, parent).unwrap();
        let mut frame = Node::new(NodeKind::Frame, "Viewport", 36., 42., 220., 140.);
        frame.rotation = 27.;
        frame.affine_transform = Some(Transform { c: 0.18, ..Transform::IDENTITY });
        let frame = doc.insert_node(parent, frame).unwrap();
        let mut group = Node::new(NodeKind::Frame, "Card", 60., 24., 42., 34.);
        group.flip_y = true;
        group.constraints.horizontal = Constraint::Stretch;
        let group = doc.insert_node(frame, group).unwrap();
        let label = doc.insert_node(group, Node::new(NodeKind::Rectangle, "Badge", 6., 8., 15., 8.)).unwrap();
        let peer = doc.insert_node(1, Node::new(NodeKind::Rectangle, "Peer", 290., 42., 68., 75.)).unwrap();
        let mut editor = Editor::new(doc);
        let before = editor.document.to_json().unwrap();
        let world = editor.document.world_transform(label).unwrap();
        let corners = editor.document.nodes[&label].local_bounds().corners();
        editor.selection = vec![frame, peer];
        let mut session = editor.begin_multi_resize().unwrap();
        let pivot = session.bounds().center();
        let factors = Point::new(0.36, 1.15);
        session.preview(&mut editor, pivot, factors);
        assert!(editor.document.nodes[&group].width < 42.);
        assert!(session.preview_selection(&mut editor, pivot, factors, true));
        assert_eq!(editor.document.nodes[&group].width, 42.);
        for point in corners {
            same(world.apply(point), editor.document.world_transform(label).unwrap().apply(point));
        }
        assert!(session.commit(&mut editor));
        let after = editor.document.to_json().unwrap();
        assert_eq!(editor.history_len(), 1);
        assert!(editor.undo());
        assert_eq!(editor.document.to_json().unwrap(), before);
        assert!(editor.redo());
        assert_eq!(editor.document.to_json().unwrap(), after);
    }

    #[test]
    fn resizing_a_layout_viewport_can_hold_flow_contents_without_changing_their_sizing_modes() {
        let mut doc = Document::new("Auto-layout viewport");
        let mut frame = Node::new(NodeKind::Frame, "Row", 10., 20., 180., 70.);
        frame.layout = Some(AutoLayout::default());
        let frame = doc.insert_node(1, frame).unwrap();
        let mut child = Node::new(NodeKind::Rectangle, "Fill", 0., 0., 50., 30.);
        child.layout_child.width = SizingMode::Fill;
        let child = doc.insert_node(frame, child).unwrap();
        doc.layout_node(frame);
        let mut e = Editor::new(doc);
        let base = e.document.nodes[&child].bounds();
        let original_settings = e.document.nodes[&child].layout_child.clone();
        let origin = e.document.world_transform(child).unwrap().apply(Point::default());
        let before = e.document.to_json().unwrap();
        e.selection = vec![frame];
        let mut drag = e.begin_single_resize(frame).unwrap();
        let rect = Rect::new(35., 20., 155., 70.);
        assert!(drag.preview_layer(&mut e, rect, false));
        assert!(e.document.nodes[&child].width < base.width);
        assert!(drag.preview_layer(&mut e, rect, true));
        same(e.document.world_transform(child).unwrap().apply(Point::default()), origin);
        assert_eq!(e.document.nodes[&child].width, base.width);
        assert_eq!(e.document.nodes[&child].layout_child, original_settings);
        drag.cancel(&mut e);
        assert_eq!(e.document.to_json().unwrap(), before);
    }

    #[test]
    fn multi_resize_on_a_page_with_auto_layout_snapshots_only_affected_geometry() {
        let mut d = Document::new("Scoped multi resize");
        let first = d
            .insert_node(1, Node::new(NodeKind::Rectangle, "A", 0.0, 0.0, 80.0, 40.0))
            .unwrap();
        let second = d
            .insert_node(1, Node::new(NodeKind::Rectangle, "B", 100.0, 0.0, 80.0, 40.0))
            .unwrap();
        let mut other = Node::new(NodeKind::Frame, "Unrelated layout", 280.0, 0.0, 160.0, 80.0);
        other.layout = Some(AutoLayout::default());
        d.insert_node(1, other).unwrap();
        let mut e = Editor::new(d);
        e.selection = vec![first, first];
        assert!(e.multi_resize_bounds().is_none());
        e.selection = vec![first, second];
        let mut session = e.begin_multi_resize().unwrap();
        assert!(
            matches!(&e.pending.as_ref().unwrap().before, PendingSnapshot::Nodes { nodes,.. } if nodes.len()==2)
        );
        assert!(session.preview(&mut e, Point::default(), Point::new(2.0, 1.0)));
        assert!(session.commit(&mut e));
        assert!(e.undo.last().unwrap().node_only);
    }
}
