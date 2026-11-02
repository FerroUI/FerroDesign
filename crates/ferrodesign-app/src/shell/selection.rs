//! A native Select Layer menu built from the same indexed, mask-aware picking
//! query used by canvas selection. Choosing a row only changes editor focus.
use super::*;
use super::styles::style_label;
use ferroui_base::input::KeyModifiers;
use ferroui_base::interactivity::RoutingStrategies;

impl EditorShell {
    pub(super) fn wire_layer_preview(&self, row: &Ref<Grid>, id: NodeId) {
        let canvas = self.canvas.downgrade();
        row.add_handler(InputElement::pointer_entered_event(), move |_, _| {
            if let Some(canvas) = canvas.upgrade() { canvas.preview_layer(Some(id)); }
        });
        let canvas = self.canvas.downgrade();
        row.add_handler(InputElement::pointer_exited_event(), move |_, _| {
            if let Some(canvas) = canvas.upgrade() { canvas.end_layer_preview(id); }
        });
        // Virtualized rows can disappear under a stationary pointer.
        let canvas = self.canvas.downgrade();
        row.unloaded(move |_, _| {
            if let Some(canvas) = canvas.upgrade() { canvas.end_layer_preview(id); }
        });
    }

    pub(super) fn wire_layer_activation(self: &Rc<Self>, button: &Ref<Button>, id: NodeId, asset: bool) {
        // Button raises Click during the bubble phase of a successful release.
        // Keep that hit testing and keyboard/automation activation in the control.
        let modifiers = Rc::new(Cell::new(KeyModifiers::empty()));
        let second_press = Rc::new(Cell::new(false));
        if !asset {
            let presses = second_press.clone();
            button.add_handler_with(InputElement::pointer_pressed_event(), move |_, event| {
                presses.set(event.click_count() == 2 && event.properties().is_left_button_pressed);
            }, RoutingStrategies::TUNNEL, true);
            let before = modifiers.clone();
            button.add_handler_with(InputElement::pointer_released_event(), move |_, event| {
                before.set(event.key_modifiers());
            }, RoutingStrategies::TUNNEL, true);
            let after = modifiers.clone();
            let released = second_press.clone();
            button.add_handler_with(InputElement::pointer_released_event(), move |_, _| {
                after.set(KeyModifiers::empty());
                released.set(false);
            }, RoutingStrategies::BUBBLE, true);
        }
        let weak = Rc::downgrade(self);
        button.click(move |_, _| {
            let Some(shell) = weak.upgrade() else { return; };
            if !shell.flush_inspector_drafts() { return; }
            shell.canvas.cancel_active_edit();
            if asset {
                let point = shell.canvas.viewport.get().screen_to_world(Point::new(100.0, 100.0));
                shell.editor.borrow_mut().create_instance(id, point.x, point.y);
                shell.changed();
            } else {
                let rename = second_press.get() && modifiers.get().is_empty()
                    && shell.editor.borrow().selection.as_slice() == [id];
                shell.layer_projection.borrow().select_row(&mut shell.editor.borrow_mut(), id, modifiers.get());
                shell.refresh_selection();
                shell.canvas.invalidate_visual();
                if rename && shell.reveal_selected_layer(true) { return; }
                shell.focus_layer_row(id, ferroui_base::input::NavigationMethod::Unspecified);
                return;
            }
            shell.canvas.focus();
        });
    }

    pub(super) fn select_matching_dialog(self: &Rc<Self>) {
        if !self.flush_inspector_drafts() { return; }
        self.canvas.cancel_active_edit();
        self.modal("Select same", "Select visible, unlocked layers on this page that share the selected properties. Mixed selections match any selected value.");
        let host = self.control::<StackPanel>("ModalContent");
        host.set_spacing(6.0);
        let editor = self.editor.borrow();
        for &(label, name, property) in MATCHING_ACTIONS {
            let enabled = editor.selection.iter().filter_map(|id| editor.document.nodes.get(id))
                .any(|node| property.supports(node) && editor.document.page_of(node.id) == Some(editor.active_page)
                    && editor.document.effective_visible(node.id) && !editor.document.effective_locked(node.id));
            host.children().add(self.style_button(label, name, enabled,
                move |shell| { shell.select_same(property); }));
        }
        drop(editor);
        host.children().add(style_label("Appearance matches layer type, paint/effect stacks, opacity, blend, corners and typography. Position, size, paths and style links may differ."));
        host.children().add(style_label("Font matches the set of used family, weight and italic faces. Text properties also compare paragraph settings and resolved character-style order, independent of text content."));
        self.control::<StackPanel>("ModalActions").children().add(
            self.style_button("Select inverse", "SelectSameInverse", true,
                |shell| { shell.command("select-inverse"); }));
        self.modal_button("Done", "close-modal");
    }

