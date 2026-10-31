//! Keep each split tool's last choice when switching between tools. Canvas shortcuts,
//! the actions palette, and flyout clicks all converge on the same Tool event.
use super::*;
use ferroui_controls::{MenuItem, ToolTip, automation::AutomationProperties};

#[derive(Default)]
pub(super) struct ToolbarState {
    displayed: Cell<Option<Tool>>,
    choices: Cell<[usize; 3]>,
}

struct Group {
    primary: &'static str,
    menu: &'static str,
    // XAML item, canvas tool, editor command, vector glyph
    choices: &'static [(&'static str, Tool, &'static str, &'static str)],
}

const GROUPS: [Group; 3] = [
    Group {
        primary: "FrameTool",
        menu: "RegionToolMenu",
        choices: &[
            ("FrameChoice", Tool::Frame, "frame", "frame"),
            ("SectionChoice", Tool::Section, "section", "section"),
            ("SliceChoice", Tool::Slice, "slice", "slice"),
        ],
    },
    Group {
        primary: "RectTool",
        menu: "ShapeToolMenu",
        choices: &[
            ("RectangleChoice", Tool::Rectangle, "rectangle", "rect"),
            ("LineChoice", Tool::Line, "line", "line"),
            ("ArrowChoice", Tool::Arrow, "arrow", "arrow"),
            ("EllipseChoice", Tool::Ellipse, "ellipse", "ellipse"),
            ("PolygonChoice", Tool::Polygon, "polygon", "polygon"),
            ("StarChoice", Tool::Star, "star", "star"),
        ],
    },
    Group {
        primary: "PenTool",
        menu: "DrawingToolMenu",
        choices: &[
            ("PenChoice", Tool::Pen, "pen", "pen"),
            ("PencilChoice", Tool::Pencil, "pencil", "pencil"),
        ],
    },
];

impl EditorShell {
    pub(super) fn wire_toolbar(self: &Rc<Self>) {
        for (index, group) in GROUPS.iter().enumerate() {
            let menu = self.button(group.menu);
            menu.set_content(Some(Control::boxed(icons::icon(
                "chevron-down",
                "#686868",
                7.0,
            ))));
            for &(name, _, command, glyph) in group.choices {
                let item = self.control::<MenuItem>(name);
                item.set_icon(Some(Control::boxed(icons::icon(glyph, "#303030", 14.0))));
                let weak = Rc::downgrade(self);
                item.click(move |_, _| {
                    if let Some(shell) = weak.upgrade() {
                        shell.command(command);
                    }
                });
            }
            let weak = Rc::downgrade(self);
            self.button(group.primary).click(move |_, _| {
                if let Some(shell) = weak.upgrade() {
                    let choice = group.choices[shell.toolbar.choices.get()[index]];
                    shell.command(choice.2);
                }
            });
        }
    }

    pub(super) fn sync_toolbar(&self) {
        let active = self.canvas.tool.get();
        let previous = self.toolbar.displayed.replace(Some(active));
        if previous == Some(active) {
            return;
        }
        let mut choices = self.toolbar.choices.get();
        for (index, group) in GROUPS.iter().enumerate() {
            let selected = group.choices.iter().position(|choice| choice.1 == active);
            let old = group.choices[choices[index]].1;
            if let Some(selected) = selected {
                choices[index] = selected;
            } else if previous.is_some() && previous != Some(old) {
                continue;
            }
            let (name, tool, _, glyph) = group.choices[choices[index]];
            self.set_tool_glyph(group.primary, glyph, active == tool);
            let item = self.control::<MenuItem>(name);
            let tip = match item.input_gesture() {
                Some(gesture) => format!("{} ({gesture})", tool.name()),
                None => tool.name().to_string(),
            };
            let button = self.button(group.primary);
            AutomationProperties::set_name(&button, Some(tool.name()));
            ToolTip::set_tip(&button, Some(Rc::new(tip)));
        }
        self.toolbar.choices.set(choices);
        for (name, tool, glyph) in [
            ("SelectTool", Tool::Select, "pointer"),
            ("TextTool", Tool::Text, "text"),
            ("HandTool", Tool::Hand, "hand"),
            ("CommentTool", Tool::Comment, "comment"),
        ] {
            if previous.is_none() || active == tool || previous == Some(tool) {
                self.set_tool_glyph(name, glyph, active == tool);
            }
        }
    }

    pub(super) fn open_tool_items(&self) -> Vec<(&'static str, Ref<MenuItem>)> {
        let mut items = Vec::new();
        for group in &GROUPS {
            if self
                .button(group.menu)
                .flyout()
                .is_some_and(|flyout| flyout.is_open())
            {
                items.extend(
                    group
                        .choices
                        .iter()
                        .map(|&(name, _, _, _)| (name, self.control::<MenuItem>(name))),
                );
            }
        }
        items
    }

    fn set_tool_glyph(&self, name: &str, glyph: &str, selected: bool) {
        let button = self.button(name);
        button.classes().set("active", selected);
        let color = if selected { "#FFFFFF" } else { "#303030" };
        button.set_content(Some(Control::boxed(icons::icon(glyph, color, 17.0))));
    }
}
