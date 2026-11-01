//! Placement is calculated from the original press throughout the gesture.
//! Modifier changes and coalesced release events therefore use the same geometry.
use super::*;

pub(super) struct ShapeDrag {
    pub(super) id: NodeId,
    pub(super) tool: Tool,
    press: Point,
    anchor: Point,
    previous: Point,
    dragging: bool,
}

impl DesignCanvas {
    pub(super) fn drawing_parent_at(&self, press: Point, tool: Tool) -> Option<NodeId> {
        if tool == Tool::Slice { return None; }
        let editor = self.editor.borrow();
        let doc = &editor.document;
        let roots = &doc.page(editor.active_page)?.children;
        let mut painter = self.painter.borrow_mut();
        let mut current = editor.hit_test_filtered(press, 0.0, |id| {
            painter.mask_contains(&editor, roots, id, press)
        });
        // Follow the frontmost visible layer. A page-level shape over a frame
        // must not silently put new artwork behind that shape.
        for _ in 0..core::MAX_DOCUMENT_DEPTH {
            let id = current?;
            let node = doc.nodes.get(&id)?;
            if doc.is_layer_destination(id)
                && (tool != Tool::Section || node.kind == NodeKind::Section)
                && doc
                    .world_transform(id)
                    .and_then(core::Transform::inverse)
                    .is_some_and(|inverse| node.contains_clip_point(inverse.apply(press)))
            {
                return Some(id);
            }
            current = node.parent;
        }
        None
    }

    pub(super) fn commit_created_layer(&self, editor: &mut Editor, id: NodeId) {
        // Keep the in-progress path and drag guides in world space. Adopt the
        // finished node before committing, so one undo removes the whole edit.
        if let Some((_, parent)) = self
            .creation_parent
            .take()
            .filter(|(created, _)| *created == id)
        {
            let doc = &mut editor.document;
            let inverse = doc
                .world_transform(parent)
                .and_then(core::Transform::inverse);
            if let (Some(inverse), Some(world)) = (inverse, doc.world_transform(id)) {
                if doc.is_layer_destination(parent) && doc.reparent(id, parent, usize::MAX).is_ok()
                {
                    doc.nodes
                        .get_mut(&id)
                        .unwrap()
                        .set_local_transform(inverse * world);
                    if doc.nodes[&parent].layout.is_some() {
                        doc.layout_all();
                    }
                }
            }
        }
        editor.commit_transaction();
    }
}

impl ShapeDrag {
    pub(super) fn new(id: NodeId, tool: Tool, press: Point, anchor: Point) -> Self {
        Self {
            id,
            tool,
            press,
            anchor,
            previous: press,
            dragging: false,
        }
    }

    pub(super) fn update(
        &mut self,
        canvas: &DesignCanvas,
        pointer: Point,
        modifiers: KeyModifiers,
        released: bool,
    ) {
        let zoom = canvas.viewport.get().zoom;
        let centered = modifiers.contains(KeyModifiers::ALT);
        let line = self.tool.kind() == NodeKind::Line;
        if self.dragging && canvas.space.get() {
            self.anchor = self.anchor + (pointer - self.previous);
        }
        self.previous = pointer;
        self.dragging |= self.press.distance(pointer) * zoom >= 3.0;
        let (start, end) = if self.dragging {
            let mut delta = canvas.snap_point(pointer) - self.anchor;
            let constrained = modifiers.contains(KeyModifiers::SHIFT) && self.tool != Tool::Text;
            if constrained {
                delta = constrained_delta(delta, line);
            }
            let mut end = self.anchor + delta;
            canvas.layout_grids.borrow_mut().clear_snap();
            if canvas.snap.get() {
                if let Some(snap) = canvas.layout_grids.borrow_mut().snap(
                    &canvas.editor.borrow(),
                    &[end],
                    5.0 / zoom,
                    &[self.id],
                    constrained.then_some(delta),
                ) {
                    end = end + snap.delta;
                }
            }
            let start = if centered {
                self.anchor - (end - self.anchor)
            } else {
                self.anchor
            };
            (start, end)
        } else if released {
            let size = match self.tool {
                Tool::Line | Tool::Arrow => Point::new(100.0, 0.0),
                Tool::Section => Point::new(400.0, 300.0),
                Tool::Text => Point::new(160.0, 36.0),
                _ => Point::new(100.0, 100.0),
            };
            let start = if centered {
                self.anchor - size * 0.5
            } else {
                self.anchor
            };
            (start, start + size)
        } else {
            return;
        };

        let bounds = Rect::from_points(start, end);
        let min_size = if line { 0.0 } else { 1.0 };
        let mut editor = canvas.editor.borrow_mut();
        let Some(node) = editor.document.nodes.get_mut(&self.id) else {
            return;
        };
        node.x = bounds.x;
        node.y = bounds.y;
        node.width = bounds.width.max(min_size);
        node.height = bounds.height.max(min_size);
        if line {
            // Keep the end marker at pointer-up for all four drag directions.
            node.flip_x = end.x < start.x;
            node.flip_y = end.y < start.y;
        }
        if self.tool == Tool::Text {
            node.text_auto_resize = if self.dragging {
                core::TextAutoResize::Fixed
            } else {
                core::TextAutoResize::WidthAndHeight
            };
        }
        editor.refresh_preview_nodes(&[self.id]);
    }
}

