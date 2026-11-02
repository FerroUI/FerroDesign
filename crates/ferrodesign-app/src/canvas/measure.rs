//! Transient distances between layer bounds. Geometry is in document units;
//! the overlay, end caps and text are drawn in screen DIPs.
use super::*;

pub(super) const GUIDE_COLOR: u32 = 0xFFE43C60;

#[derive(Default)]
pub(super) struct Measurements {
    target: Cell<Option<NodeId>>,
    labels: RefCell<[Option<(f64, FormattedText)>; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    from: Point,
    to: Point,
    along_x: bool,
}

fn horizontal(x1: f64, x2: f64, y: f64) -> Span {
    Span { from: Point::new(x1, y), to: Point::new(x2, y), along_x: true }
}
fn vertical(y1: f64, y2: f64, x: f64) -> Span {
    Span { from: Point::new(x, y1), to: Point::new(x, y2), along_x: false }
}

// When projections are disjoint the nearest facing edges define the gap.
// If they intersect, use the midpoint of the shared projection as the ruler.
fn gap(a: f64, b: f64, c: f64, d: f64) -> Option<(f64, f64)> {
    if b <= c {
        Some((b, c))
    } else if d <= a {
        Some((a, d))
    } else {
        None
    }
}

fn spans(a: Rect, b: Rect) -> [Option<Span>; 4] {
    let x = (a.x.max(b.x) + a.right().min(b.right())) * 0.5;
    let y = (a.y.max(b.y) + a.bottom().min(b.bottom())) * 0.5;
    match (gap(a.x, a.right(), b.x, b.right()), gap(a.y, a.bottom(), b.y, b.bottom())) {
        (Some((ax, bx)), Some((ay, by))) => [Some(horizontal(ax, bx, ay)), Some(vertical(ay, by, bx)), None, None],
        (Some((ax, bx)), None) => [Some(horizontal(ax, bx, y)), None, None, None],
        (None, Some((ay, by))) => [Some(vertical(ay, by, x)), None, None, None],
        // Nested and overlapping bounds expose their corresponding edge offsets.
        (None, None) => [
            horizontal(a.x, b.x, y),
            horizontal(a.right(), b.right(), y),
            vertical(a.y, b.y, x),
            vertical(a.bottom(), b.bottom(), x),
        ]
        .map(|span| (span.from != span.to).then_some(span)),
    }
}

impl DesignCanvas {
    pub(crate) fn measurement_target(&self) -> Option<NodeId> {
        self.measurements.target.get()
    }

    pub(super) fn clear_measurement(&self) {
        if self.measurements.target.take().is_some() {
            self.invalidate_visual();
        }
    }

    /// Consumes only idle hover. Pointer presses still go through the normal
    /// cycle-selection, centered-resize and tool handlers.
    pub(super) fn measure_hover(&self, world: Point, modifiers: KeyModifiers) -> bool {
        if !modifiers.contains(KeyModifiers::ALT)
            || self.tool.get() != Tool::Select
            || self.space.get()
            || self.guide_drag_active()
            || self.presentation.get().is_some()
            || !self.is_focused()
            || self.vector_node.get().is_some()
            || self.linear_gradient_active()
            || self.image_crop.get().is_some()
            || self.stroke_width_mode_active()
            || self.editor.borrow().has_active_transaction()
            || self.layout_drag.borrow().is_some()
            || self.editor.borrow().selection.is_empty()
            || !matches!(*self.gesture.borrow(), Gesture::None)
        {
            self.clear_measurement();
            return false;
        }
        if self.hover_edit_handle(world, modifiers) {
            return true;
        }
        let hit = self.pick_with_modifiers(world, modifiers, false);
        let editor = self.editor.borrow();
        let target = hit.and_then(|id| {
            if !editor.selection.contains(&id) {
                return Some(id);
            }
            let &[selected] = editor.selection.as_slice() else {
                return None;
            };
            editor
                .document
                .nodes
                .get(&selected)?
                .parent
                .filter(|parent| editor.document.nodes.contains_key(parent))
        });
        drop(editor);
        let previous_hit = self.hover.replace(hit);
        if self.measurements.target.replace(target) != target || previous_hit != hit {
            self.invalidate_visual();
        }
        true
    }

    pub(super) fn measure_at_pointer(&self, modifiers: KeyModifiers) -> bool {
        self.selection_pointer
            .get()
            .filter(|_| self.is_pointer_over())
            .is_some_and(|screen| self.measure_hover(self.viewport.get().screen_to_world(screen), modifiers))
    }

