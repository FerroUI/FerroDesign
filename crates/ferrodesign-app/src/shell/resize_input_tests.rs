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

#[test]
fn transparent_star_edge_clicks_pick_geometry_while_drags_resize_the_selection() {
    let fixture = Fixture::with_kind(false, NodeKind::Star);
    let shell = &fixture.shell;
    let canvas = &shell.canvas;
    let mut doc = core::Document::new("Rounded star edge hit testing");
    let mut back = Node::new(NodeKind::Rectangle, "Behind the tip", 20.0, 20.0, 400.0, 250.0).with_fill(core::Color::WHITE);
    back.visible = false;
    let back = doc.insert_node(1, back).unwrap();
    // The affine star and the rounded-away tip are the browser polygon-corners fixture.
    let mut star = Node::new(NodeKind::Star, "Rounded star", 100.0, 60.0, 160.0, 140.0)
        .with_fill(core::Color::WHITE).with_radius(20.25);
    star.rotation = 17.0;
    star.flip_x = true;
    star.polygon_sides = 5;
    star.affine_transform = Some(core::Transform { a: 1.0, b: 0.15, c: 0.2, d: 0.9, e: 0.0, f: 0.0 });
    let transform = star.local_transform();
    let star = doc.insert_node(1, star).unwrap();
    let mut editor = Editor::new(doc);
    editor.selection = vec![star];
    *shell.editor.borrow_mut() = editor;
    shell.refresh();
    fixture.window.capture_rendered_frame().unwrap();
    let view = core::Viewport { zoom: 2.0, pan: Point::new(40.0, 40.0), ..canvas.viewport.get() };
    canvas.viewport.set(view);
    canvas.invalidate_visual();
    fixture.window.capture_rendered_frame().unwrap();
    let screen = |local_x, local_y| {
        let p = view.world_to_screen(transform.apply(Point::new(local_x, local_y)));
        canvas.translate_point(ferroui_base::Point::new(p.x, p.y), &fixture.window).unwrap()
    };
    let tip = screen(80.0, 1.0);
    let before = fixture.json();
    fixture.window.mouse_move(tip, RawInputModifiers::NONE);
    assert!(canvas.cursor().is_some(), "the transparent tip is in the rotated side's resize target");
    fixture.window.mouse_down(tip, MouseButton::Left, RawInputModifiers::NONE);
    assert!(!shell.editor.borrow().has_active_transaction());
    let jitter = ferroui_base::Point::new(tip.x + 1.0, tip.y);
    fixture.window.mouse_move(jitter, RawInputModifiers::LEFT_MOUSE_BUTTON);
    fixture.window.mouse_up(jitter, MouseButton::Left, RawInputModifiers::NONE);
    assert!(shell.editor.borrow().selection.is_empty());
    assert_eq!(shell.editor.borrow().history_len(), 0);
    assert_eq!(fixture.json(), before);

    shell.editor.borrow_mut().update_node(back, "Show backdrop", |n| n.visible = true);
    let before = fixture.json();
    let depth = shell.editor.borrow().history_len();
    shell.editor.borrow_mut().select(star, false);
    shell.refresh();
    fixture.window.mouse_down(tip, MouseButton::Left, RawInputModifiers::SHIFT);
    fixture.window.mouse_up(tip, MouseButton::Left, RawInputModifiers::SHIFT);
    assert_eq!(shell.editor.borrow().selection, [star, back]);
    shell.editor.borrow_mut().select(star, false);
    shell.refresh();
    fixture.window.mouse_down(tip, MouseButton::Left, RawInputModifiers::NONE);
    fixture.window.mouse_up(tip, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(shell.editor.borrow().selection, [back]);
    assert_eq!(shell.editor.borrow().history_len(), depth);
    assert_eq!(fixture.json(), before);

    shell.editor.borrow_mut().select(star, false);
    shell.refresh();
    fixture.window.mouse_down(tip, MouseButton::Left, RawInputModifiers::NONE);
    // Coalesced drag crosses the same transparent side without a prior move event.
    fixture.window.mouse_up(screen(80.0, -11.0), MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(shell.editor.borrow().selection, [star]);
    assert!(shell.editor.borrow().document.nodes[&star].height > 150.0);
    assert_eq!(shell.editor.borrow().history_len(), depth + 1);
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);
}

#[test]
fn free_move_commits_final_pointer_position_and_modifiers_once() {
    let fixture = Fixture::with_kind(false, NodeKind::Rectangle);
    let shell = &fixture.shell;
    let canvas = &shell.canvas;
    fixture.window.capture_rendered_frame().unwrap();
    let view = core::Viewport { zoom: 1.0, pan: Point::new(70.0, 70.0), ..canvas.viewport.get() };
    canvas.viewport.set(view);
    canvas.snap.set(false);
    canvas.invalidate_visual();
    fixture.window.capture_rendered_frame().unwrap();
    let screen = |x, y| {
        let p = view.world_to_screen(Point::new(x, y));
        canvas.translate_point(ferroui_base::Point::new(p.x, p.y), &fixture.window).unwrap()
    };
    let before = fixture.json();
    let start = screen(80.0, 80.0);
    let end = screen(103.0, 87.0);

    // With no intermediate sample, pointer-up is the only evidence of a drag.
    fixture.window.mouse_down(start, MouseButton::Left, RawInputModifiers::NONE);
    fixture.window.mouse_up(end, MouseButton::Left, RawInputModifiers::SHIFT);
    let bounds = || shell.editor.borrow().document.nodes[&fixture.id].bounds();
    assert_eq!(bounds(), core::Rect::new(43.0, 20.0, 120.0, 120.0));
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);

    // An earlier unconstrained sample must not override Shift on release.
    fixture.window.mouse_down(start, MouseButton::Left, RawInputModifiers::NONE);
    fixture.window.mouse_move(screen(84.0, 92.0), RawInputModifiers::LEFT_MOUSE_BUTTON);
    assert_eq!(bounds(), core::Rect::new(24.0, 32.0, 120.0, 120.0));
    fixture.window.mouse_up(end, MouseButton::Left, RawInputModifiers::SHIFT);
    assert_eq!(bounds(), core::Rect::new(43.0, 20.0, 120.0, 120.0));
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);

    // A stationary click must retain the redo branch.
    fixture.window.mouse_down(start, MouseButton::Left, RawInputModifiers::NONE);
    fixture.window.mouse_up(start, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(shell.editor.borrow().history_len(), 0);
    assert!(shell.editor.borrow().can_redo());
}
