//! Selection rotation and its editor-only origin. World geometry drives the
//! transform; target, handle, and hit tolerances stay in screen DIPs.
use super::*;
use ferroui_base::input::StandardCursorType;

#[derive(Default)]
pub(super) struct RotationState {
    shown: Cell<bool>,
    cache: RefCell<Option<Geometry>>,
    origin: RefCell<Option<Origin>>,
}

struct Geometry {
    token: core::EditorRenderToken,
    page: NodeId,
    selected: Vec<NodeId>,
    bounds: Option<core::RotationBounds>,
    orbit_center: Option<Point>,
}

#[derive(Clone)]
struct Origin {
    token: core::EditorRenderToken,
    page: NodeId,
    selected: Vec<NodeId>,
    layer: NodeId,
    relative: Point,
}

impl Origin {
    fn belongs(&self, e: &Editor) -> bool {
        e.active_page == self.page && self.selected == e.selection && e.owns_render_token(&self.token)
    }
}

pub(super) struct OriginDrag {
    before: Option<Origin>,
    bounds: core::RotationBounds,
    start: Point,
    press: Point,
    token: core::EditorRenderToken,
    selected: Vec<NodeId>,
}

pub(super) struct Drag {
    original: core::RotationBounds,
    pivot: Point,
    press: Point,
    start_angle: f64,
    screen: Point,
    token: core::EditorRenderToken,
    selected: Vec<NodeId>,
    session: Option<core::SelectionRotation>,
    delta: f64,
    painted: bool,
    modifiers: KeyModifiers,
    clicks: i32,
}

#[derive(Clone, Copy)]
enum Hit {
    Origin,
    Grip,
    Outside,
}

fn geometry_handle(c: [Point; 4], zoom: f64) -> (Point, Point) {
    let top = (c[0] + c[1]) * 0.5;
    let mut outward = top - (c[0] + c[2]) * 0.5;
    if outward.distance(Point::default()) < 1e-9 {
        let edge = c[1] - c[0];
        outward = Point::new(edge.y, -edge.x);
    }
    let norm = outward.distance(Point::default());
    if norm < 1e-9 {
        outward = Point::new(0.0, -1.0);
    } else {
        outward = outward * (1.0 / norm);
    }
    (top, top + outward * (24.0 / zoom))
}

fn outside_corner(c: [Point; 4], world: Point, zoom: f64) -> bool {
    (0..4).any(|i| {
        let delta = world - c[i];
        let distance = delta.distance(Point::default()) * zoom;
        let left = c[(i + 3) % 4] - c[i];
        let right = c[(i + 1) % 4] - c[i];
        (8.0..=19.0).contains(&distance)
            && delta.x * left.x + delta.y * left.y < 0.0
            && delta.x * right.x + delta.y * right.y < 0.0
    })
}

impl RotationState {
    fn geometry(&self, e: &Editor) -> Option<core::RotationBounds> {
        let token = e.render_change_token();
        let mut cached = self.cache.borrow_mut();
        if let Some(old) = cached.as_ref() {
            if old.token == token && old.page == e.active_page && old.selected == e.selection {
                return old.bounds;
            }
        }
        if self
            .origin
            .borrow()
            .as_ref()
            .is_some_and(|origin| !origin.belongs(e))
        {
            self.origin.replace(None);
        }
        let bounds = e.rotation_bounds();
        *cached = Some(Geometry {
            token,
            page: e.active_page,
            selected: e.selection.clone(),
            bounds,
            orbit_center: None,
        });
        bounds
    }

    fn default_pivot(&self, b: core::RotationBounds) -> Point {
        self.cache
            .borrow()
            .as_ref()
            .and_then(|entry| entry.orbit_center)
            .unwrap_or_else(|| b.center())
    }

    // An asymmetric selection's axis-aligned bounds can drift as it turns.
    // Consecutive turns keep their original center until a different edit
    // or selection invalidates the geometry cache.
    fn finish_turn(&self, editor: &Editor, pivot: Point) {
        if editor.selection.len() > 1 && self.origin.borrow().is_none() {
            self.geometry(editor);
            if let Some(entry) = self.cache.borrow_mut().as_mut() {
                entry.orbit_center = Some(pivot);
            }
        }
    }

    fn pivot(&self, e: &Editor, b: core::RotationBounds) -> Point {
        let origin = self.origin.borrow();
        let Some(o) = origin
            .as_ref()
            .filter(|o| b.free_origin && o.belongs(e) && o.layer == b.anchor_layer)
        else {
            return self.default_pivot(b);
        };
        let n = &e.document.nodes[&o.layer];
        let extent = |v| if v == 0.0 { 1.0 } else { v };
        e.scene.transforms[&o.layer].apply(Point::new(
            o.relative.x * extent(n.width),
            o.relative.y * extent(n.height),
        ))
    }

