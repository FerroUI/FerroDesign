//! Option-click cycles layers. Crossing four screen DIPs changes that click
//! into a drag whose artwork is transient until the native pointer releases.
use super::*;
use ferroui_base::input::StandardCursorType;

pub(super) struct AltDrag {
    start: Point,
    start_screen: Point,
    current: Point,
    press_modifiers: KeyModifiers,
    hit: Option<NodeId>,
    original: Vec<NodeId>,
    page: NodeId,
    token: core::EditorRenderToken,
    active: bool,
    copy: bool,
    session: Option<core::LayoutTransferSession>,
    roots: Vec<NodeId>,
    bounds: Option<Rect>,
    target: Option<core::LayoutTransferTarget>,
    painter: RefCell<ScenePainter>,
}

impl DesignCanvas {
    pub(super) fn press_alt_drag(&self, world: Point, screen: Point, modifiers: KeyModifiers) -> bool {
        if self.tool.get() != Tool::Select
            || self.vector_node.get().is_some()
            || !modifiers.contains(KeyModifiers::ALT)
            || modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::META)
        {
            return false;
        }
        let hit = self.pick_with_modifiers(world, modifiers, false);
        self.begin_alt_drag(hit, world, screen, modifiers);
        true
    }

    fn begin_alt_drag(&self, hit: Option<NodeId>, world: Point, screen: Point, modifiers: KeyModifiers) {
        let editor = self.editor.borrow();
        if editor.has_active_transaction() {
            return;
        }
        *self.gesture.borrow_mut() = Gesture::AltDrag(Box::new(AltDrag {
            start: world,
            start_screen: screen,
            current: world,
            press_modifiers: modifiers,
            hit,
            original: editor.selection.clone(),
            page: editor.active_page,
            token: editor.render_change_token(),
            active: false,
            copy: true,
            session: None,
            roots: Vec::new(),
            bounds: None,
            target: None,
            painter: RefCell::default(),
        }));
    }

    pub(super) fn alt_drag_modifier(&self, event: &KeyEventArgs, pressed: bool) -> bool {
        let flag = match event.key {
            Key::LeftAlt | Key::RightAlt => KeyModifiers::ALT,
            Key::LeftShift | Key::RightShift => KeyModifiers::SHIFT,
            _ => return false,
        };
        let mut modifiers = event.key_modifiers;
        modifiers.set(flag, pressed);
        let mut world = None;
        if pressed
            && flag == KeyModifiers::ALT
            && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::META)
        {
            if let Some((id, origin, screen)) = self.drag_copy_origin() {
                self.cancel_layout_drag();
                self.begin_alt_drag(Some(id), origin, screen, modifiers);
                world = self
                    .selection_pointer
                    .get()
                    .map(|p| self.viewport.get().screen_to_world(p));
            }
        }
        let mut gesture = self.gesture.borrow_mut();
        let Gesture::AltDrag(drag) = &mut *gesture else {
            return false;
        };
        drag.update(self, world.unwrap_or(drag.current), modifiers);
        drop(gesture);
        self.invalidate_visual();
        true
    }
}

impl AltDrag {
    fn valid(&self, editor: &Editor) -> bool {
        editor.active_page == self.page
            && editor.selection == self.original
            && editor.render_change_token() == self.token
            && !editor.has_active_transaction()
    }

    pub(super) fn update(&mut self, canvas: &DesignCanvas, world: Point, modifiers: KeyModifiers) {
        self.current = world;
        let editor = canvas.editor.borrow();
        self.target = None;
        if !self.valid(&editor) {
            return;
        }
        let copy = modifiers.contains(KeyModifiers::ALT);
        if !self.active || self.copy != copy {
            if !self.active
                && canvas
                    .viewport
                    .get()
                    .world_to_screen(world)
                    .distance(self.start_screen)
                    < 4.0
            {
                return;
            }
            self.active = true;
            self.copy = copy;
            let Some(hit) = self.hit else { return };
            let selected = self
                .original
                .iter()
                .copied()
                .find(|id| *id == hit || editor.document.is_ancestor(*id, hit));
            let source = selected.map_or(std::slice::from_ref(&hit), |_| self.original.as_slice());
            let prepare = if copy {
                core::LayoutTransferSession::for_copy
            } else {
                core::LayoutTransferSession::new
            };
            self.session = prepare(&editor.document, source, selected.unwrap_or(hit), self.start).ok();
            self.roots = self
                .session
                .iter()
                .flat_map(|session| session.source_ids())
                .collect();
            self.bounds = self
                .roots
                .iter()
                .filter_map(|id| editor.document.world_bounds(*id))
                .reduce(Rect::union);
        }
        let Some(session) = &self.session else { return };
        let mut delta = world - self.start;
        if modifiers.contains(KeyModifiers::SHIFT) {
            if delta.x.abs() > delta.y.abs() {
                delta.y = 0.0;
            } else {
                delta.x = 0.0;
            }
        }
        if canvas.snap.get() {
            delta = Point::new(delta.x.round(), delta.y.round());
        }
        let Some(mut target) = session.target(self.start + delta) else {
            return;
        };
        canvas.snap_guides.set((None, None));
        canvas.layout_grids.borrow_mut().clear_snap();
        if canvas.snap.get() && target.kind != core::LayoutTransferKind::Flow {
            if let Some(bounds) = self.bounds {
                // A copy may align with its originals; both move modes exclude the
                // dragged subtree's own layout guides.
                let exclude = if copy { &[][..] } else { self.roots.as_slice() };
                delta = canvas.snap_move_delta(
                    &editor,
                    bounds,
                    delta,
                    modifiers.contains(KeyModifiers::SHIFT),
                    &self.roots,
                    exclude,
                );
                target.position = self.start + delta;
            }
        }
        self.target = Some(target);
        let cursor = if copy {
            StandardCursorType::DragCopy
        } else {
            StandardCursorType::DragMove
        };
        canvas.set_handle_feedback(Some(cursor), None);
    }

