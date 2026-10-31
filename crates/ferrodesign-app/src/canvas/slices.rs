//! Export-region guides belong to editor chrome, outside the artwork render pass.
use super::*;

pub(super) fn draw(ctx: &mut DrawingContext, editor: &Editor, viewport: Viewport) {
    use ferroui_base::media::{immutable::{ImmutableDashStyle, ImmutableSolidColorBrush}, PenLineCap, PenLineJoin};
    let ids = editor.scene.slice_regions_in(viewport.visible_rect());
    if ids.is_empty() { return; }
    let outline: Rc<dyn IPen> = Rc::new(ImmutablePen::new(
        Some(Rc::new(ImmutableSolidColorBrush::from_uint32(0xFF818B9B))), 1.0, Some(Rc::new(ImmutableDashStyle::new(Some(&[4.0, 3.0]), 0.0))),
        PenLineCap::Flat, PenLineJoin::Miter, 10.0,
    ));
    for id in ids {
        let node = &editor.document.nodes[&id];
        let transform = editor.scene.transforms[&id];
        let corners = node.local_bounds().corners().map(|p| viewport.world_to_screen(transform.apply(p)));
        for i in 0..4 {
            ctx.draw_line(&outline, ui(corners[i]), ui(corners[(i + 1) % 4]));
        }
        label(ctx, &node.name, Point::new(corners[0].x, corners[0].y - 16.0), 11.0, "#788596");
    }
}

pub(super) fn pick(editor: &Editor, id: NodeId, world: Point, tolerance: f64) -> bool {
    // Let the user select artwork through an empty, unselected slice. Its
    // edge or Layers row selects it; its entire interior can then be dragged.
    if editor.selection.contains(&id) { return true; }
    let node = &editor.document.nodes[&id];
    let corners = node.local_bounds().corners().map(|p| editor.scene.transforms[&id].apply(p));
    (0..4).any(|i| core::distance_to_segment(world, corners[i], corners[(i + 1) % 4]) <= tolerance)
}
