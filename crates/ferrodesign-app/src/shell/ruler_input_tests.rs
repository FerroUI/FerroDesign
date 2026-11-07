use super::*;
use ferroui_base::input::PhysicalKey;

#[test]
fn ruler_gestures_use_release_coordinates_and_keep_artwork_out_of_guide_history() {
    let f = Fixture::new(false);
    let shell = &f.shell;
    let canvas = &shell.canvas;
    f.window.capture_rendered_frame().unwrap();
    assert!(canvas.focus());
    f.window
        .key_press(Key::R, RawInputModifiers::SHIFT, PhysicalKey::R, None);
    f.window
        .key_release(Key::R, RawInputModifiers::SHIFT, PhysicalKey::R, None);
    assert!(canvas.show_rulers.get());
    let v = canvas.viewport.get();
    let at = |x, y| {
        canvas
            .translate_point(ferroui_base::Point::new(x, y), &f.window)
            .unwrap()
    };
    let plain = RawInputModifiers::NONE;
    let held = RawInputModifiers::LEFT_MOUSE_BUTTON;
    let source = at(75., 8.);
    let middle = at(75., 80.);
    let end = at(75., 120.);
    let before = f.json();
    let page = shell.editor.borrow().active_page;
    let selection = shell.editor.borrow().selection.clone();
    f.window.mouse_move(source, plain);
    f.window.mouse_down(source, MouseButton::Left, plain);
    f.window.mouse_move(middle, held);
    assert_eq!(f.json(), before);
    f.window.mouse_up(end, MouseButton::Left, plain);
    let first = canvas.selected_ruler_guide().unwrap();
    let initial = *shell.editor.borrow().document.ruler_guide(first).unwrap();
    assert_eq!(first.owner, page);
    assert!(initial.horizontal);
    assert_eq!(
        initial.position,
        v.screen_to_world(Point::new(75., 120.)).y.round()
    );
    assert_eq!(shell.editor.borrow().history_len(), 1);
    let screen_y = v.world_to_screen(Point::new(0., initial.position)).y;
    let line = at(77., screen_y);
    f.window.mouse_move(line, plain);
    f.window
        .mouse_down(line, MouseButton::Left, RawInputModifiers::ALT);
    f.window
        .mouse_move(at(77., screen_y + 31.), held | RawInputModifiers::ALT);
    f.window.mouse_up(
        at(77., screen_y + 35.),
        MouseButton::Left,
        RawInputModifiers::ALT,
    );
    assert_eq!(shell.editor.borrow().document.guides[&page].len(), 2);
    assert_eq!(shell.editor.borrow().history_len(), 2);
    let copy = canvas.selected_ruler_guide().unwrap();
    let copy_y = v
        .world_to_screen(Point::new(
            0.,
            shell
                .editor
                .borrow()
                .document
                .ruler_guide(copy)
                .unwrap()
                .position,
        ))
        .y;
    let snapshot = f.json();
    f.window.mouse_move(at(90., copy_y), plain);
    f.window
        .mouse_down(at(90., copy_y), MouseButton::Left, plain);
    f.window.mouse_move(at(90., 200.), held);
    f.window
        .key_press(Key::Escape, plain, PhysicalKey::Escape, None);
    f.window
        .key_release(Key::Escape, plain, PhysicalKey::Escape, None);
    f.window.mouse_up(at(90., 200.), MouseButton::Left, plain);
    assert_eq!(f.json(), snapshot);
    f.window
        .key_press(Key::Delete, plain, PhysicalKey::Delete, None);
    f.window
        .key_release(Key::Delete, plain, PhysicalKey::Delete, None);
    assert_eq!(shell.editor.borrow().document.guides[&page], [initial]);
    assert_eq!(shell.editor.borrow().selection, selection);
    assert!(shell.command("undo"));
    assert_eq!(f.json(), snapshot);
}

