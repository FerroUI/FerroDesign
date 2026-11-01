//! Native inspector calculations use the same bounded evaluator as FerroUI NumberBox.
use super::*;
use ferroui_controls::utils::number_expression::evaluate_number_expression;
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

    pub(super) fn apply(self, editor: &mut Editor, input: &str) -> Result<bool, String> {
        let expression = GeometryInput::parse(input)?;
        let mut values = BTreeMap::new();
        for &id in &editor.selection {
            if let Some(node) = editor.document.nodes.get(&id) {
                if !editor.document.effective_locked(id) {
                    let value = expression.value(self.read(node))?;
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
        editor.update_nodes_with_layout(&ids, "Edit geometry", |node| set(node, values[&node.id]));
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
        if value.is_finite() && value.abs() <= 1.0e7 { Ok(value) } else {
            Err("Each result must be a finite number between −10,000,000 and 10,000,000.".into())
        }
    }
}
