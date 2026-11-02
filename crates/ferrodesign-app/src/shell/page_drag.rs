//! Drag an arranged page row across the visible midpoints. The page order
//! changes only on release, and dragging an inactive row keeps the canvas open.
use super::*;
use ferroui_base::{
    input::{IPointer, PointerType},
    Point as UiPoint,
};
use ferroui_controls::ScrollViewer;

pub(in crate::shell) struct PageDrag {
    source: usize,
    version: core::EditorRenderToken,
    page: NodeId,
    pointer: Rc<dyn IPointer>,
    started: Instant,
    origin: UiPoint,
    current: UiPoint,
    active: bool,
    slot: Option<usize>,
    marker: Option<(Ref<Grid>, &'static str)>,
    line: Option<Ref<Border>>,
    timer: Option<Subscription>,
}

impl EditorShell {
    pub(in crate::shell) fn has_page_drag(&self) -> bool {
        self.page_drag.borrow().as_ref().is_some_and(|d| d.active)
    }

    pub(super) fn wire_page_drag_source(self: &Rc<Self>, button: &Ref<ToggleButton>, id: NodeId) {
        let weak = Rc::downgrade(self);
        button.add_handler_with(
            InputElement::pointer_pressed_event(),
            move |_, e| {
                if !e.properties().is_left_button_pressed || !e.key_modifiers().is_empty() {
                    return;
                }
                let Some(s) = weak.upgrade() else { return };
                if s.page_drag
                    .borrow()
                    .as_ref()
                    .is_some_and(|d| d.active || d.pointer.captured().is_some())
                {
                    return;
                }
                let editor = s.editor.borrow();
                if editor.has_active_transaction() || editor.document.pages.len() < 2 {
                    return;
                }
                let Some(source) = editor.document.pages.iter().position(|p| p.id == id) else {
                    return;
                };
                let origin = e.get_position(Some(&s.control::<ScrollViewer>("PagesViewport")));
                s.page_drag.replace(Some(PageDrag {
                    source,
                    version: editor.render_change_token(),
                    page: editor.active_page,
                    pointer: e.pointer().clone(),
                    started: Instant::now(),
                    origin,
                    current: origin,
                    active: false,
                    slot: None,
                    marker: None,
                    line: None,
                    timer: None,
                }));
            },
            RoutingStrategies::TUNNEL,
            false,
        );
    }

