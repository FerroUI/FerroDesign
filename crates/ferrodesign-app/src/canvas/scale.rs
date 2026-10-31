//! Uniform scale uses a world-space selection box shared by hit testing and feedback.
use super::*;
use ferroui_base::input::{Cursor, StandardCursorType};

pub(crate) fn anchor(bounds: Rect, index: usize) -> Point {
    Point::new(
        bounds.x + bounds.width * (index % 3) as f64 / 2.0,
        bounds.y + bounds.height * (index / 3) as f64 / 2.0,
    )
}

const HANDLES: [usize; 8] = [0, 2, 6, 8, 1, 3, 5, 7];

fn can_scale_from(bounds: Rect, index: usize) -> bool {
    anchor(bounds, index) != anchor(bounds, 8 - index)
}

pub(super) struct CachedBounds {
    version: core::EditorRenderToken,
    page: NodeId,
    selection: Vec<NodeId>,
    bounds: Option<Rect>,
}

fn handle_at(bounds: Rect, pointer: Point, tolerance: f64) -> Option<usize> {
    // Prioritize painted corners over an edge when their hit areas meet.
    [0, 2, 6, 8]
        .into_iter()
        .filter_map(|i| {
            let distance = anchor(bounds, i).distance(pointer);
            (distance <= tolerance && can_scale_from(bounds, i)).then_some((i, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
        .or_else(|| {
            let x = (bounds.x..=bounds.x + bounds.width).contains(&pointer.x);
            let y = (bounds.y..=bounds.y + bounds.height).contains(&pointer.y);
            [
                (1, x, (pointer.y - bounds.y).abs()),
                (7, x, (pointer.y - bounds.y - bounds.height).abs()),
                (3, y, (pointer.x - bounds.x).abs()),
                (5, y, (pointer.x - bounds.x - bounds.width).abs()),
            ]
            .into_iter()
            .filter(|(i, within, distance)| {
                *within && *distance <= tolerance && can_scale_from(bounds, *i)
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(i, _, _)| i)
        })
}

pub(super) struct ScaleDrag {
    session: core::SelectionScale,
    handle: usize,
    press: Point,
    factor: f64,
    visible_bounds: Rect,
}
impl ScaleDrag {
    pub(super) fn update(&mut self, editor: &mut Editor, pointer: Point, centered: bool) {
        let bounds = self.session.bounds();
        let pivot = anchor(bounds, if centered { 4 } else { 8 - self.handle });
        let ray = anchor(bounds, self.handle) - pivot;
        let length2 = ray.x * ray.x + ray.y * ray.y;
        if length2 <= f64::EPSILON {
            return;
        }
        // Project travel onto the original handle ray. Mid-edge handles use
        // one axis and corners preserve aspect even off the diagonal.
        let delta = pointer - self.press;
        let factor = (1.0 + (delta.x * ray.x + delta.y * ray.y) / length2).max(0.001);
        if self.session.preview(editor, pivot, factor) {
            self.factor = factor;
            let start = pivot + (anchor(bounds, 0) - pivot) * factor;
            self.visible_bounds = Rect::new(
                start.x,
                start.y,
                bounds.width * factor,
                bounds.height * factor,
            );
        }
    }
    pub(super) fn commit(self, editor: &mut Editor) -> bool {
        self.session.commit(editor)
    }
}

impl DesignCanvas {
    // Pointer moves and camera frames share one eligibility/hierarchy walk.
    // Selection changes do not advance the editor's render token.
    fn scale_box(&self, editor: &Editor) -> Option<Rect> {
        if let Gesture::Scale(drag) = &*self.gesture.borrow() {
            return Some(drag.visible_bounds);
        }
        let version = editor.render_change_token();
        let mut cache = self.scale_bounds.borrow_mut();
        if let Some(entry) = cache.as_ref() {
            if entry.version == version
                && entry.page == editor.active_page
                && entry.selection == editor.selection
            {
                return entry.bounds;
            }
        }
        let bounds = editor.selection_scale_bounds();
        *cache = Some(CachedBounds {
            version,
            page: editor.active_page,
            selection: editor.selection.clone(),
            bounds,
        });
        bounds
    }

    pub(super) fn press_scale_handle(&self, world: Point) -> bool {
        let mut editor = self.editor.borrow_mut();
        let Some(handle) = self
            .scale_box(&editor)
            .and_then(|bounds| handle_at(bounds, world, 7.0 / self.viewport.get().zoom))
        else {
            return false;
        };
        let Some(session) = editor.begin_scale_selection() else {
            return false;
        };
        let visible_bounds = session.bounds();
        *self.gesture.borrow_mut() = Gesture::Scale(ScaleDrag {
            session,
            handle,
            press: world,
            factor: 1.0,
            visible_bounds,
        });
        true
    }

    pub(crate) fn cancel_scale_drag(&self) -> bool {
        if !matches!(*self.gesture.borrow(), Gesture::Scale(_)) {
            return false;
        }
        if let Gesture::Scale(drag) = self.gesture.replace(Gesture::None) {
            drag.session.cancel(&mut self.editor.borrow_mut());
            self.invalidate_visual();
            self.emit(CanvasEvent::Selection);
        }
        true
    }

    pub(super) fn update_scale_cursor(&self, world: Point) {
        let handle = self
            .scale_box(&self.editor.borrow())
            .and_then(|bounds| handle_at(bounds, world, 7.0 / self.viewport.get().zoom));
        let kind = match handle {
            Some(0 | 8) => Some(StandardCursorType::TopLeftCorner),
            Some(2 | 6) => Some(StandardCursorType::TopRightCorner),
            Some(1 | 7) => Some(StandardCursorType::SizeNorthSouth),
            Some(3 | 5) => Some(StandardCursorType::SizeWestEast),
            _ => None,
        };
        if self.scale_cursor.replace(kind) != kind {
            self.set_cursor(kind.map(Cursor::new));
        }
    }

    pub(super) fn render_scale_handles(
        &self,
        ctx: &mut DrawingContext,
        editor: &Editor,
        view: Viewport,
    ) {
        let Some(bounds) = self.scale_box(editor) else {
            return;
        };
        let outline = pen(0xFF0D99FF, 1.0);
        let white = brush("#FFFFFF");
        let top = view.world_to_screen(anchor(bounds, 0));
        ctx.draw_rectangle_outline(
            &outline,
            UiRect::new(
                top.x,
                top.y,
                bounds.width * view.zoom,
                bounds.height * view.zoom,
            ),
            0.0,
        );
        for index in HANDLES {
            if !can_scale_from(bounds, index) {
                continue;
            }
            let p = view.world_to_screen(anchor(bounds, index));
            ctx.draw_rectangle(
                Some(&white),
                Some(&outline),
                UiRect::new(p.x - 3.0, p.y - 3.0, 6.0, 6.0),
                0.0,
                0.0,
                &BoxShadows::default(),
            );
        }
        if let Gesture::Scale(drag) = &*self.gesture.borrow() {
            let p = view.world_to_screen(anchor(bounds, 7));
            let text = format!("{:.1}%", drag.factor * 100.0);
            let width = text.len() as f64 * 7.0 + 12.0;
            let fill = brush("#0878CC");
            ctx.draw_rectangle(
                Some(&fill),
                None,
                UiRect::new(p.x - width / 2.0, p.y + 8.0, width, 20.0),
                4.0,
                4.0,
                &BoxShadows::default(),
            );
            label(
                ctx,
                &text,
                Point::new(p.x - width / 2.0 + 6.0, p.y + 11.0),
                11.0,
                "#FFFFFF",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapsed_sides_of_lines_do_not_offer_inert_scale_handles() {
        let line = Rect::new(10.0, 20.0, 150.0, 0.0);
        assert_eq!(handle_at(line, line.center(), 5.0), None);
        assert!(handle_at(line, anchor(line, 3), 5.0).is_some());
        let point = Rect::new(10.0, 20.0, 0.0, 0.0);
        assert_eq!(handle_at(point, point.center(), 5.0), None);
    }
}