    pub(super) fn draw_measurements(&self, ctx: &mut DrawingContext, editor: &Editor, v: Viewport) {
        let lines = if let Some(core::RulerRedlines { origin: from, edges }) = self.ruler_redlines(editor, v) {
            let [a, b] = edges.map(|end| end.map(|(to, distance)| {
                let along_x = (to.x - from.x).abs() >= (to.y - from.y).abs();
                (Span { from, to, along_x }, distance)
            }));
            [a, b, None, None]
        } else {
            let Some(target) = self.measurement_target() else { return; };
            let bounds = |id: NodeId| {
                let transform = editor.scene.transforms.get(&id)?;
                Some(transform.transform_rect(editor.document.nodes.get(&id)?.local_bounds()))
            };
            let Some(b) = bounds(target) else { return; };
            let Some(a) = editor.selection.iter().filter_map(|id| bounds(*id)).reduce(Rect::union) else { return; };
            spans(a, b).map(|line| line.map(|span| (span, span.from.distance(span.to))))
        };
        let guide = pen(GUIDE_COLOR, 1.0);
        let ink = brush("#BC244B");
        let view = Rect::new(0.0, 0.0, v.width, v.height);
        let mut labels = self.measurements.labels.borrow_mut();
        let mut bodies = [None; 4];

        for (i, line) in lines.into_iter().enumerate() {
            let Some((span, distance)) = line else { continue; };
            let from = v.world_to_screen(span.from);
            let to = v.world_to_screen(span.to);
            let Some(visible) = Rect::from_points(from, to).intersection(view) else { continue; };
            let direction = to - from;
            let length = from.distance(to);
            let cap = if length > 1e-9 {
                Point::new(-direction.y, direction.x) * (3.0 / length)
            } else if span.along_x {
                Point::new(0., 3.)
            } else {
                Point::new(3., 0.)
            };
            ctx.draw_line(&guide, ui(from), ui(to));
            for end in [from, to] {
                ctx.draw_line(&guide, ui(end - cap), ui(end + cap));
            }
            if labels[i].as_ref().is_none_or(|(old, _)| *old != distance) {
                let precise = format!("{distance:.2}");
                let text = if distance > 0.0 && distance < 0.005 {
                    "<0.01"
                } else {
                    precise.trim_end_matches('0').trim_end_matches('.')
                };
                labels[i] = Some((
                    distance,
                    FormattedText::new(
                        text,
                        CultureInfo::invariant_culture(),
                        FlowDirection::LeftToRight,
                        Typeface::from_name("fonts:Inter#Inter"),
                        11.0,
                        Some(brush("#FFFFFF")),
                    ),
                ));
            }
            let label = &labels[i].as_ref().unwrap().1;
            let (w, h) = (label.width().ceil() + 10.0, label.height().ceil() + 4.0);
            if w + 4.0 > v.width || h + 4.0 > v.height {
                continue;
            }
            let center = visible.center();
            let body = |x: f64, y: f64| {
                Rect::new(x.clamp(2.0, v.width - w - 2.0), y.clamp(2.0, v.height - h - 2.0), w, h)
            };
            let candidates = if span.along_x {
                [body(center.x - w / 2.0, center.y - h - 4.0), body(center.x - w / 2.0, center.y + 4.0)]
            } else {
                [body(center.x + 4.0, center.y - h / 2.0), body(center.x - w - 4.0, center.y - h / 2.0)]
            };
            bodies[i] = candidates
                .into_iter()
                .find(|rect| !bodies[..i].iter().flatten().any(|old: &Rect| old.intersects(*rect)));
        }
        for (entry, body) in labels.iter().zip(bodies) {
            if let (Some((_, label)), Some(body)) = (entry, body) {
                ctx.draw_rectangle(Some(&ink), None, rect(body), 4.0, 4.0, &BoxShadows::default());
                ctx.draw_text(label, ui(Point::new(body.x + 5.0, body.y + 2.0)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_use_only_separated_axes_and_reverse_symmetrically() {
        let selected = Rect::new(-40.0, 10.0, 120.0, 50.0);
        let target = Rect::new(95.125, 20.0, 30.0, 10.0);
        assert_eq!(spans(selected, target), [Some(horizontal(80.0, 95.125, 25.0)), None, None, None]);
        assert_eq!(spans(target, selected), [Some(horizontal(95.125, 80.0, 25.0)), None, None, None]);
        let flush = Rect::new(80.0, 20.0, 30.0, 10.0);
        assert_eq!(spans(selected, flush), [Some(horizontal(80.0, 80.0, 25.0)), None, None, None]);
        let diagonal = Rect::new(95.125, -20.0, 30.0, 12.5);
        assert_eq!(
            spans(selected, diagonal),
            [Some(horizontal(80.0, 95.125, 10.0)), Some(vertical(10.0, -7.5, 95.125)), None, None]
        );
        assert_eq!(
            spans(selected, Rect::new(20.0, 80.0, 30.0, 40.0)),
            [Some(vertical(60.0, 80.0, 35.0)), None, None, None]
        );
    }

    #[test]
    fn parent_insets_use_authored_world_bounds_and_coincident_edges_have_no_length() {
        let parent = Rect::new(20.0, 25.0, 250.0, 130.0);
        let child = Rect::new(53.5, 50.0, 60.0, 30.0);
        assert_eq!(
            spans(child, parent),
            [
                Some(horizontal(53.5, 20.0, 65.0)),
                Some(horizontal(113.5, 270.0, 65.0)),
                Some(vertical(50.0, 25.0, 83.5)),
                Some(vertical(80.0, 155.0, 83.5))
            ]
        );
        assert_eq!(spans(child, child), [None; 4]);
    }
}
