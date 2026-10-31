//! Uniform scaling in world space. A gesture owns its transaction and original
//! lengths. Pointer samples never compound a previous preview.
use crate::{
    parse_layout_guide_binding, parse_layout_variable_binding, Editor, EditorRenderToken,
    EditorTransactionToken, GridTrack, LayoutGridPattern, LayoutGuideVariableField, NetworkCurve,
    Node, NodeId, NodeKind, PathSegment, Point, Rect, Transform, MAX_EFFECT_DISTANCE,
    MAX_EFFECT_RADIUS,
};

#[derive(Clone, Debug)]
pub struct SelectionScale {
    transaction: EditorTransactionToken,
    version: EditorRenderToken,
    bounds: Rect,
    ids: Vec<NodeId>,
    layers: Vec<LayerScale>,
    last: Option<(Point, f64)>,
    max_factor: f64,
    smallest_length: f64,
}

#[derive(Clone, Debug)]
struct LayerScale {
    position: Point,
    parent_inverse: Option<Transform>,
    center: Point,
    affine: Option<Transform>,
    retained: bool,
    lengths: Vec<f64>,
}

fn scale_roots(editor: &Editor) -> impl Iterator<Item = (NodeId, Transform, Rect)> + '_ {
    editor.root_selection().into_iter().filter_map(|id| {
        let document = &editor.document;
        let node = document.nodes.get(&id)?;
        let mut ancestor = node.parent?;
        while let Some(parent) = document.nodes.get(&ancestor) {
            if parent.kind == NodeKind::Instance {
                return None;
            }
            ancestor = parent.parent?;
        }
        if ancestor != editor.active_page {
            return None;
        }
        let inverse = document.world_transform(node.parent?)?.inverse()?;
        let bounds = document.world_bounds(id)?;
        bounds.is_finite().then_some((id, inverse, bounds))
    })
}

// An instance's local affine survives component refresh. Applying scale at the
// same boundary preserves linked dimensions when styles or variables change.
fn retains_geometry(node: &Node) -> bool {
    node.kind == NodeKind::Instance
        || node.text_style.is_some()
        || node.effect_style.is_some()
        || node.grid_style.is_some()
        || node.variable_bindings.keys().any(|key| {
            matches!(
                key.as_str(),
                "width" | "height" | "corner_radius" | "font_size"
            ) || parse_layout_variable_binding(key).is_some()
                || matches!(
                    parse_layout_guide_binding(key),
                    Some((
                        _,
                        LayoutGuideVariableField::Size
                            | LayoutGuideVariableField::Gutter
                            | LayoutGuideVariableField::Offset
                    ))
                )
        })
}

impl Editor {
    pub fn selection_scale_bounds(&self) -> Option<Rect> {
        scale_roots(self)
            .map(|(_, _, bounds)| bounds)
            .reduce(Rect::union)
    }

    pub fn begin_scale_selection(&mut self) -> Option<SelectionScale> {
        if self.has_active_transaction() {
            return None;
        }
        let roots: Vec<_> = scale_roots(self).collect();
        let bounds = roots.iter().map(|entry| entry.2).reduce(Rect::union)?;
        let mut pending: Vec<_> = roots
            .into_iter()
            .map(|(id, inverse, _)| (id, Some(inverse)))
            .collect();
        let mut ids = Vec::new();
        let mut layers = Vec::new();
        let mut max_factor = f64::MAX;
        let mut smallest_length = f64::MAX;
        while let Some((id, parent_inverse)) = pending.pop() {
            let node = self.document.nodes.get_mut(&id)?;
            let retained = retains_geometry(node);
            let mut lengths = Vec::new();
            if !retained {
                visit_lengths(node, |value, limit| {
                    lengths.push(*value);
                    let magnitude = value.abs();
                    if magnitude != 0.0 {
                        max_factor = max_factor.min(limit / magnitude);
                        smallest_length = smallest_length.min(magnitude);
                    }
                });
                pending.extend(node.children.iter().map(|child| (*child, None)));
            }
            ids.push(id);
            layers.push(LayerScale {
                position: Point::new(node.x, node.y),
                center: node.local_bounds().center(),
                affine: node.affine_transform,
                parent_inverse,
                lengths,
                retained,
            });
        }
        self.begin_node_transaction(&ids, "Scale layers");
        Some(SelectionScale {
            transaction: self.active_transaction_token()?,
            version: self.render_change_token(),
            bounds,
            layers,
            ids,
            max_factor,
            smallest_length,
            last: None,
        })
    }