#[test]
fn phone_ruler_dialog_keeps_exact_position_focus_actions_and_validation_reachable() {
    let f = Fixture::new(false);
    let shell = &f.shell;
    f.window.set_width(320.0);
    f.window.set_height(568.0);
    let render = || {
        f.window.capture_rendered_frame().unwrap();
    };
    render();
    let control = |name: &str| {
        shell
            .root
            .get_visual_descendants()
            .find_map(|v| {
                v.cast::<Control>()
                    .filter(|c| c.name().as_deref() == Some(name))
            })
            .unwrap_or_else(|| panic!("Expected native ruler control {name}"))
    };
    let button = |name: &str| control(name).cast::<Button>().unwrap();
    let field = |index| {
        control(&format!("RulerPosition{index}"))
            .cast::<TextBox>()
            .unwrap()
    };
    let target = |name: &str| {
        let bounds = control(name).bounds();
        assert!(
            bounds.width >= 44.0 && bounds.height >= 44.0,
            "{name}: {bounds:?}"
        );
    };
    let click = |name: &str| {
        let b = button(name);
        b.bring_into_view();
        render();
        f.click_button(&b, RawInputModifiers::NONE);
        f.stop_save_timer();
    };

    shell.ruler_guides_dialog(0);
    render();
    assert!(button("RulerAddVertical").is_focused());
    for name in [
        "RulerAddVertical",
        "RulerAddHorizontal",
        "RulerDone",
        "CloseModalButton",
    ] {
        target(name);
    }
    click("RulerAddVertical");
    render();
    let position = field(0);
    assert!(position.is_focused());
    assert_eq!(position.selected_text(), position.text().unwrap());
    for name in ["RulerPosition0", "RulerApply0", "RulerRemove0"] {
        target(name);
    }
    let initial = f.json();
    let depth = shell.editor.borrow().history_len();
    position.set_text(Some("oops"));
    f.key(&position, Key::Enter);
    render();
    assert!(position.is_focused() && control("RulerError0").is_effectively_visible());
    assert_eq!(position.text().as_deref(), Some("oops"));
    assert_eq!(shell.editor.borrow().history_len(), depth);
    assert_eq!(f.json(), initial);
    position.set_text(Some("-12.75"));
    assert!(!control("RulerError0").is_visible());
    f.key(&position, Key::Enter);
    render();
    let position = field(0);
    assert!(position.is_focused());
    assert_eq!(position.selected_text(), "-12.75");
    assert_eq!(shell.editor.borrow().history_len(), depth + 1);

    // Eight rows force a scroll at 320px. The final row of the next page
    // owns focus, while Done remains outside the scrolling content.
    for _ in 0..8 {
        click("RulerAddVertical");
    }
    render();
    assert!(field(8).is_focused());
    click("RulerPrevious");
    render();
    assert!(field(0).is_focused());
    click("RulerNext");
    render();
    assert!(field(8).is_focused());
    click("RulerRemove8");
    render();
    assert!(field(7).is_focused());
    let last = control("RulerPosition7");
    let location = |c: &Control| {
        c.translate_point(ferroui_base::Point::default(), &f.window)
            .unwrap()
    };
    let footer = button("RulerDone");
    assert!(location(&last).y >= 0.0);
    assert!(location(&last).y + last.bounds().height <= location(&footer).y);
    assert!(location(&footer).y + footer.bounds().height <= 568.0);
    click("RulerDone");
    assert!(shell.canvas.is_focused());
}

#[test]
fn guide_dropped_over_auto_layout_group_belongs_to_enclosing_frame() {
    let f = Fixture::new(false);
    let shell = &f.shell;
    let mut doc = core::Document::new("Guide over grouped artwork");
    let frame = doc
        .insert_node(1, Node::new(NodeKind::Frame, "Card", 25., 35., 240., 160.))
        .unwrap();
    let mut group = Node::new(NodeKind::Group, "Actions", 22., 28., 100., 50.);
    group.layout = Some(core::AutoLayout::default());
    let group = doc.insert_node(frame, group).unwrap();
    let child = doc
        .insert_node(
            group,
            Node::new(NodeKind::Rectangle, "Button", 0., 0., 80., 40.)
                .with_fill(core::Color::WHITE),
        )
        .unwrap();
    let editor = Editor::new(doc);
    let point = editor
        .document
        .world_transform(child)
        .unwrap()
        .apply(Point::new(20., 15.));
    let local = editor
        .document
        .world_transform(frame)
        .unwrap()
        .inverse()
        .unwrap()
        .apply(point);
    *shell.editor.borrow_mut() = editor;
    shell.refresh();
    f.window.capture_rendered_frame().unwrap();
    let canvas = &shell.canvas;
    let view = core::Viewport {
        zoom: 1.,
        pan: Point::new(40., 40.),
        ..canvas.viewport.get()
    };
    canvas.viewport.set(view);
    canvas.set_rulers_visible(true);
    f.window.capture_rendered_frame().unwrap();
    let screen = view.world_to_screen(point);
    let at = |x, y| {
        canvas
            .translate_point(ferroui_base::Point::new(x, y), &f.window)
            .unwrap()
    };
    let start = at(8., screen.y);
    let end = at(screen.x, screen.y);
    f.window.mouse_move(start, RawInputModifiers::NONE);
    f.window
        .mouse_down(start, MouseButton::Left, RawInputModifiers::NONE);
    f.window
        .mouse_move(end, RawInputModifiers::LEFT_MOUSE_BUTTON);
    f.window
        .mouse_up(end, MouseButton::Left, RawInputModifiers::NONE);
    let guide = canvas.selected_ruler_guide().unwrap();
    assert_eq!(guide.owner, frame);
    assert_eq!(
        shell
            .editor
            .borrow()
            .document
            .ruler_guide(guide)
            .unwrap()
            .position,
        local.x.round()
    );
}

