use super::*;

#[test]
fn move_tool_edge_resize_commits_coalesced_release_and_cancels_with_escape() {
    let fixture = Fixture::with_kind(false, NodeKind::Rectangle);
    let shell = &fixture.shell;
    let canvas = &shell.canvas;
    fixture.window.capture_rendered_frame().unwrap();
    let mut view = canvas.viewport.get();
    view.zoom = 1.0;
    view.pan = Point::new(80.0, 80.0);
    canvas.viewport.set(view);
    canvas.invalidate_visual();
    fixture.window.capture_rendered_frame().unwrap();
    let screen = |world| {
        let p = view.world_to_screen(world);
        canvas.translate_point(ferroui_base::Point::new(p.x, p.y), &fixture.window).unwrap()
    };
    let bounds = || shell.editor.borrow().document.nodes[&fixture.id].bounds();
    let before = fixture.json();
    let original = bounds();
    let press = screen(Point::new(original.right() - 2.0, original.y + original.height * 0.35));
    let release = screen(Point::new(original.right() + 24.0, original.y + original.height * 0.65));
    fixture.window.mouse_move(press, RawInputModifiers::NONE);
    fixture.window.mouse_down(press, MouseButton::Left, RawInputModifiers::NONE);
    // A touch/desktop host may coalesce the only move into pointer-up.
    fixture.window.mouse_up(release, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(bounds(), core::Rect::new(original.x, original.y, original.width + 26.0, original.height));
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);
    fixture.window.mouse_move(press, RawInputModifiers::NONE);
    fixture.window.mouse_down(press, MouseButton::Left, RawInputModifiers::NONE);
    fixture.window.mouse_move(release, RawInputModifiers::LEFT_MOUSE_BUTTON);
    assert_ne!(bounds(), original);
    let mut event = KeyEventArgs::new();
    event.set_routed_event(Some(InputElement::key_down_event()));
    event.key = Key::Escape;
    canvas.raise_event(&event);
    assert!(event.handled());
    fixture.window.mouse_up(release, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(fixture.json(), before);
    assert!(shell.editor.borrow().can_redo());
}