    pub fn scale_selection(&mut self, pivot: Point, factor: f64) -> bool {
        if factor == 1.0 {
            return false;
        }
        let Some(mut scale) = self.begin_scale_selection() else {
            return false;
        };
        if scale.preview(self, pivot, factor) {
            scale.commit(self)
        } else {
            scale.cancel(self);
            false
        }
    }
}

impl SelectionScale {
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn preview(&mut self, editor: &mut Editor, pivot: Point, factor: f64) -> bool {
        if !self.owns(editor)
            || editor.render_change_token() != self.version
            || !pivot.x.is_finite()
            || !pivot.y.is_finite()
            || !factor.is_finite()
            || factor <= 0.0
            || self.smallest_length * factor == 0.0
            || (factor != 1.0 && factor > self.max_factor)
        {
            return false;
        }
        if self.last == Some((pivot, factor)) || (factor == 1.0 && self.last.is_none()) {
            return true;
        }
        // Validate all positions before publishing any edits; a finite
        // multiplier can overflow when applied to a distant world pivot.
        let Some(placements) = self
            .layers
            .iter()
            .map(|layer| layer.placement(pivot, factor))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        for ((id, layer), (position, affine)) in self.ids.iter().zip(&self.layers).zip(placements) {
            let node = editor
                .document
                .nodes
                .get_mut(id)
                .expect("unchanged scale transaction");
            node.x = position.x;
            node.y = position.y;
            node.affine_transform = affine;
            if !layer.retained {
                let mut original = layer.lengths.iter();
                visit_lengths(node, |value, _| *value = original.next().unwrap() * factor);
            }
        }
        editor.refresh_preview_nodes(&self.ids);
        self.version = editor.render_change_token();
        self.last = Some((pivot, factor));
        true
    }

    fn owns(&self, editor: &Editor) -> bool {
        editor.active_transaction_token().as_ref() == Some(&self.transaction)
    }

    pub fn commit(self, editor: &mut Editor) -> bool {
        self.owns(editor) && editor.commit_transaction()
    }

    pub fn cancel(self, editor: &mut Editor) {
        if self.owns(editor) {
            editor.cancel_transaction();
        }
    }
}

impl LayerScale {
    fn placement(&self, world_pivot: Point, factor: f64) -> Option<(Point, Option<Transform>)> {
        if factor == 1.0 {
            return Some((self.position, self.affine));
        }
        // Uniform scale commutes with any linear parent transform. Only the
        // root's translation depends on the pivot; descendants scale about
        // their parent's local origin as its coordinate system expands.
        let pivot = self
            .parent_inverse
            .map_or(Point::default(), |inverse| inverse.apply(world_pivot));
        let mut position = self.position * factor + pivot * (1.0 - factor);
        let affine = if self.retained {
            position = position + self.center * (factor - 1.0);
            let about_center = Transform::translation(
                self.center.x * (1.0 - factor),
                self.center.y * (1.0 - factor),
            ) * Transform::scale(factor, factor);
            Some(about_center * self.affine.unwrap_or(Transform::IDENTITY))
        } else {
            self.affine.map(|a| Transform {
                e: a.e * factor,
                f: a.f * factor,
                ..a
            })
        };
        let matrix = affine.unwrap_or(Transform::IDENTITY);
        [
            position.x, position.y, matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f,
        ]
        .iter()
        .all(|v| v.is_finite())
        .then_some((position, affine))
    }
}

