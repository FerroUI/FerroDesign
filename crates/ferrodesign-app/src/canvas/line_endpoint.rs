//! Direct line editing keeps stroke, arrow-cap and affine paint coordinates in
//! the layer's original local basis while moving its geometric endpoints.
use super::*;

pub(super) struct EndpointDrag {
    source: Node,
    inverse: core::Transform,
    start: Point,
    end: Point,
    press: Point,
    move_start: bool,
}

impl EndpointDrag {
    pub(super) fn new(
        editor: &Editor,
        source: Node,
        move_start: bool,
        press: Point,
    ) -> Option<Self> {
        let world = editor.document.world_transform(source.id)?;
        let end = world.apply(Point::new(source.width, source.height));
        Some(Self {
            source,
            inverse: world.inverse()?,
            start: world.apply(Point::default()),
            end,
            press,
            move_start,
        })
    }

    pub(super) fn update(&self, canvas: &DesignCanvas, pointer: Point, modifiers: KeyModifiers) {
        let centered = modifiers.contains(KeyModifiers::ALT);
        let (moving, fixed) = if self.move_start {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        };
        let anchor = if centered {
            (moving + fixed) * 0.5
        } else {
            fixed
        };
        let delta = pointer - self.press;
        let mut editor = canvas.editor.borrow_mut();
        if editor.document.effective_locked(self.source.id) {
            return;
        }
        if delta.distance(Point::default()) < 0.01 / canvas.viewport.get().zoom {
            // Preserve the source's exact reflection/affine decomposition when
            // clicking a handle or returning it to its original position.
            let Some(node) = editor.document.nodes.get_mut(&self.source.id) else {
                return;
            };
            if *node == self.source {
                return;
            }
            *node = self.source.clone();
        } else {
            let mut target = canvas.snap_point(moving + delta);
            let constrained = modifiers.contains(KeyModifiers::SHIFT);
            if constrained {
                target = anchor + drawing::constrained_delta(target - anchor, true);
            }
            canvas.layout_grids.borrow_mut().clear_snap();
            if canvas.snap.get() {
                if let Some(snap) = canvas.layout_grids.borrow_mut().snap(
                    &editor,
                    &[target],
                    5.0 / canvas.viewport.get().zoom,
                    &[self.source.id],
                    constrained.then_some(target - anchor),
                ) {
                    target = target + snap.delta;
                }
            }
            let opposite = if centered {
                anchor - (target - anchor)
            } else {
                fixed
            };
            let (start, end) = if self.move_start {
                (target, opposite)
            } else {
                (opposite, target)
            };
            let start = self.inverse.apply(start);
            let vector = self.inverse.apply(end) - start;
            let Some(node) = editor.document.nodes.get_mut(&self.source.id) else {
                return;
            };
            node.width = vector.x.abs();
            node.height = vector.y.abs();
            node.set_local_transform(
                self.source.local_transform()
                    * core::Transform::translation(start.x, start.y)
                    * core::Transform::scale(
                        1.0_f64.copysign(vector.x),
                        1.0_f64.copysign(vector.y),
                    ),
            );
        }
        editor.refresh_preview_nodes(&[self.source.id]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directed_line_keeps_the_opposite_endpoint_through_crossing_and_center_resize() {
        let mut document = core::Document::new("Endpoint manipulation");
        let mut line = Node::new(NodeKind::Line, "Arrow", 10.0, 20.0, 70.0, 0.0);
        line.strokes[0].cap = core::StrokeCap::Arrow;
        let id = document.insert_node(document.pages[0].id, line).unwrap();
        let canvas = DesignCanvas::new(Rc::new(RefCell::new(Editor::new(document))));
        canvas.snap.set(false);
        canvas.editor.borrow_mut().selection = vec![id];
        let press = Point::new(80.0, 21.0);
        assert_eq!(canvas.handle_at(press).unwrap().handle, resize::Handle::Corner(2));
        let source = canvas.editor.borrow().document.nodes[&id].clone();
        let drag = EndpointDrag::new(&canvas.editor.borrow(), source, false, press).unwrap();
        canvas
            .editor
            .borrow_mut()
            .begin_node_transaction(&[id], "Move line endpoint");
        for (mods, pointer, expected_start, expected_end) in [
            (
                KeyModifiers::SHIFT,
                (-20.0, 22.0),
                (10.0, 20.0),
                (-20.0, 20.0),
            ),
            (
                KeyModifiers::ALT | KeyModifiers::SHIFT,
                (47.0, 1.0),
                (45.0, 40.0),
                (45.0, 0.0),
            ),
        ] {
            drag.update(&canvas, Point::new(pointer.0, pointer.1), mods);
            let editor = canvas.editor.borrow();
            let node = &editor.document.nodes[&id];
            let world = node.local_transform();
            assert!(
                world
                    .apply(Point::default())
                    .distance(Point::new(expected_start.0, expected_start.1))
                    < 1e-8
            );
            assert!(
                world
                    .apply(Point::new(node.width, node.height))
                    .distance(Point::new(expected_end.0, expected_end.1))
                    < 1e-8
            );
            assert_eq!(node.strokes[0].cap, core::StrokeCap::Arrow);
        }
        drag.update(&canvas, press, KeyModifiers::empty());
        assert!(!canvas.editor.borrow_mut().commit_transaction());
    }
}
