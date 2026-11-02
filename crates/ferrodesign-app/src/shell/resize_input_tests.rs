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
    let height = shell.field("HeightField");
    height.set_text(Some("unfinished expression"));
    fixture.window.mouse_move(release, RawInputModifiers::LEFT_MOUSE_BUTTON);
    fixture.window.capture_rendered_frame().unwrap();
    assert_ne!(bounds(), original);
    assert_eq!(shell.field("WidthField").text().as_deref(), Some(number(bounds().width).as_str()));
    assert_eq!(height.text().as_deref(), Some("unfinished expression"), "Preview preserves an unrelated dirty field");
    height.set_text(Some(&number(original.height)));
    let mut event = KeyEventArgs::new();
    event.set_routed_event(Some(InputElement::key_down_event()));
    event.key = Key::Escape;
    canvas.raise_event(&event);
    assert!(event.handled());
    fixture.window.mouse_up(release, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(fixture.json(), before);
    fixture.window.capture_rendered_frame().unwrap();
    assert_eq!(shell.field("WidthField").text().as_deref(), Some(number(original.width).as_str()));
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

#[test]
fn option_drag_creates_no_layer_before_drop_and_cancels_without_consuming_redo() {
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
    let start = screen(80.0, 80.0);
    let end = screen(128.0, 89.0);
    let before = fixture.json();
    fixture.window.mouse_down(start, MouseButton::Left, RawInputModifiers::ALT);
    fixture.window.mouse_move(end, RawInputModifiers::ALT | RawInputModifiers::LEFT_MOUSE_BUTTON);
    assert_eq!(fixture.json(), before);
    assert!(!shell.editor.borrow().has_active_transaction());
    fixture.window.mouse_up(end, MouseButton::Left, RawInputModifiers::ALT | RawInputModifiers::SHIFT);
    let id = shell.editor.borrow().selection[0];
    assert_ne!(id, fixture.id);
    assert_eq!(shell.editor.borrow().document.nodes[&id].bounds(), core::Rect::new(68.0, 20.0, 120.0, 120.0));
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);
    for cancel in [false, true] {
        fixture.window.mouse_down(start, MouseButton::Left, RawInputModifiers::ALT);
        if cancel {
            fixture.window.mouse_move(end, RawInputModifiers::ALT | RawInputModifiers::LEFT_MOUSE_BUTTON);
            let mut event = KeyEventArgs::new();
            event.set_routed_event(Some(InputElement::key_down_event()));
            event.key = Key::Escape;
            canvas.raise_event(&event);
        }
        fixture.window.mouse_up(screen(81.0, 80.0), MouseButton::Left, RawInputModifiers::ALT);
        assert_eq!(fixture.json(), before);
        assert!(shell.editor.borrow().can_redo());
    }
    // Letting go of Option before the pointer drops switches back to Move.
    fixture.window.mouse_down(start, MouseButton::Left, RawInputModifiers::ALT);
    fixture.window.mouse_move(end, RawInputModifiers::ALT | RawInputModifiers::LEFT_MOUSE_BUTTON);
    fixture.window.mouse_up(end, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(shell.editor.borrow().selection, [fixture.id]);
    assert_eq!(shell.editor.borrow().document.nodes.len(), 1);
    assert_eq!(shell.editor.borrow().document.nodes[&fixture.id].bounds(), core::Rect::new(68.0, 29.0, 120.0, 120.0));

    assert!(shell.command("undo"));
    fixture.window.mouse_down(start, MouseButton::Left, RawInputModifiers::NONE);
    fixture.window.mouse_move(end, RawInputModifiers::LEFT_MOUSE_BUTTON);
    assert_ne!(fixture.json(), before);
    let mut alt = KeyEventArgs::new();
    alt.set_routed_event(Some(InputElement::key_down_event()));
    alt.key = Key::LeftAlt;
    alt.key_modifiers = ferroui_base::input::KeyModifiers::ALT;
    canvas.raise_event(&alt);
    assert!(alt.handled());
    assert_eq!(fixture.json(), before, "engaging Option restores the original before previewing the copy");
    fixture.window.mouse_up(end, MouseButton::Left, RawInputModifiers::ALT);
    assert_ne!(shell.editor.borrow().selection, [fixture.id]);
    assert_eq!(shell.editor.borrow().document.nodes.len(), 2);
    assert_eq!(shell.editor.borrow().history_len(), 1);
}

#[test]
fn multi_resize_shared_move_handle_uses_final_pointer_sample_and_one_history_entry() {
    let fixture = Fixture::with_kind(false, NodeKind::Rectangle);
    let shell = &fixture.shell;
    let canvas = &shell.canvas;
    let mut doc = core::Document::new("Shared Move handles");
    let a = doc.insert_node(1, Node::new(NodeKind::Rectangle, "A", 20.0, 20.0, 60.0, 50.0)).unwrap();
    let b = doc.insert_node(1, Node::new(NodeKind::Rectangle, "B", 140.0, 60.0, 40.0, 40.0)).unwrap();
    let mut editor = Editor::new(doc);
    editor.selection = vec![a,b];
    *shell.editor.borrow_mut() = editor;
    shell.refresh();
    fixture.window.capture_rendered_frame().unwrap();
    let view = core::Viewport { zoom: 1.0, pan: Point::new(100.0, 75.0), ..canvas.viewport.get() };
    canvas.viewport.set(view);
    canvas.snap.set(false);
    canvas.invalidate_visual();
    fixture.window.capture_rendered_frame().unwrap();
    let screen = |x, y| {
        let p = view.world_to_screen(Point::new(x,y));
        canvas.translate_point(ferroui_base::Point::new(p.x,p.y), &fixture.window).unwrap()
    };
    let before = fixture.json();
    let edge = screen(180.0,60.0);
    let end = screen(220.0,71.0);
    fixture.window.mouse_down(edge, MouseButton::Left, RawInputModifiers::NONE);
    assert!(!shell.editor.borrow().has_active_transaction());
    fixture.window.mouse_up(end, MouseButton::Left, RawInputModifiers::NONE);
    let bounds = |id| shell.editor.borrow().document.nodes[&id].bounds();
    assert_eq!(bounds(a),core::Rect::new(20.0,20.0,75.0,50.0));
    assert_eq!(bounds(b),core::Rect::new(170.0,60.0,50.0,40.0));
    assert_eq!(shell.editor.borrow().selection,[a,b]);
    assert_eq!(shell.editor.borrow().history_len(),1);
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(),before);
    fixture.window.mouse_down(edge, MouseButton::Left, RawInputModifiers::ALT);
    fixture.window.mouse_move(end, RawInputModifiers::ALT | RawInputModifiers::LEFT_MOUSE_BUTTON);
    assert_eq!(bounds(a),core::Rect::new(-20.0,20.0,90.0,50.0));
    let mut shift = KeyEventArgs::new();
    shift.set_routed_event(Some(InputElement::key_down_event()));
    shift.key = Key::LeftShift;
    shift.key_modifiers = ferroui_base::input::KeyModifiers::ALT;
    canvas.raise_event(&shift);
    assert!(shift.handled());
    assert_eq!(bounds(a),core::Rect::new(-20.0,0.0,90.0,75.0));
    let mut cancel = KeyEventArgs::new();
    cancel.set_routed_event(Some(InputElement::key_down_event()));
    cancel.key = Key::Escape;
    canvas.raise_event(&cancel);
    fixture.window.mouse_up(end, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(fixture.json(),before);
    assert!(shell.editor.borrow().can_redo());

    // Two equal widths must continue to show their common live value.
    // Position and height stay mixed, just as they are in the full inspector.
    shell.editor.borrow_mut().update_node(b, "Set equal widths", |node| node.width = 60.0);
    shell.refresh();
    let before_live = fixture.json();
    let depth = shell.editor.borrow().history_len();
    let width = shell.field("WidthField");
    assert_eq!(width.text().as_deref(), Some("60"));
    let edge = screen(200.0, 60.0);
    let end = screen(236.0, 60.0);
    fixture.window.mouse_move(edge, RawInputModifiers::NONE);
    fixture.window.mouse_down(edge, MouseButton::Left, RawInputModifiers::NONE);
    fixture.window.mouse_move(end, RawInputModifiers::LEFT_MOUSE_BUTTON);
    fixture.window.capture_rendered_frame().unwrap();
    assert_eq!(bounds(a).width, 72.0);
    assert_eq!(bounds(b).width, 72.0);
    assert_eq!(width.text().as_deref(), Some("72"));
    assert_eq!(shell.field("XField").placeholder_text().as_deref(), Some("Mixed"));
    assert_eq!(shell.field("HeightField").placeholder_text().as_deref(), Some("Mixed"));
    assert_eq!(shell.editor.borrow().history_len(), depth);
    assert!(canvas.cancel_active_edit());
    fixture.window.mouse_up(end, MouseButton::Left, RawInputModifiers::NONE);
    fixture.window.capture_rendered_frame().unwrap();
    assert_eq!(fixture.json(), before_live);
    assert_eq!(width.text().as_deref(), Some("60"));
    assert_eq!(shell.editor.borrow().history_len(), depth);
}

#[test]
fn selection_rotation_moves_the_origin_then_uses_one_keyboard_or_coalesced_pointer_edit() {
    use ferroui_base::input::KeyModifiers;
    let fixture=Fixture::with_kind(false,NodeKind::Rectangle);
    let shell=&fixture.shell;
    let canvas=&shell.canvas;
    let mut doc=core::Document::new("Rotation input");
    let a=doc.insert_node(1,Node::new(NodeKind::Rectangle,"A",40.,40.,40.,30.)).unwrap();
    let b=doc.insert_node(1,Node::new(NodeKind::Rectangle,"B",130.,40.,40.,30.)).unwrap();
    let mut e=Editor::new(doc);e.selection=vec![a,b];*shell.editor.borrow_mut()=e;
    shell.refresh();fixture.window.capture_rendered_frame().unwrap();
    let view=core::Viewport{zoom:1.,pan:Point::new(100.,100.),..canvas.viewport.get()};
    canvas.viewport.set(view);canvas.invalidate_visual();fixture.window.capture_rendered_frame().unwrap();
    let screen=|p:Point|{let s=view.world_to_screen(p);canvas.translate_point(ferroui_base::Point::new(s.x,s.y),&fixture.window).unwrap()};
    let key=|k,mods|{
        let mut event=KeyEventArgs::new();event.set_routed_event(Some(InputElement::key_down_event()));
        event.key=k;event.key_modifiers=mods;canvas.raise_event(&event);assert!(event.handled());
    };
    let before=fixture.json();
    let pivot=Point::new(40.,40.);let center=Point::new(105.,55.);
    key(Key::R,KeyModifiers::ALT);
    fixture.window.mouse_down(screen(center),MouseButton::Left,RawInputModifiers::NONE);
    fixture.window.mouse_up(screen(pivot),MouseButton::Left,RawInputModifiers::NONE);
    assert_eq!(fixture.json(),before);
    assert_eq!(shell.editor.borrow().history_len(),0);
    let check=|degrees|{
        let orbit=core::Transform::translation(pivot.x,pivot.y)*core::Transform::rotation(degrees)*core::Transform::translation(-pivot.x,-pivot.y);
        let e=shell.editor.borrow();
        for (id,p) in [(a,Point::new(60.,55.)),(b,Point::new(150.,55.))] {
            let node=&e.document.nodes[&id];
            assert!(e.document.world_transform(id).unwrap().apply(node.local_bounds().center()).distance(orbit.apply(p))<1e-7);
            assert!((node.rotation-degrees).abs()<1e-7);
        }
    };
    key(Key::Down,KeyModifiers::CONTROL|KeyModifiers::ALT|KeyModifiers::SHIFT);
    check(15.);assert_eq!(shell.editor.borrow().history_len(),1);
    assert!(shell.command("undo"));assert_eq!(fixture.json(),before);
    let grip=Point::new(105.,16.);
    fixture.window.mouse_down(screen(grip),MouseButton::Left,RawInputModifiers::NONE);
    fixture.window.mouse_up(screen(Point::new(64.,105.)),MouseButton::Left,RawInputModifiers::SHIFT);
    check(90.);
    assert!(shell.command("undo"));assert_eq!(fixture.json(),before);
    fixture.window.mouse_down(screen(grip),MouseButton::Left,RawInputModifiers::NONE);
    fixture.window.mouse_move(screen(Point::new(64.,105.)),RawInputModifiers::LEFT_MOUSE_BUTTON);
    key(Key::Escape,KeyModifiers::NONE);
    fixture.window.mouse_up(screen(Point::new(64.,105.)),MouseButton::Left,RawInputModifiers::NONE);
    assert_eq!(fixture.json(),before);assert_eq!(shell.editor.borrow().selection,[a,b]);
    assert!(shell.editor.borrow().can_redo());
}
