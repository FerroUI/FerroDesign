//! Move-tool handles operate in the layer's local axes. Hit areas stay in screen DIPs.
use super::*;
use ferroui_base::input::{Cursor, StandardCursorType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Handle {
    Corner(usize),
    Edge(usize),
    Rotate,
}

impl Handle {
    fn axes(self) -> Point {
        match self {
            Self::Corner(i) => [
                Point::new(-1.0, -1.0), Point::new(1.0, -1.0),
                Point::new(1.0, 1.0), Point::new(-1.0, 1.0),
            ][i],
            Self::Edge(i) => [
                Point::new(0.0, -1.0), Point::new(1.0, 0.0),
                Point::new(0.0, 1.0), Point::new(-1.0, 0.0),
            ][i],
            Self::Rotate => Point::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Hit {
    pub(super) handle: Handle,
    id: NodeId,
    transform: core::Transform,
}

impl Hit {
    fn cursor(self) -> Option<StandardCursorType> {
        if self.handle == Handle::Rotate { return None; }
        let axis = self.transform.apply_vector(self.handle.axes());
        // The platform exposes four bidirectional resize cursors.
        let octant = (axis.y.atan2(axis.x) / std::f64::consts::FRAC_PI_4).round() as i32;
        Some(match octant.rem_euclid(4) {
            0 => StandardCursorType::SizeWestEast,
            1 => StandardCursorType::TopLeftCorner,
            2 => StandardCursorType::SizeNorthSouth,
            _ => StandardCursorType::TopRightCorner,
        })
    }
}

impl DesignCanvas {
    pub(super) fn set_resize_hover(&self, hit: Option<Hit>) {
        let edge = hit.and_then(|h| match h.handle {
            Handle::Edge(i) => Some((h.id, i)),
            _ => None,
        });
        self.set_handle_feedback(hit.and_then(Hit::cursor), edge);
    }

    pub(super) fn set_handle_feedback(&self, cursor: Option<StandardCursorType>, edge: Option<(NodeId, usize)>) {
        if self.manipulation_cursor.replace(cursor) != cursor {
            self.set_cursor(cursor.map(Cursor::new));
        }
        if self.hovered_resize_edge.replace(edge) != edge {
            self.invalidate_visual();
        }
    }

    pub(super) fn handle_at(&self, pointer: Point) -> Option<Hit> {
        if self.tool.get() != Tool::Select || self.space.get() || self.presentation.get().is_some()
            || self.vector_node.get().is_some() || self.pen_node.get().is_some()
            || self.image_crop.get().is_some() || self.stroke_width_mode_active() {
            return None;
        }
        let editor = self.editor.borrow();
        let &[id] = editor.selection.as_slice() else { return None; };
        let node = editor.document.nodes.get(&id)?;
        let transform = *editor.scene.transforms.get(&id)?;
        transform.inverse()?;
        if editor.document.effective_locked(id) || !editor.document.effective_visible(id) {
            return None;
        }
        let corners = node.local_bounds().corners().map(|p| transform.apply(p));
        let zoom = self.viewport.get().zoom;
        let hit = |handle| Some(Hit { handle, id, transform });
        let line = node.kind == NodeKind::Line;
        let candidates: &[usize] = if line { &[2, 0] } else { &[0, 1, 2, 3] };
        let nearest = candidates.iter().map(|&i| (i, corners[i].distance(pointer)))
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        if nearest.1 * zoom < 8.0 { return hit(Handle::Corner(nearest.0)); }
        if line { return None; }
        let top = (corners[0] + corners[1]) * 0.5;
        let center = (corners[0] + corners[2]) * 0.5;
        let direction = top - center;
        let length = direction.distance(Point::default()).max(0.001);
        if (top + direction * (24.0 / (zoom * length))).distance(pointer) * zoom < 8.0 {
            return hit(Handle::Rotate);
        }
        // A narrow edge target leaves the interior available for moving layers and editing radii.
        (0..4).map(|i| (i, core::distance_to_segment(pointer, corners[i], corners[(i + 1) % 4])))
            .filter(|(_, d)| d * zoom <= 4.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .and_then(|(i, _)| hit(Handle::Edge(i)))
    }
}

pub(super) struct ResizeDrag {
    id: NodeId,
    original: Rect,
    local: core::Transform,
    world: core::Transform,
    inverse: core::Transform,
    press: Point,
    axes: Point,
}

impl ResizeDrag {
    pub(super) fn new(node: &Node, hit: Hit, press: Point) -> Option<Self> {
        Some(Self { id: node.id, original: node.bounds(), local: node.local_transform(),
            world: hit.transform, inverse: hit.transform.inverse()?, press, axes: hit.handle.axes() })
    }

    fn anchor(&self, width: f64, height: f64, centered: bool) -> Point {
        let axes = if centered { Point::default() } else { self.axes };
        Point::new(width * (1.0 - axes.x) * 0.5, height * (1.0 - axes.y) * 0.5)
    }

    fn constrain(&self, size: Point) -> Point {
        let ratio = self.original.width.max(0.001) / self.original.height.max(0.001);
        if self.axes.y == 0.0 || (self.axes.x != 0.0 && size.x > size.y * ratio) {
            Point::new(size.x, size.x / ratio)
        } else {
            Point::new(size.y * ratio, size.y)
        }
    }

    pub(super) fn update(&self, canvas: &DesignCanvas, pointer: Point, modifiers: KeyModifiers) {
        let mut editor = canvas.editor.borrow_mut();
        canvas.layout_grids.borrow_mut().clear_snap();
        if editor.document.effective_locked(self.id) { return; }
        let delta = self.inverse.apply_vector(pointer - self.press);
        let centered = modifiers.contains(KeyModifiers::ALT);
        let constrained = modifiers.contains(KeyModifiers::SHIFT);
        let extent = if centered { 2.0 } else { 1.0 };
        let original = Point::new(self.original.width, self.original.height);
        let axes = self.axes;
        let original_anchor = self.anchor(original.x, original.y, centered);
        let mut size = original;
        // Use motion from the original press. Grabbing either side of the stroke cannot jump,
        // and a click/release preserves fractional authored sizes even with pixel snap enabled.
        if (delta.x * axes.x).abs().max((delta.y * axes.y).abs()) > 1e-9 {
            let snap_pixels = canvas.snap.get();
            let dimension = |start: f64, distance: f64, axis: f64| {
                if axis == 0.0 { return start; }
                let value = (start + axis * distance * extent).max(1.0);
                if snap_pixels { value.round() } else { value }
            };
            size = Point::new(dimension(original.x, delta.x, axes.x),
                dimension(original.y, delta.y, axes.y));
            if constrained { size = self.constrain(size); }
            if snap_pixels {
                let offset = Point::new(axes.x * size.x / extent, axes.y * size.y / extent);
                let handle = self.world.apply(original_anchor + offset);
                let direction = (constrained || axes.x == 0.0 || axes.y == 0.0)
                    .then(|| self.world.apply_vector(offset));
                let mut grids = canvas.layout_grids.borrow_mut();
                if let Some(snap) = grids.snap(&editor, &[handle], 5.0 / canvas.viewport.get().zoom, &[self.id], direction) {
                    let local = self.inverse.apply_vector(snap.delta);
                    let adjusted = Point::new(size.x + local.x * axes.x * extent,
                        size.y + local.y * axes.y * extent);
                    if (axes.x == 0.0 || adjusted.x >= 1.0) && (axes.y == 0.0 || adjusted.y >= 1.0) {
                        size = if constrained { self.constrain(adjusted) } else { adjusted };
                    } else {
                        grids.clear_snap();
                    }
                }
            }
        }
        let Some(node) = editor.document.nodes.get(&self.id) else { return; };
        let position = if size == original {
            Point::new(self.original.x, self.original.y)
        } else {
            let zero = Rect::new(0.0, 0.0, size.x, size.y);
            self.local.apply(original_anchor)
                - node.local_transform_for_bounds(zero).apply(self.anchor(size.x, size.y, centered))
        };
        if node.bounds() != Rect::new(position.x, position.y, size.x, size.y) {
            editor.resize_node_and_position(self.id, size.x, size.y, position);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_edges_preserve_affine_anchors_and_exact_unedited_dimensions() {
        let mut doc = core::Document::new("Transformed edge drags");
        let mut parent = Node::new(NodeKind::Frame, "Parent", 110.0, 50.0, 450.0, 310.0);
        parent.rotation = 23.0;
        parent.affine_transform = Some(core::Transform { c: 0.35, ..core::Transform::IDENTITY });
        let parent = doc.insert_node(1, parent).unwrap();
        let mut layer = Node::new(NodeKind::Rectangle, "Layer", 35.0, 42.0, 64.5, 32.25);
        layer.rotation = -11.0;
        layer.flip_x = true;
        let id = doc.insert_node(parent, layer).unwrap();
        let original = doc.nodes[&id].clone();
        let transform = doc.world_transform(id).unwrap();
        let canvas = DesignCanvas::new(Rc::new(RefCell::new(Editor::new(doc))));
        canvas.editor.borrow_mut().selection = vec![id];
        canvas.viewport.set(Viewport { zoom: 2.0, ..Viewport::default() });
        canvas.snap.set(false);
        for edge in 0..4 {
            let axes = Handle::Edge(edge).axes();
            let grab = original.local_bounds().center() + Point::new(
                original.width * axes.x * 0.5, original.height * axes.y * 0.5);
            let world = transform.apply(grab);
            let hit = canvas.handle_at(world).unwrap();
            assert_eq!(hit.handle, Handle::Edge(edge));
            let drag = ResizeDrag::new(&original, hit, world).unwrap();
            let delta = Point::new(if axes.x == 0.0 { 13.0 } else { 10.0 * axes.x },
                if axes.y == 0.0 { 11.0 } else { 10.0 * axes.y });
            for mods in [KeyModifiers::empty(), KeyModifiers::SHIFT, KeyModifiers::ALT,
                KeyModifiers::SHIFT | KeyModifiers::ALT] {
                let centered = mods.contains(KeyModifiers::ALT);
                let mut size = Point::new(original.width, original.height);
                if axes.x != 0.0 { size.x += if centered { 20.0 } else { 10.0 }; }
                if axes.y != 0.0 { size.y += if centered { 20.0 } else { 10.0 }; }
                if mods.contains(KeyModifiers::SHIFT) {
                    if axes.x != 0.0 { size.y = size.x / 2.0; } else { size.x = size.y * 2.0; }
                }
                let anchor = transform.apply(drag.anchor(original.width, original.height, centered));
                let mut editor = canvas.editor.borrow_mut();
                editor.begin_node_transaction(&[id], "Resize");
                drop(editor);
                drag.update(&canvas, transform.apply(grab + delta), mods);
                let editor = canvas.editor.borrow();
                let current = &editor.document.nodes[&id];
                assert!((size.x - current.width).abs() < 1e-8);
                assert!((size.y - current.height).abs() < 1e-8);
                let new_anchor = drag.anchor(current.width, current.height, centered);
                assert!(anchor.distance(editor.document.world_transform(id).unwrap().apply(new_anchor)) < 1e-8);
                drop(editor);
                canvas.editor.borrow_mut().cancel_transaction();
            }
        }
        assert_eq!(canvas.editor.borrow().history_len(), 0);
        canvas.editor.borrow_mut().update_node(parent, "Lock parent", |n| n.locked = true);
        assert!(canvas.handle_at(transform.apply(Point::new(0.0, original.height / 2.0))).is_none());
    }

    #[test]
    fn resize_edge_hit_width_and_directional_cursor_follow_the_screen() {
        let mut doc = core::Document::new("Resize cursor");
        let mut layer = Node::new(NodeKind::Rectangle, "Rotated", 200.0, 180.0, 128.5, 60.25);
        layer.rotation = 90.0;
        let transform = layer.local_transform();
        let id = doc.insert_node(1, layer).unwrap();
        let canvas = DesignCanvas::new(Rc::new(RefCell::new(Editor::new(doc))));
        canvas.editor.borrow_mut().selection = vec![id];
        for zoom in [0.5, 1.0, 4.0] {
            canvas.viewport.set(Viewport { zoom, ..Viewport::default() });
            let right = transform.apply(Point::new(128.5 + 3.0 / zoom, 16.0));
            let hit = canvas.handle_at(right).unwrap();
            assert_eq!(hit.handle, Handle::Edge(1));
            assert_eq!(hit.cursor(), Some(StandardCursorType::SizeNorthSouth));
            assert!(canvas.handle_at(transform.apply(Point::new(128.5 + 5.0 / zoom, 24.0))).is_none());
            assert_eq!(canvas.handle_at(transform.apply(Point::new(128.5, 0.0))).unwrap().handle,
                Handle::Corner(1));
        }
    }
}
