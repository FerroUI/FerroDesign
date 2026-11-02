//! Native ruler chrome and a transient drag. Only pointer release edits the file.
use super::*;
use ferrodesign_core::{Guide, RulerGuideId, Transform};

const WIDTH: f64 = 20.0;
const HIT: f64 = 4.0;

#[derive(Clone)]
struct Selection {
    key: RulerGuideId,
    token: core::EditorRenderToken,
    page: NodeId,
}

struct Drag {
    pointer: i32,
    source: Option<RulerGuideId>,
    horizontal: bool,
    start: Point,
    grab_offset: Point,
    moved: bool,
    duplicate: bool,
    measuring: bool,
    pointer_world: Point,
    token: core::EditorRenderToken,
    page: NodeId,
    preview: Option<(NodeId, Guide)>,
}

#[derive(Default)]
pub(super) struct Rulers {
    selected: Option<Selection>,
    drag: Option<Drag>,
}

impl DesignCanvas {
    pub fn set_rulers_visible(&self, visible: bool) {
        self.show_rulers.set(visible);
        self.layout_grids.borrow_mut().rulers_visible = visible;
        if !visible {
            *self.rulers.borrow_mut() = Rulers::default();
        }
        self.invalidate_visual();
    }

    pub fn selected_ruler_guide(&self) -> Option<RulerGuideId> {
        let editor = self.editor.borrow();
        let rulers = self.rulers.borrow();
        let s = rulers.selected.as_ref()?;
        (s.page == editor.active_page
            && s.token == editor.render_change_token()
            && editor.document.ruler_guide(s.key).is_some())
        .then_some(s.key)
    }

    pub fn select_ruler_guide(&self, id: Option<RulerGuideId>) {
        let editor = self.editor.borrow();
        self.rulers.borrow_mut().selected = id.map(|key| Selection {
            key,
            token: editor.render_change_token(),
            page: editor.active_page,
        });
        self.invalidate_visual();
    }

    pub(super) fn guide_drag_active(&self) -> bool {
        self.rulers.borrow().drag.is_some()
    }

    pub(super) fn cancel_guide_drag(&self) -> bool {
        let cancelled = self.rulers.borrow_mut().drag.take().is_some();
        if cancelled {
            self.invalidate_visual();
        }
        cancelled
    }

    pub(super) fn forget_selected_ruler(&self) {
        if self.rulers.borrow_mut().selected.take().is_some() {
            self.invalidate_visual();
        }
    }
    pub(super) fn lost_ruler_capture(&self, pointer: i32) -> bool {
        let owned = self
            .rulers
            .borrow()
            .drag
            .as_ref()
            .is_some_and(|d| d.pointer == pointer);
        owned && self.cancel_guide_drag()
    }

    pub(super) fn guide_at(&self, screen: Point) -> Option<RulerGuideId> {
        let e = self.editor.borrow();
        let v = self.viewport.get();
        let world = v.screen_to_world(screen);
        let near = Rect::new(world.x, world.y, 0., 0.).inflate(HIT / v.zoom);
        let mut nearest = None;
        let mut distance = HIT / v.zoom;
        for (owner, guides) in e.ruler_guides_in(near) {
            if e.document.effective_locked(owner) {
                continue;
            }
            for (index, &guide) in guides.iter().enumerate() {
                if let Some((a, b)) =
                    e.document
                        .ruler_guide_segment(e.active_page, owner, guide, near)
                {
                    let d = core::distance_to_segment(world, a, b);
                    if d <= distance {
                        distance = d;
                        nearest = Some(RulerGuideId { owner, index });
                    }
                }
            }
        }
        nearest
    }

    pub(super) fn press_ruler(
        &self,
        screen: Point,
        pointer: i32,
        modifiers: KeyModifiers,
        strip_only: bool,
    ) -> bool {
        if !self.show_rulers.get() || self.editor.borrow().has_active_transaction() {
            return false;
        }
        let top = screen.y < WIDTH;
        let left = screen.x < WIDTH;
        if strip_only != (top || left) {
            return false;
        }
        if top && left {
            return true;
        }
        let source = if strip_only {
            None
        } else {
            if self.tool.get() != Tool::Select || self.vector_node.get().is_some() {
                return false;
            }
            let Some(id) = self.guide_at(screen) else {
                return false;
            };
            Some(id)
        };
        let editor = self.editor.borrow();
        let preview = source.and_then(|id| {
            editor
                .document
                .ruler_guide(id)
                .copied()
                .map(|g| (id.owner, g))
        });
        let horizontal = preview.map_or(top, |(_, g)| g.horizontal);
        let world = self.viewport.get().screen_to_world(screen);
        let grab_offset = preview
            .and_then(|(owner, guide)| {
                let t = editor
                    .document
                    .ruler_owner_transform(editor.active_page, owner)?;
                let mut local = t.inverse()?.apply(world);
                if horizontal {
                    local.y = guide.position;
                } else {
                    local.x = guide.position;
                }
                Some(t.apply(local) - world)
            })
            .unwrap_or_default();
        self.rulers.borrow_mut().drag = Some(Drag {
            pointer,
            source,
            horizontal,
            start: screen,
            grab_offset,
            moved: false,
            duplicate: source.is_some() && modifiers.contains(KeyModifiers::ALT),
            measuring: modifiers.contains(KeyModifiers::ALT),
            pointer_world: world,
            page: editor.active_page,
            token: editor.render_change_token(),
            preview,
        });
        self.invalidate_visual();
        true
    }

