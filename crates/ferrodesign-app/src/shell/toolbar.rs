//! Keep each split tool's last choice when switching between tools. Canvas shortcuts,
//! the actions palette, and flyout clicks all converge on the same Tool event.
use super::*;
use ferroui_base::input::{KeyModifiers, NavigationMethod};
use ferroui_base::interactivity::RoutingStrategies;
use ferroui_controls::{MenuItem, MenuItemToggleType, ToolTip, automation::AutomationProperties};

#[derive(Default)]
pub(super) struct ToolbarState {
    displayed: Cell<Option<Tool>>,
    choices: Cell<[usize; 4]>,
}

struct Group {
    primary: &'static str,
    menu: &'static str,
    // XAML item, canvas tool, editor command, vector glyph
    choices: &'static [(&'static str, Tool, &'static str, &'static str)],
}

const GROUPS: [Group; 4] = [
    Group {
        primary: "SelectTool",
        menu: "MoveToolMenu",
        choices: &[
            ("MoveChoice", Tool::Select, "select", "pointer"),
            ("HandChoice", Tool::Hand, "hand", "hand"),
            ("ScaleChoice", Tool::Scale, "scale", "scale"),
        ],
    },
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
        self.wire_dock_navigation();
        for (index, group) in GROUPS.iter().enumerate() {
            let menu = self.button(group.menu);
            menu.set_content(Some(Control::boxed(icons::icon(
                "chevron-down",
                "#686868",
                7.0,
            ))));
            for (choice_index, &(name, _, command, glyph)) in group.choices.iter().enumerate() {
                let item = self.control::<MenuItem>(name);
                item.set_toggle_type(MenuItemToggleType::Radio);
                item.set_group_name(Some(group.menu.into()));
                item.set_is_checked(choice_index == self.toolbar.choices.get()[index]);
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

    // Native buttons keep their Space/Enter, pointer, flyout and focus behavior.
    // Arrow navigation moves inside the dock with a visible directional focus cue.
    fn wire_dock_navigation(self: &Rc<Self>) {
        let names: Vec<_> = GROUPS.iter()
            .flat_map(|group| [group.primary, group.menu])
            .chain(["TextTool", "CommentTool", "ActionsButton"])
            .collect();
        let dock = Rc::new(names);
        let weak = Rc::downgrade(self);
        self.canvas.add_handler_with(InputElement::key_down_event(), move |_, e| {
            if e.key != Key::F6 || !e.key_modifiers.is_empty() { return; }
            let Some(shell) = weak.upgrade() else { return; };
            if !shell.control::<Border>("Toolbar").is_effectively_visible() { return; }
            let tool = shell.canvas.tool.get();
            let name = GROUPS.iter().find(|group| group.choices.iter().any(|choice| choice.1 == tool))
                .map(|group| group.primary)
                .unwrap_or(if tool == Tool::Text { "TextTool" } else { "CommentTool" });
            e.set_handled(shell.button(name).focus_with(NavigationMethod::Directional, KeyModifiers::NONE));
        }, RoutingStrategies::TUNNEL, false);
        for (index, &name) in dock.iter().enumerate() {
            let weak = Rc::downgrade(self);
            let dock = dock.clone();
            self.button(name).add_handler_with(InputElement::key_down_event(), move |_, e| {
                if !e.key_modifiers.is_empty() { return; }
                let Some(shell) = weak.upgrade() else { return; };
                let target = match e.key {
                    Key::Right => (index + 1) % dock.len(),
                    Key::Left => (index + dock.len() - 1) % dock.len(),
                    Key::Home => 0,
                    Key::End => dock.len() - 1,
                    Key::Down => {
                        if let Some(group) = GROUPS.iter().find(|group| group.primary == name || group.menu == name) {
                            let owner = shell.button(group.menu);
                            if let Some(flyout) = owner.flyout() {
                                flyout.show_at(&owner);
                                e.set_handled(true);
                            }
                        }
                        return;
                    }
                    Key::Escape | Key::F6 => {
                        e.set_handled(shell.canvas.focus());
                        return;
                    }
                    _ => return,
                };
                let button = shell.button(dock[target]);
                if button.focus_with(NavigationMethod::Directional, KeyModifiers::NONE) {
                    button.bring_into_view();
                    e.set_handled(true);
                }
            }, RoutingStrategies::TUNNEL, false);
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
            item.set_is_checked(true);
            let shortcut = item.input_gesture().map(|key| key.to_string()).unwrap_or_default();
            let tip = match item.input_gesture() {
                Some(gesture) => format!("{} ({gesture})", tool.name()),
                None => tool.name().to_string(),
            };
            let button = self.button(group.primary);
            AutomationProperties::set_name(&button, Some(tool.name()));
            AutomationProperties::set_accelerator_key(&button, &shortcut);
            ToolTip::set_tip(&button, Some(Rc::new(tip)));
        }
        self.toolbar.choices.set(choices);
        for (name, tool, glyph) in [
            ("TextTool", Tool::Text, "text"),
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
        button.classes().set("accent", selected);
        AutomationProperties::set_item_status(&button, selected.then_some("Active tool"));
        let color = if selected { "#FFFFFF" } else { "#303030" };
        button.set_content(Some(Control::boxed(icons::icon(glyph, color, 17.0))));
    }
}