    fn set_pivot(&self, e: &Editor, b: core::RotationBounds, world: Point) {
        if world == self.default_pivot(b) {
            self.origin.replace(None);
            return;
        }
        let Some(inverse) = e.scene.transforms[&b.anchor_layer].inverse() else {
            return;
        };
        let p = inverse.apply(world);
        let n = &e.document.nodes[&b.anchor_layer];
        let relative = Point::new(
            if n.width == 0.0 { p.x } else { p.x / n.width },
            if n.height == 0.0 { p.y } else { p.y / n.height },
        );
        if !relative.x.is_finite() || !relative.y.is_finite() {
            return;
        }
        let mut slot = self.origin.borrow_mut();
        if let Some(o) = slot
            .as_mut()
            .filter(|o| o.layer == b.anchor_layer && o.belongs(e))
        {
            o.relative = relative;
            return;
        }
        *slot = Some(Origin {
            token: e.render_change_token(),
            page: e.active_page,
            selected: e.selection.clone(),
            layer: b.anchor_layer,
            relative,
        });
    }
}

impl DesignCanvas {
    fn rotation_available(&self) -> bool {
        self.tool.get() == Tool::Select
            && !self.space.get()
            && self.presentation.get().is_none()
            && self.vector_node.get().is_none()
            && self.pen_node.get().is_none()
            && self.image_crop.get().is_none()
            && !self.stroke_width_mode_active()
            && !self.linear_gradient_active()
    }

    fn rotation_hit(&self, world: Point) -> Option<(Hit, core::RotationBounds, Point)> {
        if !self.rotation_available() {
            return None;
        }
        let e = self.editor.borrow();
        if e.has_active_transaction() {
            return None;
        }
        let bounds = self.rotation.geometry(&e)?;
        let pivot = self.rotation.pivot(&e, bounds);
        let zoom = self.viewport.get().zoom;
        let hit = if self.rotation.shown.get() && bounds.free_origin && world.distance(pivot) * zoom <= 8.0 {
            Hit::Origin
        } else if geometry_handle(bounds.corners, zoom).1.distance(world) * zoom <= 7.0 {
            Hit::Grip
        } else if outside_corner(bounds.corners, world, zoom) {
            Hit::Outside
        } else {
            return None;
        };
        Some((hit, bounds, pivot))
    }

    pub(super) fn hover_rotation(&self, world: Point) -> bool {
        let Some((hit, _, _)) = self.rotation_hit(world) else {
            return false;
        };
        self.set_handle_feedback(
            Some(match hit {
                Hit::Origin => StandardCursorType::Cross,
                _ => StandardCursorType::Hand,
            }),
            None,
        );
        self.clear_measurement();
        self.clear_corner_radius_hover();
        if self.hover.take().is_some() {
            self.invalidate_visual();
        }
        true
    }

    pub(super) fn press_rotation(
        &self,
        world: Point,
        screen: Point,
        modifiers: KeyModifiers,
        clicks: i32,
    ) -> bool {
        let Some((hit, bounds, pivot)) = self.rotation_hit(world) else {
            return false;
        };
        let e = self.editor.borrow();
        let token = e.render_change_token();
        let selected = e.selection.clone();
        *self.gesture.borrow_mut() = match hit {
            Hit::Origin => Gesture::RotationOrigin(OriginDrag {
                before: self.rotation.origin.borrow().clone(),
                bounds,
                start: pivot,
                press: world,
                token,
                selected,
            }),
            Hit::Grip | Hit::Outside => {
                let axis = bounds.corners[1] - bounds.corners[0];
                Gesture::Rotate(Drag {
                    original: bounds,
                    pivot,
                    press: world,
                    start_angle: axis.y.atan2(axis.x).to_degrees(),
                    screen,
                    token,
                    selected,
                    session: None,
                    delta: 0.0,
                    painted: matches!(hit, Hit::Grip),
                    modifiers,
                    clicks,
                })
            }
        };
        self.invalidate_visual();
        true
    }

