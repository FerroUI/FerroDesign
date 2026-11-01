//! Native inspector calculations use the same bounded evaluator as FerroUI NumberBox.
use super::*;
use ferroui_controls::utils::number_expression::evaluate_number_expression;
use ferroui_base::input::KeyModifiers;
use ferroui_base::interactivity::RoutingStrategies;
use std::borrow::Cow;

#[derive(Clone, Copy)]
pub(super) enum GeometryField { X, Y, Width, Height, Rotation }

impl GeometryField {
    pub(super) fn named(name: &str) -> Option<Self> {
        match name {
            "XField" => Some(Self::X), "YField" => Some(Self::Y),
            "WidthField" => Some(Self::Width), "HeightField" => Some(Self::Height),
            "RotationField" => Some(Self::Rotation), _ => None,
        }
    }

    fn read(self, node: &Node) -> f64 {
        match self {
            Self::X => node.x, Self::Y => node.y, Self::Width => node.width,
            Self::Height => node.height, Self::Rotation => node.rotation,
        }
    }

    fn apply(self, editor: &mut Editor, input: Option<&str>, step: f64) -> Result<bool, String> {
        let expression = input.map(GeometryInput::parse).transpose()?;
        let mut values = BTreeMap::new();
        for &id in &editor.selection {
            if let Some(node) = editor.document.nodes.get(&id) {
                if !editor.document.effective_locked(id) {
                    let current = self.read(node);
                    let baseline = match &expression {
                        Some(expression) => expression.value(current)?,
                        None => current,
                    };
                    let value = checked_value(baseline + step)?;
                    let value = if matches!(self, Self::Width | Self::Height) { value.max(1.0) } else { value };
                    values.insert(id, value);
                }
            }
        }
        if values.is_empty() { return Err("Select an unlocked layer to edit its geometry.".into()); }
        if values.iter().all(|(id, value)| self.read(&editor.document.nodes[id]) == *value) {
            return Ok(false);
        }
        let set: fn(&mut Node, f64) = match self {
            Self::X => |node, value| node.x = value,
            Self::Y => |node, value| node.y = value,
            Self::Rotation => |node, value| node.rotation = value,
            Self::Width | Self::Height => {
                let dimension = if matches!(self, Self::Width) { core::LayerDimension::Width } else { core::LayerDimension::Height };
                editor.set_layer_dimensions(dimension, &values).map_err(|e| e.to_string())?;
                return Ok(true);
            }
        };
        let ids: Vec<_> = values.keys().copied().collect();
        editor.update_nodes_with_layout(&ids, "Edit geometry", |node| {
            let value = values[&node.id];
            if value != self.read(node) {
                set(node, value);
                if node.component_source.is_some() {
                    let key = if matches!(self, Self::Rotation) { "rotation" } else { "position" };
                    node.overridden_properties.insert(key.into());
                }
            }
        });
        Ok(true)
    }
}

enum GeometryInput<'a> {
    Absolute(f64),
    Relative(Cow<'a, str>),
}

impl<'a> GeometryInput<'a> {
    fn parse(input: &'a str) -> Result<Self, String> {
        let input = input.trim().trim_end_matches('%');
        // Retain ordinary signed and scientific literals, including negative coordinates.
        if let Ok(value) = input.parse() { return Ok(Self::Absolute(value)); }
        if input.starts_with(['*', '/', '^']) || input.starts_with("+ ") || input.starts_with("- ") {
            return Ok(Self::Relative(format!("x{input}").into()));
        }
        if input.contains(['x', 'X']) {
            // Both x and Mixed contain this character. Validate the complete
            // tokens in the evaluator rather than rewriting variable names.
            return Ok(Self::Relative(input.into()));
        }
        Self::calculate(input, 0.0).map(Self::Absolute)
    }

    fn calculate(input: &str, current: f64) -> Result<f64, String> {
        evaluate_number_expression(input, |token| {
            let name = token.strip_prefix('-').unwrap_or(token);
            if name.eq_ignore_ascii_case("x") || name.eq_ignore_ascii_case("mixed") {
                Some(if name.len() == token.len() { current } else { -current })
            } else {
                token.parse::<f64>().ok().filter(|n| n.is_finite())
            }
        }).map_err(|error| format!("{error}. Try 320/2 or x+10."))
    }

    fn value(&self, current: f64) -> Result<f64, String> {
        let value = match self {
            Self::Absolute(value) => *value,
            Self::Relative(input) => Self::calculate(input, current)?,
        };
        checked_value(value)
    }
}

fn checked_value(value: f64) -> Result<f64, String> {
    if value.is_finite() && value.abs() <= 1.0e7 {
        Ok(value)
    } else {
        Err("Each result must be a finite number between −10,000,000 and 10,000,000.".into())
    }
}

impl EditorShell {
    pub(super) fn apply_geometry_field(
        self: &Rc<Self>, name: &str, field: GeometryField, input: Option<&str>, step: f64,
    ) -> bool {
        let result = field.apply(&mut self.editor.borrow_mut(), input, step);
        let changed = match result {
            Ok(changed) => changed,
            Err(error) => return self.reject_inspector_field(name, &error),
        };
        self.field(name).set_border_brush(Some(icons::brush("#E5E5E5")));
        if changed { self.changed(); } else { self.refresh_selection(); }
        true
    }

    pub(super) fn wire_geometry_steps(
        self: &Rc<Self>, name: &'static str, property: GeometryField, committed: Rc<RefCell<String>>,
    ) {
        let field = self.field(name);
        let weak_field = field.downgrade();
        let weak_shell = Rc::downgrade(self);
        field.add_handler_with(InputElement::key_down_event(), move |_, event| {
            let sign = match event.key {
                Key::Up => 1.0,
                Key::Down => -1.0,
                _ => return,
            };
            let step = match event.key_modifiers {
                value if value.is_empty() => 1.0,
                KeyModifiers::SHIFT => 10.0,
                KeyModifiers::ALT => 0.1,
                _ => return,
            };
            let (Some(shell), Some(field)) = (weak_shell.upgrade(), weak_field.upgrade()) else { return; };
            if shell.updating.get() || field.is_composing_text() || !field.is_enabled() { return; }
            event.set_handled(true);
            let text = field.text().unwrap_or_default();
            // The native inspector rounds its text for display. Start a clean step
            // from each stored number, even when there is no common displayed value.
            let draft = (text != *committed.borrow()).then_some(text.as_str());
            shell.finish_canvas_edits_for_inspector();
            if shell.apply_geometry_field(name, property, draft, sign * step) {
                // A no-op at the minimum can normalize a draft without changing history.
                *committed.borrow_mut() = field.text().unwrap_or_default();
                field.select_all();
                shell.sync_document_draft_dirty();
            }
        }, RoutingStrategies::TUNNEL, false);
    }
}
