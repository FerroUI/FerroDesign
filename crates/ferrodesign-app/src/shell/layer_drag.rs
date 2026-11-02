//! The row button owns ordinary clicks. Crossing a screen-space threshold
//! transfers capture to the stable viewport, so the source row may be recycled.
use super::*;
use ferrodesign_core::{LayerRowSlot, LayerTreeMove};
use ferroui_base::{
    input::{IPointer, PointerType},
    interactivity::RoutingStrategies,
    Point as UiPoint, Vector,
};
use ferroui_controls::ScrollViewer;

#[derive(Default)]
pub(super) struct LayerDrag {
    pressed: Option<Press>,
    session: Option<LayerTreeMove>,
    target: Option<(NodeId, LayerRowSlot)>,
    marker: Option<WeakRef<Grid>>,
    timer: Option<Rc<dyn IDisposable>>,
}

struct Press {
    source: NodeId,
    version: core::EditorRenderToken,
    page: NodeId,
    pointer: Rc<dyn IPointer>,
    start: UiPoint,
    current: UiPoint,
    time: Instant,
}

impl EditorShell {
    pub(super) fn has_layer_drag(&self) -> bool {
        self.layer_drag.borrow().session.is_some()
    }

    pub(super) fn wire_layer_drag_source(self: &Rc<Self>, button: &Ref<Button>, id: NodeId) {
        let weak = Rc::downgrade(self);
        button.add_handler_with(
            InputElement::pointer_pressed_event(),
            move |_, e| {
                if !e.properties().is_left_button_pressed
                    || !e.key_modifiers().is_empty()
                {
                    return;
                }
                let Some(shell) = weak.upgrade() else { return };
                if shell.layer_drag.borrow().session.is_some()
                    || !shell
                        .field("LayerSearch")
                        .text()
                        .unwrap_or_default()
                        .trim()
                        .is_empty()
                {
                    return;
                }
                let viewport = shell.control::<ScrollViewer>("LayersViewport");
                let start = e.get_position(Some(&viewport));
                shell.layer_drag.borrow_mut().pressed = Some(Press {
                    source: id,
                    version: shell.editor.borrow().render_change_token(),
                    page: shell.editor.borrow().active_page,
                    start,
                    current: start,
                    time: Instant::now(),
                    pointer: e.pointer().clone(),
                });
            },
            RoutingStrategies::TUNNEL,
            false,
        );
    }

    pub(super) fn wire_layer_drag(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.root.add_handler_with(
            InputElement::pointer_moved_event(),
            move |_, e| {
                let Some(s) = weak.upgrade() else { return };
                if s.layer_drag
                    .borrow()
                    .pressed
                    .as_ref()
                    .map(|p| p.pointer.id())
                    != Some(e.pointer().id())
                {
                    return;
                }
                if !e.properties().is_left_button_pressed {
                    s.end_layer_drag(false);
                    return;
                }
                let viewport = s.control::<ScrollViewer>("LayersViewport");
                let point = e.get_position(Some(&viewport));
                if s.update_layer_drag(point) {
                    e.set_handled(true);
                }
            },
            RoutingStrategies::TUNNEL,
            true,
        );
        let weak = Rc::downgrade(self);
        self.root.add_handler_with(
            InputElement::pointer_released_event(),
            move |_, e| {
                let Some(s) = weak.upgrade() else { return };
                if s.layer_drag
                    .borrow()
                    .pressed
                    .as_ref()
                    .map(|p| p.pointer.id())
                    != Some(e.pointer().id())
                {
                    return;
                }
                let viewport = s.control::<ScrollViewer>("LayersViewport");
                let active = s.update_layer_drag(e.get_position(Some(&viewport)));
                s.end_layer_drag(active);
                if active {
                    e.set_handled(true);
                }
            },
            RoutingStrategies::TUNNEL,
            true,
        );
        let weak = Rc::downgrade(self);
        let viewport = self.control::<ScrollViewer>("LayersViewport");
        viewport.add_handler(InputElement::pointer_capture_lost_event(), move |_, e| {
            let Some(s) = weak.upgrade() else { return };
            let viewport = s
                .control::<ScrollViewer>("LayersViewport")
                .upcast::<InputElement>();
            if e.pointer().captured().as_ref() != Some(&viewport)
                && s.layer_drag.borrow().session.is_some()
            {
                s.end_layer_drag(false);
            }
        });
        let weak = Rc::downgrade(self);
        self.root.add_handler_with(
            InputElement::key_down_event(),
            move |_, e| {
                if e.key == Key::Escape {
                    if let Some(s) = weak
                        .upgrade()
                        .filter(|s| s.layer_drag.borrow().session.is_some())
                    {
                        s.end_layer_drag(false);
                        e.set_handled(true);
                    }
                }
            },
            RoutingStrategies::TUNNEL,
            false,
        );
        let weak = Rc::downgrade(self);
        viewport.unloaded(move |_, _| {
            if let Some(s) = weak.upgrade() {
                s.end_layer_drag(false);
            }
        });
    }