pub(super) fn constrained_delta(delta: Point, line: bool) -> Point {
    if line {
        // Choose the nearest of the eight compass directions. Project onto
        // that ray so horizontal/vertical lines have an exactly zero extent.
        let threshold = std::f64::consts::SQRT_2 - 1.0;
        if delta.y.abs() <= delta.x.abs() * threshold {
            return Point::new(delta.x, 0.0);
        }
        if delta.x.abs() <= delta.y.abs() * threshold {
            return Point::new(0.0, delta.y);
        }
        let extent = (delta.x.abs() + delta.y.abs()) * 0.5;
        return Point::new(extent.copysign(delta.x), extent.copysign(delta.y));
    }
    let extent = delta.x.abs().max(delta.y.abs());
    Point::new(extent.copysign(delta.x), extent.copysign(delta.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas(tool: Tool, press: Point) -> (Ref<DesignCanvas>, ShapeDrag) {
        let document = core::Document::new("Shape placement");
        let canvas = DesignCanvas::new(Rc::new(RefCell::new(Editor::new(document))));
        canvas.snap.set(false);
        let id = canvas.new_node(tool, press).unwrap();
        (canvas, ShapeDrag::new(id, tool, press, press))
    }

    #[test]
    fn click_tolerates_screen_jitter_and_release_can_be_the_first_drag_sample() {
        let start = Point::new(10.0, 20.0);
        let (canvas, mut drag) = canvas(Tool::Ellipse, start);
        canvas.viewport.set(Viewport {
            zoom: 2.0,
            ..canvas.viewport.get()
        });
        drag.update(&canvas, Point::new(11.0, 20.0), KeyModifiers::empty(), true);
        let mut editor = canvas.editor.borrow_mut();
        assert_eq!(editor.document.nodes[&drag.id].width, 100.0);
        assert_eq!(editor.document.nodes[&drag.id].height, 100.0);
        assert!(editor.commit_transaction());
        assert!(editor.undo());
        assert!(editor.document.nodes.is_empty());
        assert!(editor.redo());
        assert_eq!(editor.document.nodes[&drag.id].x, start.x);
    }

    #[test]
    fn shape_modifiers_are_live_and_invalidate_only_the_created_layer() {
        let center = Point::new(100.0, 50.0);
        let (canvas, mut drag) = canvas(Tool::Rectangle, center);
        let token = canvas.editor.borrow().render_change_token();
        let pointer = Point::new(80.0, 53.0);
        drag.update(
            &canvas,
            pointer,
            KeyModifiers::SHIFT | KeyModifiers::ALT,
            false,
        );
        {
            let editor = canvas.editor.borrow();
            let node = &editor.document.nodes[&drag.id];
            assert_eq!(
                (node.x, node.y, node.width, node.height),
                (80.0, 30.0, 40.0, 40.0)
            );
            assert_eq!(
                editor.render_changes_since(&token),
                core::EditorRenderChanges::Nodes {
                    nodes: vec![drag.id],
                    ancestors: vec![],
                }
            );
        }
        drag.update(&canvas, pointer, KeyModifiers::empty(), true);
        let editor = canvas.editor.borrow();
        let node = &editor.document.nodes[&drag.id];
        assert_eq!(
            (node.x, node.y, node.width, node.height),
            (80.0, 50.0, 20.0, 3.0)
        );
    }

    #[test]
    fn constrained_arrow_preserves_its_directed_endpoints_in_every_quadrant() {
        let start = Point::new(30.0, 30.0);
        for (delta, expected) in [
            ((29.0, -5.0), (29.0, 0.0)),
            ((-3.0, -23.0), (0.0, -23.0)),
            ((-9.0, 11.0), (-10.0, 10.0)),
            ((-11.0, -9.0), (-10.0, -10.0)),
        ] {
            let (canvas, mut drag) = canvas(Tool::Arrow, start);
            let pointer = start + Point::new(delta.0, delta.1);
            drag.update(&canvas, pointer, KeyModifiers::SHIFT, true);
            let editor = canvas.editor.borrow();
            let node = &editor.document.nodes[&drag.id];
            let transform = node.local_transform();
            assert_eq!(transform.apply(Point::default()), start);
            assert_eq!(
                transform.apply(Point::new(node.width, node.height)),
                start + Point::new(expected.0, expected.1),
            );
            assert_eq!(node.strokes[0].cap, core::StrokeCap::Arrow);
        }
    }

    #[test]
    fn creating_in_a_rotated_frame_keeps_world_geometry_and_rolls_back_membership() {
        use ferroui_base::{platform::IPlatformRenderInterface, FerroLocator};
        let scope = FerroLocator::enter_scope();
        FerroLocator::current_mutable()
            .bind::<dyn IPlatformRenderInterface>()
            .to_constant(Rc::new(ferroui_vello::PlatformRenderInterface::default()));
        let mut document = core::Document::new("Nested drawing");
        let page = document.pages[0].id;
        let mut frame = Node::new(NodeKind::Frame, "Rotated frame", 30.0, 40.0, 120.0, 120.0);
        frame.rotation = 31.0;
        let parent = document.insert_node(page, frame).unwrap();
        let canvas = DesignCanvas::new(Rc::new(RefCell::new(Editor::new(document))));
        canvas.snap.set(false);
        let start = Point::new(90.0, 100.0);
        let end = Point::new(56.0, 117.0);
        let id = canvas.new_node(Tool::Arrow, start).unwrap();
        let mut drag = ShapeDrag::new(id, Tool::Arrow, start, start);
        drag.update(&canvas, end, KeyModifiers::empty(), true);
        let mut editor = canvas.editor.borrow_mut();
        canvas.commit_created_layer(&mut editor, id);
        assert_eq!(editor.document.nodes[&id].parent, Some(parent));
        let world = editor.document.world_transform(id).unwrap();
        assert!(world.apply(Point::default()).distance(start) < 1e-8);
        assert!(world.apply(Point::new(34.0, 17.0)).distance(end) < 1e-8);
        assert_eq!(editor.history_len(), 1);
        assert!(editor.undo());
        assert!(editor.document.nodes[&parent].children.is_empty());
        assert!(editor.redo());
        assert_eq!(editor.document.nodes[&parent].children, [id]);
        scope.dispose();
    }

    #[test]
    fn text_click_and_drag_choose_the_native_text_sizing_modes() {
        let start = Point::new(20.0, 30.0);
        for (end, expected_mode) in [
            (start, core::TextAutoResize::WidthAndHeight),
            (Point::new(8.0, 64.0), core::TextAutoResize::Fixed),
        ] {
            let (canvas, mut drag) = canvas(Tool::Text, start);
            drag.update(&canvas, end, KeyModifiers::empty(), true);
            let editor = canvas.editor.borrow();
            let node = &editor.document.nodes[&drag.id];
            assert_eq!(node.text_auto_resize, expected_mode);
            if start != end {
                assert_eq!(
                    (node.x, node.y, node.width, node.height),
                    (8.0, 30.0, 12.0, 34.0)
                );
            }
        }
    }
}