    pub(super) fn move_ruler(&self, screen: Point, pointer: i32, modifiers: KeyModifiers) -> bool {
        let (horizontal, page, grab_offset) = {
            let mut rulers = self.rulers.borrow_mut();
            let Some(drag) = rulers.drag.as_mut() else {
                return false;
            };
            if drag.pointer != pointer {
                return true;
            }
            drag.measuring = modifiers.contains(KeyModifiers::ALT);
            drag.pointer_world = self.viewport.get().screen_to_world(screen);
            if drag.start.distance(screen) >= 3.0 {
                drag.moved = true;
            }
            if !drag.moved {
                return true;
            }
            if screen == drag.start {
                drag.preview = drag.source.and_then(|id| {
                    self.editor
                        .borrow()
                        .document
                        .ruler_guide(id)
                        .copied()
                        .map(|g| (id.owner, g))
                });
                self.invalidate_visual();
                return true;
            }
            (drag.horizontal, drag.page, drag.grab_offset)
        };
        let v = self.viewport.get();
        let preview = if screen.x >= WIDTH
            && screen.y >= WIDTH
            && screen.x < v.width
            && screen.y < v.height
        {
            let world = v.screen_to_world(screen);
            let owner = self
                .containing_parent_at(world, |editor, id| {
                    !editor.document.effective_locked(id)
                        && editor.document.ruler_owner_transform(page, id).is_some()
                })
                .unwrap_or(page);
            let e = self.editor.borrow();
            e.document
                .ruler_owner_transform(page, owner)
                .and_then(Transform::inverse)
                .map(|inv| {
                    let local = inv.apply(world + grab_offset);
                    let coordinate = if horizontal { local.y } else { local.x };
                    let mut position = if self.snap.get() {
                        coordinate.round()
                    } else {
                        coordinate
                    };
                    if let Some(node) = e.document.nodes.get(&owner) {
                        position =
                            position.clamp(0., if horizontal { node.height } else { node.width });
                    }
                    (
                        owner,
                        Guide {
                            horizontal,
                            position,
                        },
                    )
                })
        } else {
            None
        };
        if let Some(drag) = self.rulers.borrow_mut().drag.as_mut() {
            drag.preview = preview;
        }
        self.invalidate_visual();
        true
    }

    pub(super) fn release_ruler(
        &self,
        screen: Point,
        pointer: i32,
        modifiers: KeyModifiers,
    ) -> bool {
        if !self.move_ruler(screen, pointer, modifiers) {
            return false;
        }
        if self
            .rulers
            .borrow()
            .drag
            .as_ref()
            .is_some_and(|d| d.pointer != pointer)
        {
            return true;
        }
        let Some(drag) = self.rulers.borrow_mut().drag.take() else {
            return true;
        };
        if self.editor.borrow().active_page != drag.page
            || self.editor.borrow().render_change_token() != drag.token
        {
            self.invalidate_visual();
            return true;
        }
        if !drag.moved {
            self.select_ruler_guide(drag.source);
            return true;
        }
        let v = self.viewport.get();
        // A drop outside the canvas cancels; dropping back on a ruler deletes.
        if screen.x < 0. || screen.y < 0. || screen.x >= v.width || screen.y >= v.height {
            self.invalidate_visual();
            return true;
        }
        self.commit_ruler(drag.source, drag.preview, drag.duplicate);
        true
    }

    pub fn commit_ruler(
        &self,
        source: Option<RulerGuideId>,
        dest: Option<(NodeId, Guide)>,
        duplicate: bool,
    ) -> bool {
        let mut editor = self.editor.borrow_mut();
        let before = editor.revision;
        let result = editor.edit_ruler_guide(source, dest, duplicate);
        let changed = editor.revision != before;
        drop(editor);
        match result {
            Ok(selected) => {
                self.select_ruler_guide(selected);
                if changed {
                    self.emit(CanvasEvent::Document);
                }
                true
            }
            Err(err) => {
                self.emit(CanvasEvent::Notice(err.to_string()));
                self.invalidate_visual();
                false
            }
        }
    }

