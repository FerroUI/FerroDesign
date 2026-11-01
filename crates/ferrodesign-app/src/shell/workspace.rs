//! Keep a usable artboard when the two desktop sidebars would consume the window.
use super::*;
use ferroui_base::interactivity::RoutingStrategies;
use ferroui_controls::primitives::ToggleButton;

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum CompactPanel {
    #[default]
    Canvas,
    Layers,
    Properties,
}

impl EditorShell {
    pub(super) fn wire_workspace(self: &Rc<Self>) {
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
                shell.switch_compact_panel(CompactPanel::Canvas);
                event.set_handled(true);
            }
        });
        let weak = Rc::downgrade(self);
        self.root.add_handler_with(InputElement::key_down_event(), move |_, event| {
            if event.key != Key::Escape || !event.key_modifiers.is_empty() { return; }
            let Some(shell) = weak.upgrade() else { return; };
            if shell.compact_panel.get() == CompactPanel::Canvas
                || shell.control::<Border>("ModalBackdrop").is_visible() { return; }
            // Give text and popup controls their own Escape. Canvas shortcuts
            // must not clear the selection underneath an open drawer.
            if event.route() == RoutingStrategies::TUNNEL && !shell.canvas.is_focused() { return; }
            shell.switch_compact_panel(CompactPanel::Canvas);
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
            shell.size_tool_dock(size.width < 440.0);
            if shell.compact.replace(compact) != compact {
                shell.compact_panel.set(CompactPanel::Canvas);
                shell.layout_workspace();
            }
        });
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
        let layers = !present && (!compact || panel == CompactPanel::Layers);
        let properties = !present && (!compact || panel == CompactPanel::Properties);
        if !layers { self.canvas.preview_layer(None); }
        self.visible("NavigationRail", layers);
        self.visible("LeftPanel", layers);
        self.visible("RightPanel", properties);
        self.visible("ZoomPanel", properties);
        self.visible("CanvasCaption", !present && !compact);
        self.visible("Toolbar", !present && (!compact || panel == CompactPanel::Canvas));
        self.visible("CompactHeader", !present && compact);
        self.visible("DrawerBackdrop", !present && compact && panel != CompactPanel::Canvas);
        self.sync_drawer_toggles(panel);
        let columns = match (present, compact, panel) {
            (true, _, _) | (false, true, CompactPanel::Canvas) => "0,0,*,0",
            (false, true, CompactPanel::Layers) => "48,248,*,0",
            (false, true, CompactPanel::Properties) => "0,0,*,288",
            (false, false, _) => "48,248,*,288",
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