// Snapshot and restore exactly the same scalar slots, without copying text,
// image data, gradients or style stacks per pointer sample. Relative paints,
// line-height ratios, stroke-profile multipliers and fractional tracks are
// dimensionless and intentionally have no entries here.
fn visit_lengths(n: &mut Node, mut visit: impl FnMut(&mut f64, f64)) {
    let any = f64::MAX;
    visit(&mut n.width, any);
    visit(&mut n.height, any);
    visit(&mut n.corner_radius, any);
    if let Some(radii) = &mut n.corner_radii {
        for r in radii {
            visit(r, any);
        }
    }
    for stroke in &mut n.strokes {
        visit(&mut stroke.width, any);
        for length in &mut stroke.dash {
            visit(length, any);
        }
    }
    for effect in &mut n.effects {
        visit(&mut effect.radius, MAX_EFFECT_RADIUS);
        visit(&mut effect.spread, MAX_EFFECT_DISTANCE);
        visit(&mut effect.offset.x, MAX_EFFECT_DISTANCE);
        visit(&mut effect.offset.y, MAX_EFFECT_DISTANCE);
    }
    if n.kind == NodeKind::Text {
        visit(&mut n.font_size, any);
        visit(&mut n.letter_spacing, any);
        for size in n.text_runs.iter_mut().filter_map(|r| r.font_size.as_mut()) {
            visit(size, any);
        }
        for p in &mut n.text_paragraphs {
            for length in [
                &mut p.space_before,
                &mut p.space_after,
                &mut p.indent,
                &mut p.letter_spacing,
            ]
            .into_iter()
            .flatten()
            {
                visit(length, 1_000_000.0);
            }
        }
    }
    let mut point = |p: &mut Point| {
        visit(&mut p.x, any);
        visit(&mut p.y, any);
    };
    for segment in &mut n.path.segments {
        match segment {
            PathSegment::MoveTo { point: p } | PathSegment::LineTo { point: p } => point(p),
            PathSegment::QuadraticTo { control, point: p } => {
                point(control);
                point(p);
            }
            PathSegment::CubicTo {
                control1,
                control2,
                point: p,
            } => {
                point(control1);
                point(control2);
                point(p);
            }
            PathSegment::Close => {}
        }
    }
    if let Some(network) = &mut n.vector_network {
        for vertex in network.vertices.values_mut() {
            point(&mut vertex.position);
        }
        for edge in network.edges.values_mut() {
            if let NetworkCurve::Cubic { control1, control2 } = &mut edge.curve {
                point(control1);
                point(control2);
            }
        }
    }
    if let Some(layout) = &mut n.layout {
        for length in [
            &mut layout.gap,
            &mut layout.cross_gap,
            &mut layout.padding.top,
            &mut layout.padding.bottom,
            &mut layout.padding.left,
            &mut layout.padding.right,
        ] {
            visit(length, any);
        }
        for track in layout
            .row_tracks
            .iter_mut()
            .chain(&mut layout.column_tracks)
        {
            if let GridTrack::Fixed(size) = track {
                visit(size, any);
            }
        }
    }
    let child = &mut n.layout_child;
    visit(&mut child.min_width, any);
    visit(&mut child.min_height, any);
    for length in [&mut child.max_width, &mut child.max_height]
        .into_iter()
        .flatten()
    {
        visit(length, any);
    }
    for guide in n.layout_grid.iter_mut().chain(&mut n.layout_grid_overlays) {
        visit(&mut guide.size, any);
        if let LayoutGridPattern::Columns { axis } | LayoutGridPattern::Rows { axis } =
            &mut guide.pattern
        {
            visit(&mut axis.gutter, any);
            visit(&mut axis.offset, any);
        }
    }
}