    pub(super) fn select_same(self: &Rc<Self>, property: core::SelectionMatch) -> bool {
        if !self.flush_inspector_drafts() { return false; }
        self.canvas.cancel_active_edit();
        self.canvas.leave_vector_edit();
        let changed = self.editor.borrow_mut().select_matching(property);
        self.close_modal();
        self.refresh_selection();
        self.canvas.invalidate_visual();
        changed
    }

    pub(super) fn select_inverse(self: &Rc<Self>) -> bool {
        if !self.flush_inspector_drafts() { return false; }
        self.canvas.cancel_active_edit();
        self.canvas.leave_vector_edit();
        let changed = self.editor.borrow_mut().invert_selection();
        self.close_modal();
        self.refresh_selection();
        self.canvas.invalidate_visual();
        changed
    }

    pub(super) fn select_layer_dialog(self: &Rc<Self>) {
        if !self.flush_inspector_drafts() {
            return;
        }
        let candidates = self.canvas.selection_candidates(true);
        self.modal("Select layer", "Layers under the last canvas pointer, from front to back. Locked layers can be selected here for inspection.");
        let host = self.control::<StackPanel>("ModalContent");
        let editor = self.editor.borrow();
        let page = editor.active_page;
        if candidates.is_empty() {
            let label = TextBlock::new();
            label.set_text(Some(
                "Move the pointer over an object, then open Select layer.",
            ));
            label.set_text_wrapping(TextWrapping::Wrap);
            host.children().add(label);
        }
        for id in candidates {
            let Some(node) = editor.document.nodes.get(&id) else {
                continue;
            };
            let locked = editor.document.effective_locked(id);
            let row = Button::new();
            row.set_name(Some(format!("SelectLayer{id}")));
            row.set_horizontal_content_alignment(ferroui_base::layout::HorizontalAlignment::Left);
            row.set_content(Some(Rc::new(format!(
                "{}  ·  {}{}",
                node.name,
                node.kind.label(),
                if locked { "  ·  Locked" } else { "" }
            ))));
            row.classes().set("active", editor.selection.contains(&id));
            let weak = Rc::downgrade(self);
            row.click(move |_, _| {
                let Some(shell) = weak.upgrade() else {
                    return;
                };
                shell.canvas.cancel_active_edit();
                let mut editor = shell.editor.borrow_mut();
                if editor.active_page != page
                    || editor.has_active_transaction()
                    || !editor.document.nodes.contains_key(&id)
                    || editor.document.page_of(id) != Some(page)
                    || (!editor.document.effective_visible(id)
                        && !(shell.canvas.show_outlines.get() && shell.canvas.outline_hidden.get()))
                {
                    return;
                }
                editor.select(id, false);
                drop(editor);
                shell.canvas.leave_vector_edit();
                shell.close_modal();
                shell.refresh_selection();
                shell.canvas.invalidate_visual();
            });
            host.children().add(row);
        }
        drop(editor);
        self.modal_button("Done", "close-modal");
    }
}

const MATCHING_ACTIONS: &[(&str, &str, core::SelectionMatch)] = &[
    ("Same appearance", "SelectSameProperties", core::SelectionMatch::Appearance),
    ("Same fill", "SelectSameFill", core::SelectionMatch::Fill),
    ("Same stroke", "SelectSameStroke", core::SelectionMatch::Stroke),
    ("Same effect", "SelectSameEffect", core::SelectionMatch::Effect),
    ("Same text properties", "SelectSameText", core::SelectionMatch::TextProperties),
    ("Same font", "SelectSameFont", core::SelectionMatch::Font),
    ("Same component instance", "SelectSameInstance", core::SelectionMatch::Instance),
];