#[test]
fn alt_while_dragging_a_ruler_measures_visible_rotated_artwork_and_cancels_cleanly() {
    let f = Fixture::new(false);
    let shell = &f.shell;
    let canvas = &shell.canvas;
    let mut doc = core::Document::new("Rotated ruler redlines");
    let mut frame = Node::new(NodeKind::Frame, "Frame", 120., 150., 180., 100.);
    frame.rotation = 30.;
    frame.clip_content = true;
    let frame = doc.insert_node(1, frame).unwrap();
    doc.insert_node(
        frame,
        Node::new(NodeKind::Rectangle, "Art", 30., 22., 60., 32.),
    )
    .unwrap();
    let transform = doc.world_transform(frame).unwrap();
    let mut editor = Editor::new(doc);
    editor.selection = vec![frame];
    *shell.editor.borrow_mut() = editor;
    shell.refresh();
    f.window.capture_rendered_frame().unwrap();
    let view = core::Viewport {
        zoom: 1.,
        pan: Point::new(80., 65.),
        ..canvas.viewport.get()
    };
    canvas.viewport.set(view);
    canvas.set_rulers_visible(true);
    f.window.capture_rendered_frame().unwrap();
    assert!(canvas.focus());
    let screen = |p: Point| {
        canvas
            .translate_point(ferroui_base::Point::new(p.x, p.y), &f.window)
            .unwrap()
    };
    let at = |x, y| screen(view.world_to_screen(transform.apply(Point::new(x, y))));
    let painted = |where_: ferroui_base::Point, red: bool| {
        let frame = f.window.capture_rendered_frame().unwrap();
        let bgra = frame.format() == Some(ferroui_base::platform::PixelFormat::BGRA8888);
        let mut bytes = [0; 7 * 7 * 4];
        frame.copy_pixels(
            ferroui_base::PixelRect::new(
                where_.x.round() as i32 - 3,
                where_.y.round() as i32 - 3,
                7,
                7,
            ),
            &mut bytes,
            7 * 4,
        );
        bytes
            .chunks_exact(4)
            .filter(|px| {
                let (r, g, b) = if bgra {
                    (px[2], px[1], px[0])
                } else {
                    (px[0], px[1], px[2])
                };
                if red {
                    r.saturating_sub(g) > 65 && r.saturating_sub(b) > 45
                } else {
                    b.saturating_sub(r) > 100 && g.saturating_sub(r) > 45
                }
            })
            .count()
    };
    let redlines = || canvas.ruler_redlines(&shell.editor.borrow(), view);
    let alt = || {
        f.window.key_press(
            Key::LeftAlt,
            RawInputModifiers::ALT,
            PhysicalKey::AltLeft,
            None,
        )
    };
    let plain = RawInputModifiers::NONE;
    let held = RawInputModifiers::LEFT_MOUSE_BUTTON;
    let before = f.json();
    let depth = shell.editor.borrow().history_len();
    let start = screen(Point::new(7., 190.));
    f.window.mouse_move(start, plain);
    f.window.mouse_down(start, MouseButton::Left, plain);
    f.window.mouse_move(at(55., 36.), held);
    assert!(redlines().is_none());
    alt();
    let spans = redlines().expect("stationary Option shows distances to the hovered child");
    let check_point =
        |p: Point, x, y| assert!(p.distance(transform.apply(Point::new(x, y))) < 1e-6);
    check_point(spans.origin, 55., 36.);
    for (edge, (x, expected)) in spans.edges.into_iter().zip([(30., 25.), (90., 35.)]) {
        let (point, distance) = edge.unwrap();
        check_point(point, x, 36.);
        assert!((distance - expected).abs() < 1e-6);
    }
    assert!(
        painted(at(46., 36.), true) >= 2,
        "the red line crosses the transformed local normal"
    );
    assert!(painted(at(55., 12.), false) >= 2, "the guide remains blue");
    f.window
        .key_release(Key::LeftAlt, plain, PhysicalKey::AltLeft, None);
    assert!(redlines().is_none());
    assert_eq!(
        painted(at(46., 36.), true),
        0,
        "releasing Option removes the redline immediately"
    );
    assert!(
        painted(at(55., 12.), false) >= 2,
        "the underlying guide drag stays active"
    );
    f.window
        .mouse_move(at(110., 70.), held | RawInputModifiers::ALT);
    let frame_spans = redlines().expect("the selected frame supplies its own visible edges");
    for (edge, (x, expected)) in frame_spans.edges.into_iter().zip([(0., 110.), (180., 70.)]) {
        let (point, distance) = edge.unwrap();
        check_point(point, x, 70.);
        assert!((distance - expected).abs() < 1e-6);
    }
    f.window
        .key_press(Key::Escape, plain, PhysicalKey::Escape, None);
    f.window.mouse_up(at(110., 70.), MouseButton::Left, plain);
    assert!(redlines().is_none());
    assert_eq!(f.json(), before);
    assert_eq!(shell.editor.borrow().history_len(), depth);
    assert_eq!(shell.editor.borrow().selection, [frame]);
}
