use super::*;
use ferroui_base::input::StandardCursorType;

pub(super) struct MultiResize {
    handle: usize,
    origin: Point,
    screen: Point,
    version: core::EditorRenderToken,
    selected: Vec<NodeId>,
    session: Option<core::SelectionResize>,
    pub(super) visible: Rect,
    painted: bool,
    modifiers: KeyModifiers,
    clicks: i32,
}

impl DesignCanvas {
    pub(super) fn multi_resize_handle(&self, world: Point) -> Option<(usize, Rect)> {
        if self.tool.get() != Tool::Select
            || self.space.get()
            || self.vector_node.get().is_some()
            || self.image_crop.get().is_some()
            || self.presentation.get().is_some()
            || self.pen_node.get().is_some()
            || self.stroke_width_mode_active()
        {
            return None;
        }
        let editor = self.editor.borrow();
        if editor.selection.len() < 2 || editor.has_active_transaction() {
            return None;
        }
        let bounds = self.transform_box(&editor, true)?;
        let zoom = self.viewport.get().zoom;
        Some((
            scale::box_handle_at(bounds, world, 7.0 / zoom, 4.0 / zoom)?,
            bounds,
        ))
    }

    pub(super) fn multi_resize_hover(&self, world: Point) -> bool {
        let Some((handle, _)) = self.multi_resize_handle(world) else {
            return false;
        };
        let cursor = match handle {
            0 | 8 => StandardCursorType::TopLeftCorner,
            2 | 6 => StandardCursorType::TopRightCorner,
            1 | 7 => StandardCursorType::SizeNorthSouth,
            _ => StandardCursorType::SizeWestEast,
        };
        self.set_handle_feedback(Some(cursor), None);
        self.clear_measurement();
        self.clear_corner_radius_hover();
        if self.hover.take().is_some() {
            self.invalidate_visual();
        }
        true
    }

    pub(super) fn press_multi_resize(
        &self,
        world: Point,
        screen: Point,
        modifiers: KeyModifiers,
        clicks: i32,
    ) -> bool {
        let Some((handle, bounds)) = self.multi_resize_handle(world) else {
            return false;
        };
        let editor = self.editor.borrow();
        let painted = scale::anchor(bounds, handle).distance(world) * self.viewport.get().zoom <= 7.0;
        *self.gesture.borrow_mut() = Gesture::MultiResize(MultiResize {
            handle,
            origin: world,
            screen,
            version: editor.render_change_token(),
            selected: editor.selection.clone(),
            session: None,
            visible: bounds,
            painted,
            modifiers,
            clicks,
        });
        true
    }

    pub(super) fn multi_resize_modifier(&self, event: &KeyEventArgs, pressed: bool) -> bool {
        let flag = match event.key {
            Key::LeftAlt | Key::RightAlt => KeyModifiers::ALT,
            Key::LeftShift | Key::RightShift => KeyModifiers::SHIFT,
            _ => return false,
        };
        let mut gesture = self.gesture.borrow_mut();
        let Gesture::MultiResize(drag) = &mut *gesture else {
            return false;
        };
        if let Some(screen) = self.selection_pointer.get() {
            let mut mods = event.key_modifiers;
            mods.set(flag, pressed);
            drag.update(self, self.viewport.get().screen_to_world(screen), mods);
        }
        drop(gesture);
        self.invalidate_visual();
        true
    }
}