    pub(super) fn ruler_alt(&self, key: Key, pressed: bool) -> bool {
        if !matches!(key, Key::LeftAlt | Key::RightAlt) {
            return false;
        }
        let mut state = self.rulers.borrow_mut();
        let Some(drag) = state.drag.as_mut() else {
            return false;
        };
        drag.measuring = pressed;
        drop(state);
        self.invalidate_visual();
        true
    }

    pub(crate) fn ruler_redlines(&self, e: &Editor, view: Viewport) -> Option<core::RulerRedlines> {
        let state = self.rulers.borrow();
        let drag = state.drag.as_ref().filter(|d| {
            d.moved && d.measuring && d.page == e.active_page && d.token == e.render_change_token()
        })?;
        let (owner, guide) = drag.preview?;
        let selected = e.selection.last().copied();
        let hit = e.hit_test(drag.pointer_world, 0.);
        // Once a guide enters a frame, the artwork beneath the pointer takes
        // precedence. In empty canvas space the selected frame remains the target.
        let target = hit.filter(|&id| id != owner).or(selected).or(hit)?;
        e.document.ruler_distances(
            drag.page,
            owner,
            guide,
            target,
            drag.pointer_world,
            view.visible_rect(),
        )
    }

    pub(super) fn ruler_key(&self, e: &KeyEventArgs) -> bool {
        if e.key == Key::R && e.key_modifiers == KeyModifiers::SHIFT {
            self.set_rulers_visible(!self.show_rulers.get());
            return true;
        }
        if !self.show_rulers.get() || self.guide_drag_active() {
            return false;
        }
        let Some(id) = self.selected_ruler_guide() else {
            return false;
        };
        if e.key_modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::META | KeyModifiers::CONTROL)
        {
            return false;
        }
        match e.key {
            Key::Escape => {
                self.select_ruler_guide(None);
            }
            Key::Delete | Key::Back => {
                self.commit_ruler(Some(id), None, false);
            }
            Key::Enter => self.emit(CanvasEvent::Action("ruler-guide-settings")),
            Key::Up | Key::Down | Key::Left | Key::Right => {
                let guide = self
                    .editor
                    .borrow()
                    .document
                    .ruler_guide(id)
                    .copied()
                    .unwrap();
                let sign = match (guide.horizontal, e.key) {
                    (true, Key::Down) | (false, Key::Right) => 1.,
                    (true, Key::Up) | (false, Key::Left) => -1.,
                    _ => return true,
                };
                let step = self.nudge.get().step(e.key_modifiers.contains(KeyModifiers::SHIFT));
                let mut position = guide.position + sign * step;
                if let Some(node) = self.editor.borrow().document.nodes.get(&id.owner) {
                    position = position.clamp(
                        0.,
                        if guide.horizontal {
                            node.height
                        } else {
                            node.width
                        },
                    );
                }
                self.commit_ruler(
                    Some(id),
                    Some((id.owner, Guide { position, ..guide })),
                    false,
                );
            }
            _ => return false,
        }
        true
    }

    pub(super) fn render_ruler_guides(&self, ctx: &mut DrawingContext, e: &Editor, v: Viewport) {
        if !self.show_rulers.get() {
            return;
        }
        let state = self.rulers.borrow();
        let selected = state
            .selected
            .as_ref()
            .filter(|s| s.page == e.active_page && s.token == e.render_change_token())
            .map(|s| s.key);
        let drag = state
            .drag
            .as_ref()
            .filter(|d| d.page == e.active_page && d.token == e.render_change_token() && d.moved);
        let normal = pen(0xE6E64B68, 1.);
        let active = pen(0xFF0D99FF, 1.5);
        let draw = |ctx: &mut DrawingContext, owner, guide, highlight| {
            if let Some((a, b)) =
                e.document
                    .ruler_guide_segment(e.active_page, owner, guide, v.visible_rect())
            {
                ctx.draw_line(
                    if highlight { &active } else { &normal },
                    ui(v.world_to_screen(a)),
                    ui(v.world_to_screen(b)),
                );
            }
        };
        for (owner, guides) in e.ruler_guides_in(v.visible_rect()) {
            for (index, &g) in guides.iter().enumerate() {
                let key = RulerGuideId { owner, index };
                if drag.is_some_and(|d| !d.duplicate && d.source == Some(key)) {
                    continue;
                }
                draw(ctx, owner, g, Some(key) == selected);
            }
        }
        if let Some((owner, g)) = drag.and_then(|d| d.preview) {
            draw(ctx, owner, g, true);
        }
    }

    pub(super) fn render_rulers(&self, ctx: &mut DrawingContext, e: &Editor, v: Viewport) {
        if !self.show_rulers.get() || v.width <= WIDTH || v.height <= WIDTH {
            return;
        }
        let stock = brush("#F5F5F5");
        let tint = brush("#D9EDFF");
        let tick = pen(0xFF939393, 1.);
        let border = pen(0xFFCBCBCB, 1.);
        let bounds = e.selection_bounds();
        let world = v.visible_rect();
        let major = ruler_step(v.zoom);
        for horizontal in [true, false] {
            let span = if horizontal { v.width } else { v.height };
            let rect = if horizontal {
                UiRect::new(0., 0., span, WIDTH)
            } else {
                UiRect::new(0., 0., WIDTH, span)
            };
            ctx.draw_rectangle(Some(&stock), None, rect, 0., 0., &BoxShadows::default());
            if let Some(b) = bounds {
                let a = v.world_to_screen(Point::new(b.x, b.y));
                let z = v.world_to_screen(Point::new(b.right(), b.bottom()));
                let (lo, hi) = if horizontal { (a.x, z.x) } else { (a.y, z.y) };
                let (lo, hi) = (lo.max(WIDTH), hi.min(span));
                if hi > lo {
                    let range = if horizontal {
                        UiRect::new(lo, 0., hi - lo, WIDTH)
                    } else {
                        UiRect::new(0., lo, WIDTH, hi - lo)
                    };
                    ctx.draw_rectangle(Some(&tint), None, range, 0., 0., &BoxShadows::default());
                }
            }
            let origin = if horizontal { v.pan.x } else { v.pan.y };
            let visible_start = if horizontal { world.x } else { world.y };
            let minor = major / 5.;
            let first = (visible_start / minor).floor();
            let count = (span / (minor * v.zoom)).ceil() as usize + 2;
            for i in 0..count {
                let index = first + i as f64;
                let pos = origin + index * minor * v.zoom;
                if pos < WIDTH || pos >= span {
                    continue;
                }
                let big = index.rem_euclid(5.) == 0.;
                let len = if big { 7. } else { 3. };
                let (a, b) = if horizontal {
                    (Point::new(pos, WIDTH - len), Point::new(pos, WIDTH))
                } else {
                    (Point::new(WIDTH - len, pos), Point::new(WIDTH, pos))
                };
                ctx.draw_line(&tick, ui(a), ui(b));
                if big {
                    let precision = (-major.log10().floor()).clamp(0., 6.) as usize;
                    let text = format!("{:.*}", precision, index * minor);
                    if horizontal {
                        label(ctx, &text, Point::new(pos + 2., 2.), 9., "#505050");
                    } else {
                        let turn = ctx.push_transform(
                            Matrix::create_rotation(-std::f64::consts::FRAC_PI_2)
                                * Matrix::create_translation(2., pos - 2.),
                        );
                        label(ctx, &text, Point::default(), 9., "#505050");
                        ctx.pop(turn);
                    }
                }
            }
            if horizontal {
                ctx.draw_line(
                    &border,
                    ui(Point::new(WIDTH, WIDTH)),
                    ui(Point::new(span, WIDTH)),
                );
            } else {
                ctx.draw_line(
                    &border,
                    ui(Point::new(WIDTH, WIDTH)),
                    ui(Point::new(WIDTH, span)),
                );
            }
        }
        ctx.draw_rectangle(
            Some(&stock),
            None,
            UiRect::new(0., 0., WIDTH, WIDTH),
            0.,
            0.,
            &BoxShadows::default(),
        );
        if let Some((owner, g)) = self
            .rulers
            .borrow()
            .drag
            .as_ref()
            .filter(|d| d.moved)
            .and_then(|d| d.preview)
        {
            let Some((a, b)) = e
                .document
                .ruler_guide_segment(e.active_page, owner, g, world)
            else {
                return;
            };
            let mid = v.world_to_screen(a.lerp(b, 0.5));
            let value = format!("{} {:.1}", if g.horizontal { "Y" } else { "X" }, g.position);
            let w = value.len() as f64 * 7.0 + 10.;
            let x = (mid.x + 5.).clamp(WIDTH, (v.width - w).max(WIDTH));
            let y = (mid.y - 22.).clamp(WIDTH, (v.height - 20.).max(WIDTH));
            ctx.draw_rectangle(
                Some(&brush("#096DAE")),
                None,
                UiRect::new(x, y, w, 19.),
                3.,
                3.,
                &BoxShadows::default(),
            );
            label(ctx, &value, Point::new(x + 5., y + 2.), 10., "#FFFFFF");
        }
    }
}

fn ruler_step(zoom: f64) -> f64 {
    let desired = 64. / zoom;
    let unit = 10.0_f64.powf(desired.log10().floor());
    [1., 2., 5., 10.]
        .into_iter()
        .map(|n| n * unit)
        .find(|n| *n >= desired)
        .unwrap_or(unit * 10.)
}
