//! Inspector labels capture horizontal mouse/pen drags. The document remains
//! undoable as a single native field edit, including constraint and binding changes.
use super::geometry_fields::GeometryField;
use super::*;
use ferroui_base::input::{Cursor, IPointer, KeyModifiers, PointerType, StandardCursorType};
use ferroui_base::interactivity::RoutingStrategies;

pub(super) struct Scrub {
    name: &'static str,
    property: GeometryField,
    pointer: Rc<dyn IPointer>,
    host: crate::platform::ScrubPointer,
    origin: ferroui_base::Point,
    last_x: f64,
    gain: f64,
    delta: f64,
    page: NodeId,
    selection: Vec<NodeId>,
    baseline: BTreeMap<NodeId, f64>,
    version: core::EditorRenderToken,
    owner: Option<core::EditorTransactionToken>,
}

impl Scrub {
    fn matches(&self, editor: &Editor) -> bool {
        self.page == editor.active_page
            && self.selection == editor.selection
            && match &self.owner {
                Some(token) => editor.active_transaction_token().as_ref() == Some(token),
                None => {
                    !editor.has_active_transaction() && self.version == editor.render_change_token()
                }
            }
    }

    fn sample(
        &mut self,
        editor: &mut Editor,
        p: ferroui_base::Point,
        keys: KeyModifiers,
    ) -> Result<bool, String> {
        if !self.matches(editor) {
            return Err("The selected layers changed during the drag.".into());
        }
        if self.owner.is_none() {
            if (p.x - self.origin.x).abs() < 3.0 {
                return Ok(false);
            }
            let ids: Vec<_> = self.baseline.keys().copied().collect();
            editor.begin_node_transaction(&ids, "Scrub geometry");
            self.owner = editor.active_transaction_token();
        }
        let dx = p.x - self.last_x;
        self.last_x = p.x;
        let dy = p.y - self.origin.y;
        let speed = if dy <= -48.0 {
            2.0
        } else if dy >= 96.0 {
            0.25
        } else if dy >= 48.0 {
            0.5
        } else {
            1.0
        };
        let step = if keys.contains(KeyModifiers::SHIFT) {
            10.0
        } else if keys.contains(KeyModifiers::ALT) {
            0.1
        } else {
            1.0
        };
        // Integrate each segment at its own speed. Changing modifiers midway
        // cannot jump to a new value or round away the layer's original fraction.
        let gain_changed = self.gain != speed * step;
        self.gain = speed * step;
        if dx == 0.0 {
            return Ok(gain_changed);
        }
        let delta = self.delta + dx * self.gain;
        let delta = if delta.abs() < 1e-9 { 0.0 } else { delta };
        if delta == 0.0 && self.delta != 0.0 {
            // Returning to the starting values must immediately restore path points,
            // constrained children and detached variable bindings, even before release.
            editor.cancel_transaction();
            let ids: Vec<_> = self.baseline.keys().copied().collect();
            editor.begin_node_transaction(&ids, "Scrub geometry");
            self.owner = editor.active_transaction_token();
            self.delta = 0.0;
            return Ok(true);
        }
        let mut values = BTreeMap::new();
        for (&id, &base) in &self.baseline {
            let mut value = base + delta;
            if !value.is_finite() || value.abs() > 1e7 {
                return Err("The drag exceeds the geometry limit.".into());
            }
            if matches!(self.property, GeometryField::Width | GeometryField::Height) {
                value = value.max(1.0);
            }
            values.insert(id, value);
        }
        self.property
            .set_values(editor, &values, self.owner.as_ref())?;
        self.delta = delta;
        Ok(true)
    }
}

impl EditorShell {
    pub(super) fn cancel_stale_geometry_scrub(self: &Rc<Self>) {
        // Hiding or disabling a focused field can raise LostFocus synchronously.
        // Release its transaction before the inspector takes its document borrow.
        let stale = self.geometry_scrub.borrow().as_ref().is_some_and(|drag| {
            let editor = self.editor.borrow();
            !drag.matches(&editor) || drag.baseline.keys().any(|id| editor.document.effective_locked(*id))
        });
        if stale {
            self.finish_geometry_scrub(false);
        }
    }

    /// A browser lock notification may arrive after a different gesture starts.
    pub fn cancel_browser_pointer_scrub(self: &Rc<Self>, token: u32) {
        if self.geometry_scrub.borrow().as_ref().is_some_and(|drag| drag.host.matches(token)) {
            self.finish_geometry_scrub(false);
        }
    }

    pub(super) fn geometry_scrub_active(&self) -> bool {
        self.geometry_scrub
            .borrow()
            .as_ref()
            .is_some_and(|drag| drag.owner.is_some())
    }