impl MultiResize {
    pub(super) fn update(&mut self, canvas: &DesignCanvas, world: Point, modifiers: KeyModifiers) {
        let mut editor = canvas.editor.borrow_mut();
        if self.session.is_none() {
            if self.screen.distance(canvas.viewport.get().world_to_screen(world)) < 3.0
                || self.version != editor.render_change_token()
                || self.selected != editor.selection
            {
                return;
            }
            self.session = editor.begin_multi_resize();
        }
        let Some(session) = &mut self.session else { return };
        let bounds = session.bounds();
        let centered = modifiers.contains(KeyModifiers::ALT);
        let pivot = scale::anchor(bounds, if centered { 4 } else { 8 - self.handle });
        let ray = scale::anchor(bounds, self.handle) - pivot;
        let delta = world - self.origin;
        let factor = |span: f64, axis: f64, motion: f64| {
            if axis == 0.0 || motion == 0.0 {
                return 1.0;
            }
            let extent = span * (1.0 + motion / axis);
            let extent = if canvas.snap.get() { extent.round() } else { extent };
            extent.max(span.min(1.0)) / span
        };
        let mut f = Point::new(
            factor(bounds.width, ray.x, delta.x),
            factor(bounds.height, ray.y, delta.y),
        );
        if modifiers.contains(KeyModifiers::SHIFT) {
            let uniform = if ray.x == 0.0 {
                f.y
            } else if ray.y == 0.0 {
                f.x
            } else {
                // Project onto the original diagonal, as the K tool does, so both inward
                // and outward strokes remain responsive near horizontal and vertical motion.
                let dot = delta.x * ray.x + delta.y * ray.y;
                (1.0 + dot / (ray.x * ray.x + ray.y * ray.y))
                    .max((1.0 / bounds.width.max(bounds.height)).min(1.0))
            };
            f = Point::new(uniform, uniform);
        }
        let mut grids = canvas.layout_grids.borrow_mut();
        grids.clear_snap();
        if canvas.snap.get() && f != Point::new(1.0, 1.0) {
            let point = pivot + Point::new(ray.x * f.x, ray.y * f.y);
            let uniform = modifiers.contains(KeyModifiers::SHIFT);
            let direction = (uniform || ray.x == 0.0 || ray.y == 0.0).then_some(ray);
            if let Some(snap) = grids.snap(
                &editor,
                &[point],
                5.0 / canvas.viewport.get().zoom,
                &self.selected,
                direction,
            ) {
                let mut corrected = f;
                if ray.x != 0.0 {
                    corrected.x += snap.delta.x / ray.x;
                }
                if ray.y != 0.0 {
                    corrected.y += snap.delta.y / ray.y;
                }
                if uniform {
                    let factor = if ray.x.abs() >= ray.y.abs() {
                        corrected.x
                    } else {
                        corrected.y
                    };
                    corrected = Point::new(factor, factor);
                }
                if bounds.width * corrected.x >= bounds.width.min(1.0)
                    && bounds.height * corrected.y >= bounds.height.min(1.0)
                {
                    f = corrected;
                } else {
                    grids.clear_snap();
                }
            }
        }
        drop(grids);
        if session.preview(&mut editor, pivot, f) {
            self.visible = Rect::new(
                pivot.x + (bounds.x - pivot.x) * f.x,
                pivot.y + (bounds.y - pivot.y) * f.y,
                bounds.width * f.x,
                bounds.height * f.y,
            );
        }
    }

    pub(super) fn cancel(self, canvas: &DesignCanvas) {
        if let Some(session) = self.session {
            session.cancel(&mut canvas.editor.borrow_mut());
        }
        canvas.set_resize_hover(None);
        canvas.layout_grids.borrow_mut().clear_snap();
        canvas.invalidate_visual();
        canvas.emit(CanvasEvent::Selection);
    }

    pub(super) fn finish(self, canvas: &DesignCanvas) {
        canvas.set_resize_hover(None);
        if let Some(session) = self.session {
            let changed = session.commit(&mut canvas.editor.borrow_mut());
            canvas.emit(if changed {
                CanvasEvent::Document
            } else {
                CanvasEvent::Selection
            });
        } else if !self.painted
            && self.version == canvas.editor.borrow().render_change_token()
            && self.selected == canvas.editor.borrow().selection
            && !canvas.cycle_pointer_click(self.origin, self.modifiers)
        {
            canvas.select_at_pointer(self.origin, self.modifiers, self.clicks, None);
        }
    }
}
