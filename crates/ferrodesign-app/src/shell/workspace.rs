//! Keep a usable artboard when the two desktop sidebars would consume the window.
use super::*;

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
                    let open = shell.compact_panel.get() != panel;
                    shell.compact_panel.set(if open { panel } else { CompactPanel::Canvas });
                    shell.layout_workspace();
                    if !open { shell.canvas.focus(); }
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.root.size_changed(move |_, event| {
            let Some(shell) = weak.upgrade() else { return; };
            let size = event.new_size();
            let dialog = shell.control::<Border>("ModalPanel");
            dialog.set_max_width((size.width - 16.0).max(0.0));
            dialog.set_max_height((size.height - 16.0).clamp(0.0, 660.0));
            let compact = size.width < 900.0;
            if shell.compact.replace(compact) != compact {
                shell.compact_panel.set(CompactPanel::Canvas);
                shell.layout_workspace();
            }
        });
    }

    pub(super) fn layout_workspace(&self) {
        let present = self.prototype.is_active();
        let compact = self.compact.get();
        let panel = self.compact_panel.get();
        let layers = !present && (!compact || panel == CompactPanel::Layers);
        let properties = !present && (!compact || panel == CompactPanel::Properties);
        self.visible("NavigationRail", layers);
        self.visible("LeftPanel", layers);
        self.visible("RightPanel", properties);
        self.visible("ZoomPanel", properties);
        self.visible("CanvasCaption", !present && !compact);
        self.visible("Toolbar", !present && (!compact || panel == CompactPanel::Canvas));
        self.visible("CompactHeader", !present && compact);
        for (button, selected) in [("CompactLayers", layers), ("CompactProperties", properties)] {
            self.button(button).classes().set("active", selected);
        }
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