    fn update_layer_drag(self: &Rc<Self>, point: UiPoint) -> bool {
        let (source, pointer, activate) = {
            let mut drag = self.layer_drag.borrow_mut();
            let active = drag.session.is_some();
            let Some(press) = &mut drag.pressed else {
                return false;
            };
            let editor = self.editor.borrow();
            if press.page != editor.active_page || press.version != editor.render_change_token() {
                drop(editor);
                drop(drag);
                self.end_layer_drag(false);
                return false;
            }
            drop(editor);
            press.current = point;
            if !active {
                if (point.x - press.start.x).hypot(point.y - press.start.y) < 5.0 {
                    return false;
                }
                // A quick touch swipe scrolls the list. Hold a name first to move it.
                if press.pointer.type_() == PointerType::Touch
                    && press.time.elapsed() < Duration::from_millis(350)
                {
                    drag.pressed = None;
                    return false;
                }
            }
            (press.source, press.pointer.clone(), !active)
        };
        if activate {
            if !self.flush_inspector_drafts() {
                self.end_layer_drag(false);
                return false;
            }
            self.canvas.cancel_active_edit();
            let session = LayerTreeMove::new(&self.editor.borrow(), source);
            let Some(session) = session else {
                self.end_layer_drag(false);
                return false;
            };
            let viewport = self.control::<ScrollViewer>("LayersViewport");
            pointer.capture(Some(&viewport.clone().upcast::<InputElement>()));
            self.layer_drag.borrow_mut().session = Some(session);
            let weak = Rc::downgrade(self);
            self.layer_drag.borrow_mut().timer = Some(DispatcherTimer::run(
                move || weak.upgrade().is_some_and(|s| s.scroll_layer_drag()),
                Duration::from_millis(40),
                DispatcherPriority::INPUT,
            ));
        }
        self.update_layer_drop_marker(point);
        true
    }

    fn scroll_layer_drag(self: &Rc<Self>) -> bool {
        let drag = self.layer_drag.borrow();
        let Some(press) = &drag.pressed else {
            return false;
        };
        let point = press.current;
        drop(drag);
        let view = self.control::<ScrollViewer>("LayersViewport");
        if !view.is_effectively_visible() {
            self.end_layer_drag(false);
            return false;
        }
        if scroll_near_edge(&view, point) { self.update_layer_drop_marker(point); }
        true
    }