    pub(super) fn wire_page_drag(self: &Rc<Self>) {
        let view = self.control::<ScrollViewer>("PagesViewport");
        let weak = Rc::downgrade(self);
        self.root.add_handler_with(
            InputElement::pointer_moved_event(),
            move |_, e| {
                let Some(s) = weak.upgrade().filter(|s| s.page_pointer(e.pointer().id())) else {
                    return;
                };
                if !e.properties().is_left_button_pressed {
                    s.finish_page_drag(false);
                    return;
                }
                let point = e.get_position(Some(&s.control::<ScrollViewer>("PagesViewport")));
                if s.update_page_drag(point) {
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
                let Some(s) = weak.upgrade().filter(|s| s.page_pointer(e.pointer().id())) else {
                    return;
                };
                let point = e.get_position(Some(&s.control::<ScrollViewer>("PagesViewport")));
                let active = s.update_page_drag(point);
                s.finish_page_drag(active);
                if active {
                    e.set_handled(true);
                }
            },
            RoutingStrategies::TUNNEL,
            true,
        );
        let weak = Rc::downgrade(self);
        self.root.add_handler_with(
            InputElement::key_down_event(),
            move |_, e| {
                if e.key == Key::Escape {
                    if let Some(s) = weak.upgrade().filter(|s| s.has_page_drag()) {
                        s.finish_page_drag(false);
                        e.set_handled(true);
                    }
                }
            },
            RoutingStrategies::TUNNEL,
            false,
        );
        let weak = Rc::downgrade(self);
        view.add_handler(InputElement::pointer_capture_lost_event(), move |_, e| {
            let Some(s) = weak
                .upgrade()
                .filter(|s| s.has_page_drag() && s.page_pointer(e.pointer().id()))
            else {
                return;
            };
            if e.pointer().captured().as_ref()
                != Some(&s.control::<ScrollViewer>("PagesViewport").upcast())
            {
                s.finish_page_drag(false);
            }
        });
        let weak = Rc::downgrade(self);
        view.unloaded(move |_, _| {
            if let Some(s) = weak.upgrade() {
                s.finish_page_drag(false);
            }
        });
    }

    fn page_pointer(&self, id: i32) -> bool {
        self.page_drag
            .borrow()
            .as_ref()
            .is_some_and(|d| d.pointer.id() == id)
    }

    fn update_page_drag(self: &Rc<Self>, point: UiPoint) -> bool {
        let mut state = self.page_drag.borrow_mut();
        let Some(drag) = state.as_mut() else {
            return false;
        };
        let editor = self.editor.borrow();
        if drag.version != editor.render_change_token() || drag.page != editor.active_page {
            let active = drag.active;
            drop(editor);
            drop(state);
            self.finish_page_drag(false);
            return active;
        }
        drop(editor);
        drag.current = point;
        if !drag.active {
            if (point.x - drag.origin.x).hypot(point.y - drag.origin.y) < 5. {
                return false;
            }
            if drag.pointer.type_() == PointerType::Touch
                && drag.started.elapsed() < Duration::from_millis(350)
            {
                *state = None;
                return false;
            }
            let pointer = drag.pointer.clone();
            drop(state);
            let view = self.control::<ScrollViewer>("PagesViewport");
            pointer.capture(Some(&view.clone().upcast::<InputElement>()));
            if !self.flush_inspector_drafts() {
                pointer.capture(None);
                self.finish_page_drag(false);
                return true;
            }
            state = self.page_drag.borrow_mut();
            let Some(drag) = state.as_mut() else {
                return true;
            };
            drag.version = self.editor.borrow().render_change_token();
            drag.active = true;
            // Stop delayed hover work for rows crossed during capture. Enabling
            // the service after release leaves the next hover to start its own timer.
            ToolTip::set_service_enabled(&view, false);
            let weak = Rc::downgrade(self);
            drag.timer = Some(DispatcherTimer::run(
                move || weak.upgrade().is_some_and(|s| s.scroll_page_drag()),
                Duration::from_millis(40),
                DispatcherPriority::INPUT,
            ));
        }
        drop(state);
        self.mark_page_slot(point);
        true
    }

    pub(in crate::shell) fn scroll_page_drag(self: &Rc<Self>) -> bool {
        let Some(point) = self
            .page_drag
            .borrow()
            .as_ref()
            .filter(|d| d.active)
            .map(|d| d.current)
        else {
            return false;
        };
        let view = self.control::<ScrollViewer>("PagesViewport");
        if !view.is_effectively_visible() {
            self.finish_page_drag(false);
            return false;
        }
        if super::super::layer_drag::scroll_near_edge(&view, point) {
            self.mark_page_slot(point);
        }
        true
    }

    fn mark_page_slot(&self, point: UiPoint) {
        let view = self.control::<ScrollViewer>("PagesViewport");
        let bounds = view.bounds();
        let mut state = self.page_drag.borrow_mut();
        let Some(drag) = state.as_mut() else { return };
        let slot = (view.is_effectively_visible()
            && point.x >= 0.
            && point.x < bounds.width
            && point.y >= 0.
            && point.y < bounds.height)
            .then(|| view.translate_point(point, &self.control::<StackPanel>("PagesHost")))
            .flatten()
            .map(|local| {
                self.page_list
                    .borrow()
                    .rows
                    .partition_point(|r| r.host.bounds().center().y <= local.y)
            })
            .filter(|i| *i != drag.source && *i != drag.source + 1);
        if slot == drag.slot {
            return;
        }
        if let Some((row, class)) = drag.marker.take() {
            row.classes().remove(class);
            if let Some(line) = &drag.line {
                row.children().remove(line.clone());
            }
        }
        drag.slot = slot;
        if let Some(slot) = slot {
            let list = self.page_list.borrow();
            let below = slot == list.rows.len();
            let row = &list.rows[slot - usize::from(below)].host;
            let class = if below {
                "insert-below"
            } else {
                "insert-above"
            };
            let line = drag.line.get_or_insert_with(|| {
                let line = Border::new();
                line.classes().add("layer-insertion");
                // A solid two-DIP rectangle remains visible even when its height
                // is smaller than the two opposed strokes of an outlined Border.
                line.set_border_thickness(ferroui_base::Thickness::default());
                line.set_background(Some(icons::brush("#0D99FF")));
                line.set_is_hit_test_visible(false);
                line
            });
            row.children().add(line.clone());
            row.classes().add(class);
            drag.marker = Some((row.clone(), class));
        }
    }

    pub(in crate::shell) fn finish_page_drag(self: &Rc<Self>, commit: bool) {
        let Some(drag) = self.page_drag.take() else {
            return;
        };
        if let Some(timer) = drag.timer {
            timer.dispose();
        }
        if let Some((row, class)) = drag.marker {
            row.classes().remove(class);
            if let Some(line) = drag.line {
                row.children().remove(line);
            }
        }
        if !drag.active {
            return;
        }
        let viewport = self.control::<ScrollViewer>("PagesViewport");
        viewport.clear_value(ToolTip::service_enabled_property());
        let view = viewport.upcast::<InputElement>();
        if drag.pointer.captured().as_ref() == Some(&view) {
            drag.pointer.capture(None);
        }
        if let Some(slot) = drag.slot.filter(|_| commit) {
            let to = slot - usize::from(slot > drag.source);
            let mut editor = self.editor.borrow_mut();
            if drag.version == editor.render_change_token()
                && drag.page == editor.active_page
                && editor.move_page_to(drag.source, to)
            {
                drop(editor);
                self.changed();
                // Refresh may reveal the active row. Keep the row that was
                // actually dropped visible, including after edge scrolling.
                let moved = self.page_list.borrow().rows[to].host.clone();
                moved.bring_into_view();
            }
        }
        self.canvas.focus();
    }
}
