//! Uniform scale uses a world-space selection box shared by hit testing and feedback.
use super::*;
use ferroui_base::input::{Cursor, StandardCursorType};

pub(crate) fn anchor(bounds: Rect, index: usize) -> Point {
    Point::new(bounds.x + bounds.width * (index % 3) as f64 / 2.0,
               bounds.y + bounds.height * (index / 3) as f64 / 2.0)
}

fn handle_at(bounds: Rect, pointer: Point, tolerance: f64) -> Option<usize> {
    // Prioritize painted corners over an edge when their hit areas meet.
    [0, 2, 6, 8].into_iter().filter_map(|i| {
        let distance = anchor(bounds, i).distance(pointer);
        (distance <= tolerance).then_some((i, distance))
    }).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i).or_else(|| {
        let x = (bounds.x..=bounds.x + bounds.width).contains(&pointer.x);
        let y = (bounds.y..=bounds.y + bounds.height).contains(&pointer.y);
        [(1, x, (pointer.y - bounds.y).abs()), (7, x, (pointer.y - bounds.y - bounds.height).abs()),
         (3, y, (pointer.x - bounds.x).abs()), (5, y, (pointer.x - bounds.x - bounds.width).abs())]
            .into_iter().filter(|(_, within, distance)| *within && *distance <= tolerance)
            .min_by(|a, b| a.2.total_cmp(&b.2)).map(|(i, _, _)| i)
    })
}

pub(super) struct ScaleDrag {
    session: core::SelectionScale,
    handle: usize,
    press: Point,
    factor: f64,
}
impl ScaleDrag {
    pub(super) fn update(&mut self, editor: &mut Editor, pointer: Point, centered: bool) {
        let bounds = self.session.bounds();
        let pivot = anchor(bounds, if centered { 4 } else { 8 - self.handle });
        let ray = anchor(bounds, self.handle) - pivot;
        let length2 = ray.x * ray.x + ray.y * ray.y;
        if length2 <= f64::EPSILON { return; }
        // Project travel onto the original handle ray. Mid-edge handles use
        // one axis and corners preserve aspect even off the diagonal.
        let delta = pointer - self.press;
        let factor = (1.0 + (delta.x * ray.x + delta.y * ray.y) / length2).max(0.001);
        if self.session.preview(editor, pivot, factor) { self.factor = factor; }
    }
    pub(super) fn commit(self, editor: &mut Editor) -> bool { self.session.commit(editor) }
}

impl DesignCanvas {
    pub(super) fn press_scale_handle(&self, world: Point) -> bool {
        let mut editor = self.editor.borrow_mut();
        let Some(handle) = editor.selection_scale_bounds()
            .and_then(|bounds| handle_at(bounds, world, 7.0 / self.viewport.get().zoom))
        else { return false; };
        let Some(session) = editor.begin_scale_selection() else { return false; };
        *self.gesture.borrow_mut() = Gesture::Scale(ScaleDrag { session, handle, press: world, factor: 1.0 });
        true
    }

    pub(crate) fn cancel_scale_drag(&self) -> bool {
        if !matches!(*self.gesture.borrow(), Gesture::Scale(_)) { return false; }
        if let Gesture::Scale(drag) = self.gesture.replace(Gesture::None) {
            drag.session.cancel(&mut self.editor.borrow_mut());
            self.invalidate_visual();
            self.emit(CanvasEvent::Selection);
        }
        true
    }

    pub(super) fn update_scale_cursor(&self, world: Point) {
        let handle = self.editor.borrow().selection_scale_bounds()
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

    pub(super) fn render_scale_handles(&self, ctx: &mut DrawingContext, editor: &Editor, view: Viewport) {
        let Some(bounds) = editor.selection_scale_bounds() else { return; };
        let outline = pen(0xFF0D99FF, 1.0);
        let white = brush("#FFFFFF");
        let top = view.world_to_screen(anchor(bounds, 0));
        ctx.draw_rectangle_outline(&outline, UiRect::new(top.x, top.y, bounds.width * view.zoom, bounds.height * view.zoom), 0.0);
        for index in [0, 1, 2, 3, 5, 6, 7, 8] {
            let p = view.world_to_screen(anchor(bounds, index));
            ctx.draw_rectangle(Some(&white), Some(&outline), UiRect::new(p.x - 3.0, p.y - 3.0, 6.0, 6.0), 0.0, 0.0, &BoxShadows::default());
        }
        if let Gesture::Scale(drag) = &*self.gesture.borrow() {
            let p = view.world_to_screen(anchor(bounds, 7));
            let text = format!("{:.1}%", drag.factor * 100.0);
            let width = text.len() as f64 * 7.0 + 12.0;
            let fill = brush("#0878CC");
            ctx.draw_rectangle(Some(&fill), None, UiRect::new(p.x - width/2.0, p.y + 8.0, width, 20.0), 4.0, 4.0, &BoxShadows::default());
            label(ctx, &text, Point::new(p.x - width/2.0 + 6.0, p.y + 11.0), 11.0, "#FFFFFF");
        }
    }
}