    pub(super) fn render(&self, ctx: &mut DrawingContext, canvas: &DesignCanvas, editor: &Editor, visible: Rect, scale: f64) {
        let Some(target) = self.target.filter(|_| self.copy && self.valid(editor)) else {
            return;
        };
        let delta = target.position - self.start;
        let transform = ctx.push_transform(Matrix::create_translation(delta.x, delta.y));
        let translated = Rect::new(visible.x - delta.x, visible.y - delta.y, visible.width, visible.height);
        if canvas.show_outlines.get() {
            canvas.outlines.borrow_mut().render(ctx, editor, translated, canvas.viewport.get().zoom,
                canvas.outline_hidden.get(), canvas.outline_bounds.get(), canvas.dark.get(), &self.roots);
        } else {
            let mut painter = self.painter.borrow_mut();
            painter.set_raster_scale(scale);
            painter.render_editor(ctx, editor, &self.roots, translated);
        }
        ctx.pop(transform);
    }

    pub(super) fn is_previewing(&self, editor: &Editor) -> bool {
        self.target.is_some() && self.valid(editor)
    }

    pub(super) fn draw_guides(&self, ctx: &mut DrawingContext, editor: &Editor, view: Viewport) {
        let Some(target) = self.target.filter(|_| self.valid(editor)) else {
            return;
        };
        let Some(session) = &self.session else { return };
        let delta = target.position - self.start;
        let outline = pen(0xFF0D99FF, 1.0);
        for corners in session.source_corners() {
            let points = corners.map(|p| ui(view.world_to_screen(p + delta)));
            for side in 0..4 {
                ctx.draw_line(&outline, points[side], points[(side + 1) % 4]);
            }
        }
        if target.kind == core::LayoutTransferKind::Flow {
            let start = ui(view.world_to_screen(target.marker.start));
            let end = ui(view.world_to_screen(target.marker.end));
            ctx.draw_line(&pen(0xFF0D99FF, 3.0), start, end);
        }
    }

    pub(super) fn finish(mut self, canvas: &DesignCanvas, world: Point, modifiers: KeyModifiers) {
        self.update(canvas, world, modifiers);
        canvas.snap_guides.set((None, None));
        canvas.layout_grids.borrow_mut().clear_snap();
        canvas.set_resize_hover(None);
        if !self.valid(&canvas.editor.borrow()) {
            return;
        }
        if !self.active {
            canvas.cycle_pointer_click(self.start, self.press_modifiers);
            return;
        }
        let (Some(session), Some(target)) = (&self.session, self.target) else {
            return;
        };
        let mut editor = canvas.editor.borrow_mut();
        let result = if self.copy {
            editor.commit_drag_copy(session, target).map(|_| ())
        } else {
            editor.selection = session.selection().to_vec();
            if session.preserves_drag_parents(&editor.document, target) && !session.has_layout_source() {
                let delta = target.position - self.start;
                editor.move_selection(delta.x, delta.y);
                Ok(())
            } else {
                editor.commit_layout_transfer(session, target).map(|_| ())
            }
        };
        if result.is_err() {
            editor.selection = self.original;
        }
        drop(editor);
        match result {
            Ok(()) => canvas.emit(CanvasEvent::Document),
            Err(error) => canvas.emit(CanvasEvent::Notice(error.to_string())),
        }
    }
}