    pub(super) fn rotation_key(&self, e: &KeyEventArgs) -> bool {
        let alt = e.key_modifiers.contains(KeyModifiers::ALT);
        let cmd = e
            .key_modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::META);
        if e.key == Key::Escape
            && e.key_modifiers.is_empty()
            && self.rotation.shown.get()
            && self.rotation_available()
            && matches!(*self.gesture.borrow(), Gesture::None)
        {
            self.rotation.shown.set(false);
            self.invalidate_visual();
            return true;
        }
        let toggle = (e.key == Key::R || e.physical_key == ferroui_base::input::PhysicalKey::R)
            && e.key_modifiers == KeyModifiers::ALT;
        let keyboard = alt && cmd && matches!(e.key, Key::Up | Key::Down);
        if !toggle && !keyboard {
            return false;
        }
        if !self.rotation_available() || !matches!(*self.gesture.borrow(), Gesture::None) {
            return true;
        }
        if toggle {
            self.rotation.shown.set(!self.rotation.shown.get());
            self.clear_measurement();
            self.invalidate_visual();
        } else {
            let mut editor = self.editor.borrow_mut();
            let Some(b) = self.rotation.geometry(&editor) else {
                return true;
            };
            let pivot = self.rotation.pivot(&editor, b);
            let step = if e.key_modifiers.contains(KeyModifiers::SHIFT) {
                15.0
            } else {
                1.0
            };
            let degrees = if e.key == Key::Up { -step } else { step };
            let changed = editor.rotate_selection_about(Some(pivot), degrees);
            if changed {
                self.rotation.finish_turn(&editor, pivot);
            }
            drop(editor);
            if changed {
                self.invalidate_visual();
                self.emit(CanvasEvent::Document);
            }
        }
        true
    }

    pub(super) fn rotation_modifier(&self, event: &KeyEventArgs, pressed: bool) -> bool {
        if !matches!(event.key, Key::LeftShift | Key::RightShift) {
            return false;
        }
        let mut g = self.gesture.borrow_mut();
        let Gesture::Rotate(d) = &mut *g else { return false };
        let old = self.editor.borrow().revision;
        if let Some(screen) = self.selection_pointer.get() {
            let mut mods = event.key_modifiers;
            mods.set(KeyModifiers::SHIFT, pressed);
            d.update(self, self.viewport.get().screen_to_world(screen), mods);
        }
        drop(g);
        if self.editor.borrow().revision != old {
            self.emit(CanvasEvent::ResizePreview);
        }
        self.invalidate_visual();
        true
    }

    pub(super) fn render_rotation(&self, ctx: &mut DrawingContext, e: &Editor, v: Viewport, g: &Gesture) {
        if !self.rotation_available() {
            return;
        }
        let (corners, pivot, show_target, delta) = match g {
            Gesture::Rotate(d) => {
                let orbit = core::Transform::translation(d.pivot.x, d.pivot.y)
                    * core::Transform::rotation(d.delta)
                    * core::Transform::translation(-d.pivot.x, -d.pivot.y);
                (
                    d.original.corners.map(|p| orbit.apply(p)),
                    d.pivot,
                    true,
                    Some(d.delta),
                )
            }
            Gesture::None | Gesture::RotationOrigin(_) => {
                let Some(b) = self.rotation.geometry(e) else {
                    return;
                };
                (
                    b.corners,
                    self.rotation.pivot(e, b),
                    self.rotation.shown.get() && b.free_origin,
                    None,
                )
            }
            _ => return,
        };
        let outline = pen(0xFF0D99FF, 1.0);
        let white = brush("#FFFFFF");
        if delta.is_some() && e.selection.len() > 1 {
            let p = corners.map(|p| ui(v.world_to_screen(p)));
            for i in 0..4 {
                ctx.draw_line(&outline, p[i], p[(i + 1) % 4]);
            }
        }
        let (top, grip) = geometry_handle(corners, v.zoom);
        ctx.draw_line(&outline, ui(v.world_to_screen(top)), ui(v.world_to_screen(grip)));
        ctx.draw_ellipse_at(
            Some(&white),
            Some(&outline),
            ui(v.world_to_screen(grip)),
            3.5,
            3.5,
        );
        if show_target {
            let p = v.world_to_screen(pivot);
            let ink = pen(0xFF096DAE, 1.5);
            ctx.draw_ellipse_at(Some(&white), Some(&ink), ui(p), 5.0, 5.0);
            for axis in [Point::new(1., 0.), Point::new(0., 1.)] {
                for sign in [-1., 1.] {
                    ctx.draw_line(&ink, ui(p + axis * (3. * sign)), ui(p + axis * (8. * sign)));
                }
            }
        }
        if let Some(degrees) = delta {
            let b = corners
                .into_iter()
                .map(|p| {
                    let s = v.world_to_screen(p);
                    Rect::new(s.x, s.y, 0., 0.)
                })
                .reduce(Rect::union)
                .unwrap();
            self.draw_size_badge(ctx, resize::BadgeValue::Rotation(degrees), b, v);
        }
    }
}

