use super::*;

#[test]
fn marquee_uses_release_coordinates_and_keeps_cancelled_selection_out_of_history() {
    let fixture = Fixture::with_kind(false, NodeKind::Rectangle);
    let shell = &fixture.shell;
    let canvas = &shell.canvas;
    let mut document = core::Document::new("Marquee selection");
    let frame = document.insert_node(1, Node::new(NodeKind::Frame, "Frame", 30.0, 30.0, 120.0, 100.0)).unwrap();
    let child = document.insert_node(frame, Node::new(NodeKind::Rectangle, "Child", 10.0, 10.0, 50.0, 45.0)).unwrap();
    let first = document.insert_node(1, Node::new(NodeKind::Rectangle, "Keep", 200.0, 30.0, 40.0, 40.0)).unwrap();
    let mut editor = Editor::new(document);
    editor.selection = vec![first];
    *shell.editor.borrow_mut() = editor;
    shell.refresh();
    fixture.window.capture_rendered_frame().unwrap();
    let view = core::Viewport { zoom: 1.0, pan: Point::new(60.0, 60.0), ..canvas.viewport.get() };
    canvas.viewport.set(view);
    canvas.invalidate_visual();
    fixture.window.capture_rendered_frame().unwrap();
    let screen = |x, y| {
        let p = view.world_to_screen(Point::new(x, y));
        canvas.translate_point(ferroui_base::Point::new(p.x, p.y), &fixture.window).unwrap()
    };
    let before = fixture.json();
    let start = screen(10.0, 10.0);
    let end = screen(160.0, 140.0);
    fixture.window.mouse_down(start, MouseButton::Left, RawInputModifiers::NONE);
    fixture.window.mouse_move(end, RawInputModifiers::LEFT_MOUSE_BUTTON);
    assert_eq!(shell.editor.borrow().selection, [first], "the preview owns no selection or document edits");
    assert!(!shell.editor.borrow().has_active_transaction());
    let mut escape = KeyEventArgs::new();
    escape.set_routed_event(Some(InputElement::key_down_event()));
    escape.key = Key::Escape;
    canvas.raise_event(&escape);
    fixture.window.mouse_up(end, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(shell.editor.borrow().selection, [first]);

    let mods = RawInputModifiers::SHIFT | RawInputModifiers::CONTROL;
    fixture.window.mouse_down(start, MouseButton::Left, mods);
    fixture.window.mouse_up(end, MouseButton::Left, mods);
    assert_eq!(shell.editor.borrow().selection, [first, child]);
    fixture.window.mouse_down(end, MouseButton::Left, RawInputModifiers::NONE);
    fixture.window.mouse_up(start, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(shell.editor.borrow().selection, [frame], "reverse drag uses the same authored bounds");
    assert_eq!(shell.editor.borrow().history_len(), 0);
    assert_eq!(fixture.json(), before);
}