    fn scrub_pointer(&self, pointer: &Rc<dyn IPointer>) -> bool {
        self.geometry_scrub
            .borrow()
            .as_ref()
            .is_some_and(|drag| drag.pointer.id() == pointer.id())
    }

    pub(super) fn wire_geometry_scrub(
        self: &Rc<Self>,
        name: &'static str,
        property: GeometryField,
    ) {
        let field = self.field(name);
        let prefix_name = name.replace("Field", "Scrub");
        let prefix = self.control::<TextBlock>(&prefix_name);
        prefix.set_cursor(Some(Cursor::new(StandardCursorType::SizeWestEast)));
        let weak_prefix = prefix.downgrade();
        let weak = Rc::downgrade(self);
        field.add_handler_with(
            InputElement::pointer_pressed_event(),
            move |_, e| {
                if !e.properties().is_left_button_pressed
                    || e.pointer().type_() == PointerType::Touch
                {
                    return;
                }
                let (Some(shell), Some(prefix)) = (weak.upgrade(), weak_prefix.upgrade()) else {
                    return;
                };
                let p = e.get_position(Some(&prefix));
                let b = prefix.bounds();
                // The whole height of the field above/below the glyph is clickable.
                let over_prefix = p.x >= 0.0 && p.x <= b.width;
                if !over_prefix && !e.key_modifiers().contains(KeyModifiers::ALT) {
                    return;
                }
                let field = shell.field(name);
                if field.is_composing_text()
                    || !field.is_enabled()
                    || shell.geometry_scrub.borrow().is_some()
                {
                    return;
                }
                if !shell.flush_inspector_drafts() {
                    field.focus();
                    e.set_handled(true);
                    return;
                }
                shell.finish_canvas_edits_for_inspector();
                let editor = shell.editor.borrow();
                if editor.has_active_transaction() {
                    return;
                }
                let baseline: BTreeMap<_, _> = editor
                    .selection
                    .iter()
                    .filter_map(|&id| {
                        let node = editor.document.nodes.get(&id)?;
                        (!editor.document.effective_locked(id)).then(|| (id, property.read(node)))
                    })
                    .collect();
                if baseline.is_empty() {
                    return;
                }
                let origin = e.get_position(Some(&shell.root));
                let drag = Scrub {
                    name,
                    property,
                    pointer: e.pointer().clone(),
                    host: if e.pointer().type_() == PointerType::Mouse {
                        crate::platform::ScrubPointer::arm(origin)
                    } else { Default::default() },
                    origin,
                    last_x: origin.x,
                    gain: 1.0,
                    delta: 0.0,
                    page: editor.active_page,
                    selection: editor.selection.clone(),
                    baseline,
                    version: editor.render_change_token(),
                    owner: None,
                };
                drop(editor);
                field.focus();
                *shell.geometry_scrub.borrow_mut() = Some(drag);
                e.pointer().capture(Some(&field.upcast::<InputElement>()));
                e.set_handled(true);
            },
            RoutingStrategies::TUNNEL,
            false,
        );
        let weak = Rc::downgrade(self);
        field.add_handler_with(
            InputElement::pointer_moved_event(),
            move |_, e| {
                if let Some(shell) = weak.upgrade().filter(|s| s.scrub_pointer(e.pointer())) {
                    if e.properties().is_left_button_pressed {
                        shell.sample_geometry_scrub(
                            e.get_position(Some(&shell.root)),
                            e.key_modifiers(),
                            true,
                        );
                    }
                    e.set_handled(true);
                }
            },
            RoutingStrategies::TUNNEL,
            false,
        );
        let weak = Rc::downgrade(self);
        field.add_handler_with(
            InputElement::pointer_released_event(),
            move |_, e| {
                if let Some(shell) = weak.upgrade().filter(|s| s.scrub_pointer(e.pointer())) {
                    shell.sample_geometry_scrub(
                        e.get_position(Some(&shell.root)),
                        e.key_modifiers(),
                        false,
                    );
                    shell.finish_geometry_scrub(true);
                    e.set_handled(true);
                }
            },
            RoutingStrategies::TUNNEL,
            false,
        );
        let weak = Rc::downgrade(self);
        field.add_handler(InputElement::pointer_capture_lost_event(), move |_, e| {
            if let Some(shell) = weak.upgrade().filter(|s| s.scrub_pointer(e.pointer())) {
                shell.finish_geometry_scrub(false);
            }
        });
        let weak = Rc::downgrade(self);
        field.add_handler(InputElement::lost_focus_event(), move |_, _| {
            if let Some(shell) = weak.upgrade() {
                if shell
                    .geometry_scrub
                    .borrow()
                    .as_ref()
                    .is_some_and(|drag| drag.name == name)
                {
                    shell.finish_geometry_scrub(false);
                }
            }
        });
    }