impl OriginDrag {
    pub(super) fn update(&self, canvas: &DesignCanvas, p: Point, modifiers: KeyModifiers) {
        let e = canvas.editor.borrow();
        if self.selected != e.selection || self.token != e.render_change_token() {
            return;
        }
        if p == self.press {
            canvas.rotation.origin.replace(self.before.clone());
            return;
        }
        let mut point = self.start + (p - self.press);
        if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::META) {
            let c = self.bounds.corners;
            let anchors = [
                self.bounds.center(),
                c[0],
                c[1],
                c[2],
                c[3],
                (c[0] + c[1]) * 0.5,
                (c[1] + c[2]) * 0.5,
                (c[2] + c[3]) * 0.5,
                (c[3] + c[0]) * 0.5,
            ];
            if let Some(near) = anchors
                .into_iter()
                .min_by(|a, b| a.distance(point).total_cmp(&b.distance(point)))
                .filter(|a| a.distance(point) * canvas.viewport.get().zoom <= 6.)
            {
                point = near;
            }
        }
        canvas.rotation.set_pivot(&e, self.bounds, point);
    }

    pub(super) fn cancel(self, canvas: &DesignCanvas) {
        canvas.rotation.origin.replace(self.before);
        canvas.invalidate_visual();
    }
}

impl Drag {
    pub(super) fn update(&mut self, canvas: &DesignCanvas, world: Point, mods: KeyModifiers) {
        let mut e = canvas.editor.borrow_mut();
        if self.session.is_none() {
            if self.token != e.render_change_token()
                || self.selected != e.selection
                || canvas.viewport.get().world_to_screen(world).distance(self.screen) < 3.0
            {
                return;
            }
            self.session = e.begin_selection_rotation(Some(self.pivot));
        }
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let from = self.press - self.pivot;
        let to = world - self.pivot;
        if from.distance(Point::default()) < 1e-9 || to.distance(Point::default()) < 1e-9 {
            return;
        }
        let mut delta = (from.x * to.y - from.y * to.x)
            .atan2(from.x * to.x + from.y * to.y)
            .to_degrees();
        if mods.contains(KeyModifiers::SHIFT) {
            delta = ((self.start_angle + delta) / 15.0).round() * 15.0 - self.start_angle;
        }
        if session.preview(&mut e, delta) {
            self.delta = delta;
        }
    }
    pub(super) fn cancel(self, canvas: &DesignCanvas) {
        if let Some(s) = self.session {
            s.cancel(&mut canvas.editor.borrow_mut());
        }
        canvas.invalidate_visual();
        canvas.emit(CanvasEvent::Selection);
    }
    pub(super) fn finish(self, canvas: &DesignCanvas) {
        if let Some(s) = self.session {
            let changed = s.commit(&mut canvas.editor.borrow_mut());
            if changed {
                canvas.rotation.finish_turn(&canvas.editor.borrow(), self.pivot);
            }
            canvas.emit(if changed {
                CanvasEvent::Document
            } else {
                CanvasEvent::Selection
            });
        } else if !self.painted
            && self.selected == canvas.editor.borrow().selection
            && self.token == canvas.editor.borrow().render_change_token()
            && !canvas.cycle_pointer_click(self.press, self.modifiers)
        {
            canvas.select_at_pointer(self.press, self.modifiers, self.clicks, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_keyboard_turns_keep_the_shared_center_of_an_asymmetric_selection() {
        let mut doc = core::Document::new("Stable collective pivot");
        let a = doc
            .insert_node(1, Node::new(NodeKind::Rectangle, "A", 5., 11., 55., 20.))
            .unwrap();
        let b = doc
            .insert_node(1, Node::new(NodeKind::Rectangle, "B", 105., 90., 100., 14.))
            .unwrap();
        let mut e = Editor::new(doc);
        e.selection = vec![a, b];
        let pivot = e.rotation_bounds().unwrap().center();
        let point = e.document.world_origin(a).unwrap();
        let canvas = DesignCanvas::new(Rc::new(RefCell::new(e)));
        let mut event = KeyEventArgs::new();
        event.key = Key::Down;
        event.key_modifiers = KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT;
        for _ in 0..4 {
            assert!(canvas.rotation_key(&event));
        }
        let result = canvas.editor.borrow().document.world_origin(a).unwrap();
        assert!((pivot + core::Transform::rotation(60.).apply_vector(point - pivot)).distance(result) < 1e-8);
    }
}
