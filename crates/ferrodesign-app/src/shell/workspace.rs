//! Keep a usable artboard when the two desktop sidebars would consume the window.
use super::*;
use ferroui_base::input::KeyModifiers;
use ferroui_base::interactivity::RoutingStrategies;
use ferroui_controls::primitives::ToggleButton;

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum CompactPanel {
    #[default]
    Canvas,
    Layers,
    Properties,
}

/// Hiding can temporarily cover a minimized layout. Showing it again returns
/// to the same sidebar policy and leaves the user's drawer choice intact.
#[derive(Default)]
pub(super) struct WorkspaceUi {
    hidden: Cell<bool>,
    minimized: Cell<bool>,
    inspect_on_phone: Cell<bool>,
}

impl WorkspaceUi {
    fn is_expanded(&self) -> bool {
        !self.hidden.get() && !self.minimized.get()
    }

    pub(super) fn name(&self) -> &'static str {
        if self.hidden.get() { "hidden" }
        else if self.minimized.get() { "minimized" }
        else { "expanded" }
    }
}

impl EditorShell {
    pub(super) fn wire_workspace(self: &Rc<Self>) {
        self.hook("MinimizeUiButton", "minimize-ui");
        self.hook("RestoreUiButton", "restore-ui");
        let weak = Rc::downgrade(self);
        self.button("MinimizedInspectorButton").click(move |_, _| {
            if let Some(shell) = weak.upgrade() {
                let showing = shell.workspace_ui.inspect_on_phone.get();
                if shell.flush_inspector_drafts() {
                    shell.workspace_ui.inspect_on_phone.set(!showing);
                    shell.layout_workspace();
                } else {
                    shell.control::<ToggleButton>("MinimizedInspectorButton").set_is_checked(Some(showing));
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.root.add_handler_with(InputElement::key_down_event(), move |_, event| {
            if !matches!(event.key, Key::OemPipe | Key::OemBackslash)
                && !matches!(event.key_symbol.as_deref(), Some("\\" | "|")) { return; }
            let modifiers = event.key_modifiers - KeyModifiers::SHIFT;
            if modifiers != KeyModifiers::CONTROL && modifiers != KeyModifiers::META { return; }
            let Some(shell) = weak.upgrade() else { return };
            // A modal owns its keyboard, and an authored prototype may bind the same chord.
            if shell.prototype.is_active() || shell.control::<Border>("ModalBackdrop").is_visible() { return; }
            shell.change_workspace_ui(if event.key_modifiers.contains(KeyModifiers::SHIFT) { "minimize-ui" } else { "toggle-ui" });
            event.set_handled(true);
        }, RoutingStrategies::TUNNEL, false);
        for (button, panel) in [
            ("CompactLayers", CompactPanel::Layers),
            ("CompactProperties", CompactPanel::Properties),
        ] {
            let weak = Rc::downgrade(self);
            self.button(button).click(move |_, _| {
                if let Some(shell) = weak.upgrade() {
                    let next = if shell.compact_panel.get() == panel { CompactPanel::Canvas } else { panel };
                    shell.switch_compact_panel(next);
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.control::<Border>("DrawerBackdrop").add_handler(InputElement::tapped_event(), move |_, event| {
            if let Some(shell) = weak.upgrade() {
                shell.close_compact_panel();
                event.set_handled(true);
            }
        });
        let weak = Rc::downgrade(self);
        self.root.add_handler_with(InputElement::key_down_event(), move |_, event| {
            if event.key != Key::Escape || !event.key_modifiers.is_empty() { return; }
            let Some(shell) = weak.upgrade() else { return; };
            if !shell.control::<Border>("DrawerBackdrop").is_visible()
                || shell.control::<Border>("ModalBackdrop").is_visible() { return; }
            // Give text and popup controls their own Escape. Canvas shortcuts
            // must not clear the selection underneath an open drawer.
            if event.route() == RoutingStrategies::TUNNEL && !shell.canvas.is_focused() { return; }
            shell.close_compact_panel();
            event.set_handled(true);
        }, RoutingStrategies::TUNNEL | RoutingStrategies::BUBBLE, false);
        let weak = Rc::downgrade(self);
        self.root.size_changed(move |_, event| {
            let Some(shell) = weak.upgrade() else { return; };
            let size = event.new_size();
            let dialog = shell.control::<Border>("ModalPanel");
            dialog.set_max_width((size.width - 16.0).max(0.0));
            dialog.set_max_height((size.height - 16.0).clamp(0.0, 660.0));
            // The desktop rail and panels reserve 584 DIPs beside the canvas.
            // Switch to drawers before they squeeze the full tool dock.
            let compact = size.width < 970.0;
            dialog.classes().set("phone", compact && size.width.min(size.height) <= 480.0);
            shell.size_tool_dock(size.width < 440.0);
            if shell.compact.replace(compact) != compact {
                shell.compact_panel.set(CompactPanel::Canvas);
                shell.workspace_ui.inspect_on_phone.set(false);
                shell.layout_workspace();
            }
        });
    }

    fn close_compact_panel(self: &Rc<Self>) {
        if self.workspace_ui.minimized.get() {
            if self.flush_inspector_drafts() {
                self.workspace_ui.inspect_on_phone.set(false);
                self.layout_workspace();
            }
        } else {
            self.switch_compact_panel(CompactPanel::Canvas);
        }
    }

    pub(super) fn change_workspace_ui(self: &Rc<Self>, command: &str) -> bool {
        if self.prototype.is_active() || self.control::<Border>("ModalBackdrop").is_visible() {
            return false;
        }
        // Changing chrome cannot splice a held canvas gesture or an IME session.
        // A completed inspector edit uses its ordinary commit/validation path.
        if self.canvas.has_pointer_gesture() || self.has_layer_drag()
            || self.editor.borrow().has_active_transaction() && self.inline_text.get().is_none()
            || self.inline_text.get().is_some() && self.field("InlineTextBox").is_composing_text()
        {
            return false;
        }
        let field = TopLevel::get_top_level(Some(&self.root))
            .and_then(|top| top.focus_manager().get_focused_element())
            .and_then(|focused| focused.cast::<TextBox>());
        self.finish_inline_text(true);
        if !self.flush_inspector_drafts() {
            if let Some(field) = field { field.focus(); }
            return false;
        }
        let was_expanded = self.workspace_ui.is_expanded();
        match command {
            "toggle-ui" => self.workspace_ui.hidden.set(!self.workspace_ui.hidden.get()),
            "restore-ui" if self.workspace_ui.hidden.get() => self.workspace_ui.hidden.set(false),
            "restore-ui" => self.workspace_ui.minimized.set(false),
            "minimize-ui" => {
                self.workspace_ui.hidden.set(false);
                self.workspace_ui.minimized.set(!self.workspace_ui.minimized.get());
                self.workspace_ui.inspect_on_phone.set(false);
            }
            _ => return false,
        }
        self.apply_workspace_mode(was_expanded);
        true
    }

    fn apply_workspace_mode(&self, was_expanded: bool) {
        let expanded = self.workspace_ui.is_expanded();
        // On desktop the canvas origin moves by the widths of the rail and left
        // panel. Keep artwork at the same window coordinates through that move.
        if !self.compact.get() && expanded != was_expanded {
            if let Some(mut camera) = self.canvas.page_camera() {
                camera.pan.x += if expanded { -296.0 } else { 296.0 };
                self.canvas.update_camera(camera);
            }
        }
        self.layout_workspace();
    }

    pub(super) fn show_layers_panel(&self) {
        let expanded = self.workspace_ui.is_expanded();
        self.workspace_ui.hidden.set(false);
        self.workspace_ui.minimized.set(false);
        self.workspace_ui.inspect_on_phone.set(false);
        if self.compact.get() { self.compact_panel.set(CompactPanel::Layers); }
        if !expanded || self.compact.get() { self.apply_workspace_mode(expanded); }
    }

    pub(super) fn sync_context_inspector(&self) {
        if self.workspace_ui.minimized.get() && !self.workspace_ui.hidden.get() && !self.prototype.is_active() {
            let selected = !self.editor.borrow().selection.is_empty();
            if !selected { self.workspace_ui.inspect_on_phone.set(false); }
            let name = if self.compact.get() { "MinimizedInspectorButton" } else { "RightPanel" };
            if self.control::<Control>(name).is_visible() != selected { self.layout_workspace(); }
        }
    }

    fn switch_compact_panel(self: &Rc<Self>, next: CompactPanel) {
        let current = self.compact_panel.get();
        if current != next && self.flush_inspector_drafts() {
            self.compact_panel.set(next);
            self.layout_workspace();
            if next == CompactPanel::Canvas { self.canvas.focus(); }
        } else {
            // Native toggles have already toggled when Click arrives. Keep
            // their accessible state consistent when a draft rejects dismissal.
            self.sync_drawer_toggles(current);
        }
    }

    fn sync_drawer_toggles(&self, panel: CompactPanel) {
        for (button, value) in [("CompactLayers", CompactPanel::Layers), ("CompactProperties", CompactPanel::Properties)] {
            self.control::<ToggleButton>(button).set_is_checked(Some(panel == value));
        }
    }

    pub(super) fn layout_workspace(&self) {
        let present = self.prototype.is_active();
        self.visible("PreviewControls", present);
        let compact = self.compact.get();
        let panel = self.compact_panel.get();
        let hidden = self.workspace_ui.hidden.get();
        let minimized = self.workspace_ui.minimized.get();
        let expanded = !present && !hidden && !minimized;
        let minimal = !present && !hidden && minimized;
        let selected = !self.editor.borrow().selection.is_empty();
        if !selected { self.workspace_ui.inspect_on_phone.set(false); }
        let layers = expanded && (!compact || panel == CompactPanel::Layers);
        let properties = if minimal {
            selected && (!compact || self.workspace_ui.inspect_on_phone.get())
        } else {
            expanded && (!compact || panel == CompactPanel::Properties)
        };
        if !layers { self.canvas.preview_layer(None); }
        self.visible("NavigationRail", layers);
        self.visible("LeftPanel", layers);
        self.visible("RightPanel", properties);
        self.visible("ZoomPanel", properties);
        self.visible("CanvasCaption", expanded && !compact);
        let drawer = compact && (layers || properties);
        self.visible("Toolbar", !present && !hidden && !drawer);
        self.visible("CompactHeader", expanded && compact);
        self.visible("DrawerBackdrop", drawer);
        self.visible("CanvasControls", !present && (hidden || minimized));
        self.visible("MinimizedInspectorButton", minimal && compact && selected);
        self.control::<ToggleButton>("MinimizedInspectorButton").set_is_checked(Some(minimal && compact && properties));
        self.control::<Border>("LeftPanel").classes().set("phone-sidebar", compact);
        self.sync_drawer_toggles(panel);
        let columns = match (layers, properties) {
            (true, true) => "48,248,*,288",
            (true, false) => "48,248,*,0",
            (false, true) => "0,0,*,288",
            (false, false) => "0,0,*,0",
        };
        self.control::<Grid>("Workspace").set_column_definitions(
            ColumnDefinitions::parse(columns).expect("workspace columns"),
        );
        // Drawers overlay the existing artboard. Opening them must preserve both
        // the canvas size and the document point under a stationary pointer.
        let canvas = self.control::<Grid>("CanvasHost");
        Grid::set_column(&canvas, if compact { 0 } else { 2 });
        Grid::set_column_span(&canvas, if compact { 4 } else { 1 });
        for name in ["NavigationRail", "LeftPanel", "RightPanel"] {
            self.control::<Border>(name).set_margin(Thickness::new(
                0.0, if compact { 48.0 } else { 0.0 }, 0.0, 0.0,
            ));
        }
    }
}
