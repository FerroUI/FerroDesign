//! Numeric scaling uses the same selection geometry and transaction as a canvas drag.
use super::*;
use ferroui_controls::MenuItem;

const ANCHORS: [&str; 9] = [
    "ScaleAnchor0", "ScaleAnchor1", "ScaleAnchor2", "ScaleAnchor3", "ScaleAnchor4",
    "ScaleAnchor5", "ScaleAnchor6", "ScaleAnchor7", "ScaleAnchor8",
];

impl EditorShell {
    pub(super) fn wire_scale(self: &Rc<Self>) {
        for (index, name) in ANCHORS.into_iter().enumerate() {
            let weak = Rc::downgrade(self);
            self.button(name).click(move |_, _| {
                if let Some(shell) = weak.upgrade() {
                    let old = shell.scale_anchor.replace(index);
                    shell.button(ANCHORS[old]).classes().set("active", false);
                    shell.button(ANCHORS[index]).classes().set("active", true);
                }
            });
        }
        self.button(ANCHORS[self.scale_anchor.get()]).classes().set("active", true);
        for (name, factor) in [("ScaleQuarter", 0.25), ("ScaleHalf", 0.5), ("ScaleDouble", 2.0),
                                ("ScaleTriple", 3.0), ("ScaleQuadruple", 4.0)] {
            let weak = Rc::downgrade(self);
            self.control::<MenuItem>(name).click(move |_, _| {
                if let Some(shell) = weak.upgrade() {
                    shell.apply_scale(factor);
                    shell.canvas.focus();
                }
            });
        }
    }

    pub(super) fn sync_scale_panel(&self, editor: &Editor) -> bool {
        let active = self.canvas.tool.get() == Tool::Scale && self.inspector_tab.get() == 0;
        let bounds = active.then(|| editor.selection_scale_bounds()).flatten();
        self.visible("ScaleInspector", bounds.is_some());
        if let Some(bounds) = bounds {
            self.field("ScaleFactorField").set_text(Some("1"));
            for (name, value) in [("ScaleWidthField", bounds.width), ("ScaleHeightField", bounds.height)] {
                self.field(name).set_text(Some(&number(value)));
                self.field(name).set_is_enabled(value > 0.0);
            }
        }
        bounds.is_some()
    }

    fn apply_scale(self: &Rc<Self>, factor: f64) -> bool {
        if self.canvas.tool.get() != Tool::Scale { return false; }
        let mut editor = self.editor.borrow_mut();
        let Some(bounds) = editor.selection_scale_bounds() else { return false; };
        if factor == 1.0 { return true; }
        let pivot = crate::canvas::scale::anchor(bounds, self.scale_anchor.get());
        if !editor.scale_selection(pivot, factor) { return false; }
        drop(editor);
        self.changed();
        true
    }

    pub(super) fn commit_scale_field(self: &Rc<Self>, name: &str, text: &str) -> bool {
        let text = text.trim();
        let factor = if name == "ScaleFactorField" {
            let (digits, unit) = if let Some(percent) = text.strip_suffix('%') {
                (percent, 0.01)
            } else {
                (text.trim_end_matches(['x', '×']), 1.0)
            };
            digits.trim().parse::<f64>().ok().map(|value| value * unit)
        } else {
            let bounds = self.editor.borrow().selection_scale_bounds();
            bounds.and_then(|bounds| {
                let extent = if name == "ScaleWidthField" { bounds.width } else { bounds.height };
                text.parse::<f64>().ok().map(|value| value / extent)
            })
        };
        if !factor.is_some_and(|factor| factor.is_finite() && factor > 0.0 && self.apply_scale(factor)) {
            return self.reject_inspector_field(name, "Enter a positive scale within the document limits.");
        }
        self.field(name).set_border_brush(None);
        true
    }
}