    fn update_layer_drop_marker(&self, point: UiPoint) {
        let view = self.control::<ScrollViewer>("LayersViewport");
        let bounds = view.bounds();
        let target = if view.is_effectively_visible()
            && point.x >= 0.0
            && point.x < bounds.width
            && point.y >= 0.0
            && point.y < bounds.height
        {
            view.get_visual_at(point)
                .and_then(|hit| {
                    hit.find_ancestor_of_type_where::<Grid>(true, |grid| {
                        grid.classes().contains("layer-row")
                    })
                })
                .and_then(|row| {
                    let id = *row.tag()?.downcast_ref::<NodeId>()?;
                    let origin = row.translate_point(UiPoint::default(), &view)?;
                    let height = row.bounds().height;
                    let y = point.y - origin.y;
                    let editor = self.editor.borrow();
                    let node = editor.document.nodes.get(&id)?;
                    let slot = if node.kind.is_container()
                        && (height * 0.25..height * 0.75).contains(&y)
                    {
                        LayerRowSlot::Inside
                    } else if y < height / 2.0 {
                        LayerRowSlot::Above
                    } else {
                        LayerRowSlot::Below
                    };
                    self.layer_drag
                        .borrow()
                        .session
                        .as_ref()?
                        .target_parent(&editor, id, slot)?;
                    Some((id, slot, row))
                })
        } else {
            None
        };
        let next = target.as_ref().map(|(id, slot, _)| (*id, *slot));
        let mut drag = self.layer_drag.borrow_mut();
        if next == drag.target
            && drag.marker.as_ref().and_then(WeakRef::upgrade)
                == target.as_ref().map(|(_, _, r)| r.clone())
        {
            return;
        }
        drag.target = next;
        let previous = drag.marker.take().and_then(|w| w.upgrade());
        drag.marker = target.as_ref().map(|(_, _, r)| r.downgrade());
        drop(drag);
        if let Some(old) = previous {
            clear_marker(&old);
        }
        if let Some((_, slot, row)) = target {
            row.classes().add(marker_class(slot));
        }
    }

    pub(super) fn end_layer_drag(self: &Rc<Self>, commit: bool) {
        let drag = self.layer_drag.take();
        if let Some(timer) = drag.timer {
            timer.dispose();
        }
        if let Some(row) = drag.marker.and_then(|w| w.upgrade()) {
            clear_marker(&row);
        }
        let Some(session) = drag.session else { return };
        if let Some(press) = drag.pressed {
            let viewport = self
                .control::<ScrollViewer>("LayersViewport")
                .upcast::<InputElement>();
            if press.pointer.captured().as_ref() == Some(&viewport) {
                press.pointer.capture(None);
            }
        }
        if commit {
            if let Some((row, slot)) = drag.target {
                let result = session.commit(&mut self.editor.borrow_mut(), row, slot);
                match result {
                    Ok(true) => {
                        if slot == LayerRowSlot::Inside {
                            self.expanded.borrow_mut().insert(row);
                        }
                        self.changed();
                    }
                    Err(error) => self.toast(&error.to_string()),
                    Ok(false) => {}
                }
            }
        }
        self.canvas.focus();
    }
}

fn clear_marker(row: &Grid) {
    for class in ["insert-above", "insert-inside", "insert-below"] {
        row.classes().remove(class);
    }
}

fn marker_class(slot: LayerRowSlot) -> &'static str {
    match slot {
        LayerRowSlot::Above => "insert-above",
        LayerRowSlot::Inside => "insert-inside",
        LayerRowSlot::Below => "insert-below",
    }
}

/// Scroll by the pointer’s depth into either edge of an arranged sidebar list.
pub(super) fn scroll_near_edge(view: &ScrollViewer, point: UiPoint) -> bool {
    let bounds = view.bounds();
    if point.x >= 0.0 && point.x < bounds.width && point.y >= 0.0 && point.y <= bounds.height {
        let edge = 24.0_f64.min(bounds.height / 4.0);
        let delta = if point.y < edge {
            point.y - edge
        } else {
            (point.y + edge - bounds.height).max(0.0)
        };
        let offset = view.offset();
        let y = (offset.y + delta).clamp(0.0, view.scroll_bar_maximum().y.max(0.0));
        if y != offset.y {
            view.set_offset(Vector::new(offset.x, y));
            view.update_layout();
            return true;
        }
    }
    false
}