    pub(super) fn wire_geometry_scrub_keys(self: &Rc<Self>) {
        // The focused TextBox also handles Escape for ordinary text drafts.
        // The shell owns a captured drag and must cancel it before that handler.
        let weak = Rc::downgrade(self);
        self.root.add_handler_with(
            InputElement::key_down_event(),
            move |_, e| {
                let Some(shell) = weak.upgrade() else { return };
                let Some(name) = shell.geometry_scrub.borrow().as_ref().map(|drag| drag.name) else {
                    return;
                };
                if shell.field(name).is_composing_text() {
                    return;
                }
                match e.key {
                    Key::Escape => {
                        shell.finish_geometry_scrub(false);
                        e.set_handled(true);
                    }
                    Key::Z
                        if e.key_modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::META) =>
                    {
                        shell.finish_geometry_scrub(false);
                        e.set_handled(true);
                    }
                    Key::Tab | Key::Enter | Key::Up | Key::Down => {
                        shell.finish_geometry_scrub(true)
                    }
                    _ => {}
                }
            },
            RoutingStrategies::TUNNEL,
            false,
        );
    }

    fn sample_geometry_scrub(self: &Rc<Self>, point: ferroui_base::Point, keys: KeyModifiers, can_request_lock: bool) {
        let Some(mut drag) = self.geometry_scrub.take() else {
            return;
        };
        let was_armed = drag.owner.is_none();
        let point = drag.host.point(point);
        let result = drag.sample(&mut self.editor.borrow_mut(), point, keys);
        let request = (can_request_lock && was_armed && drag.owner.is_some() && result.is_ok())
            .then_some(drag.host.token());
        let (name, property, delta, gain) = (drag.name, drag.property, drag.delta, drag.gain);
        self.geometry_scrub.replace(Some(drag));
        match result {
            Ok(true) => {
                self.sync_document_draft_dirty();
                let editor = self.editor.borrow();
                let selection = fields::InspectorSelection::new(&editor);
                let value = selection.common(|node| property.read(node)).map(number);
                let field = self.field(name);
                let updating = self.updating.replace(true);
                selection.show(&field, value.as_deref(), None);
                if value.is_none() {
                    field.set_placeholder_text(Some(&format!("Mixed ({delta:+.2})")));
                }
                self.updating.set(updating);
                let caption = name.strip_suffix("Field").unwrap_or(name);
                self.text(
                    "GeometryScrubStatus",
                    &format!("{caption} {delta:+.2} · {gain}×"),
                );
                self.visible("GeometryScrubStatus", true);
                self.canvas.invalidate_visual();
            }
            Ok(false) => {}
            Err(message) => {
                self.finish_geometry_scrub(false);
                self.toast(&message);
            }
        }
        // The host can lose focus while requesting a privileged capability.
        // Publish the gesture and finish updating its UI before crossing into JS.
        if let Some(token) = request {
            crate::platform::ScrubPointer::request(token);
        }
    }

    pub(super) fn finish_geometry_scrub(self: &Rc<Self>, commit: bool) {
        let Some(drag) = self.geometry_scrub.take() else {
            return;
        };
        // Releasing capture can raise CaptureLost synchronously.
        if drag
            .pointer
            .captured()
            .is_some_and(|c| c == self.field(drag.name).upcast::<InputElement>())
        {
            drag.pointer.capture(None);
        }
        self.visible("GeometryScrubStatus", false);
        if let Some(owner) = &drag.owner {
            let mut editor = self.editor.borrow_mut();
            let valid = drag.matches(&editor);
            if editor.active_transaction_token().as_ref() == Some(owner) {
                let page = editor.active_page;
                let selection = editor.selection.clone();
                editor.cancel_transaction();
                if !valid && editor.active_page != page {
                    editor.set_active_page(page);
                }
                editor.selection = selection;
            }
            // Reapply once against the exact authored geometry. Repeated preview
            // samples cannot compound vector rounding or a child's constraints.
            let result = if valid && commit && drag.delta != 0.0 {
                drag.property.apply(&mut editor, None, drag.delta)
            } else {
                Ok(false)
            };
            drop(editor);
            match result {
                Ok(true) => self.changed(),
                other => {
                    self.refresh_selection();
                    self.canvas.invalidate_visual();
                    if self.pending_save.get() {
                        self.schedule_pending_save();
                    }
                    if let Err(message) = other {
                        self.toast(&message);
                    }
                }
            }
            self.sync_document_draft_dirty();
        } else if commit {
            self.field(drag.name).select_all();
        }
    }
}
