//! Physical modifier, pointer, and Vello rendering coverage for spacing measurements.
use super::*;
use ferroui_base::{input::PhysicalKey, platform::PixelFormat, PixelRect};

#[test]
fn alt_measurements_use_real_hover_and_release_without_editing() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let canvas = &shell.canvas;
    let window = &fixture.window;
    let mut doc = core::Document::new("Spacing");
    let card = doc
        .insert_node(1, Node::new(NodeKind::Frame, "Card", 30.0, 45.0, 275.0, 175.0).with_fill(core::Color::WHITE))
        .unwrap();
    let a = doc
        .insert_node(
            card,
            Node::new(NodeKind::Rectangle, "Icon", 20.0, 40.0, 48.0, 48.0)
                .with_fill(core::Color::rgb(88, 60, 190)),
        )
        .unwrap();
    let group = doc.insert_node(card, Node::new(NodeKind::Group, "Actions", 116.0, 38.0, 128.0, 52.0)).unwrap();
    let b = doc
        .insert_node(
            group,
            Node::new(NodeKind::Rectangle, "Button", 0.0, 0.0, 128.0, 52.0)
                .with_fill(core::Color::rgb(47, 139, 103)),
        )
        .unwrap();
    *shell.editor.borrow_mut() = Editor::new(doc);
    shell.editor.borrow_mut().select(a, false);
    shell.refresh();
    window.capture_rendered_frame().unwrap();
    let view = core::Viewport { zoom: 2.0, pan: Point::new(40.0, 22.0), ..canvas.viewport.get() };
    canvas.viewport.set(view);
    canvas.invalidate_visual();
    assert!(canvas.focus());
    window.capture_rendered_frame().unwrap();
    let point = |p| {
        let screen = view.world_to_screen(p);
        canvas.translate_point(ferroui_base::Point::new(screen.x, screen.y), window).unwrap()
    };
    let target_center = point(Point::new(195.0, 110.0));
    let before = fixture.json();
    window.mouse_move(target_center, RawInputModifiers::NONE);
    assert_eq!(canvas.measurement_target(), None);
    window.key_press(Key::LeftAlt, RawInputModifiers::ALT, PhysicalKey::AltLeft, None);
    assert_eq!(canvas.measurement_target(), Some(b), "holding Alt at a stationary pointer");
    let red_pixels = || {
        let frame = window.capture_rendered_frame().unwrap();
        let top_left = point(Point::new(98.0, 88.0));
        let mut rgba = vec![0; 96 * 56 * 4];
        frame.copy_pixels(PixelRect::new(top_left.x as i32, top_left.y as i32, 96, 56), &mut rgba, 96 * 4);
        let bgra = frame.format() == Some(PixelFormat::BGRA8888);
        rgba.chunks_exact(4)
            .filter(|p| {
                let (r, g, b) = if bgra { (p[2], p[1], p[0]) } else { (p[0], p[1], p[2]) };
                r > 150 && g < 110 && b < 145 && r > g.saturating_add(90)
            })
            .count()
    };
    assert!(red_pixels() > 200, "the rendered gap has a colored badge and ruler");
    window.key_release(Key::LeftAlt, RawInputModifiers::NONE, PhysicalKey::AltLeft, None);
    assert_eq!(canvas.measurement_target(), None);
    assert_eq!(red_pixels(), 0, "key-up invalidates the stationary overlay");
    window.mouse_move(point(Point::new(74.0, 109.0)), RawInputModifiers::ALT);
    assert_eq!(canvas.measurement_target(), Some(card), "hovering the selection measures its container");
    window.mouse_move(point(Point::new(200.0, 105.0)), RawInputModifiers::ALT);
    assert_eq!(canvas.measurement_target(), Some(b));
    window.key_press(Key::Space, RawInputModifiers::ALT, PhysicalKey::Space, None);
    assert_eq!(canvas.measurement_target(), None);
    window.key_release(Key::Space, RawInputModifiers::ALT, PhysicalKey::Space, None);
    window.mouse_move(target_center, RawInputModifiers::ALT);
    assert_eq!(canvas.measurement_target(), Some(b));
    canvas.select_groups.set(true);
    window.mouse_move(point(Point::new(193.0, 107.0)), RawInputModifiers::ALT);
    assert_eq!(canvas.measurement_target(), Some(group));
    window.key_press(
        Key::LeftCtrl,
        RawInputModifiers::CONTROL | RawInputModifiers::ALT,
        PhysicalKey::ControlLeft,
        None,
    );
    assert_eq!(canvas.measurement_target(), Some(b), "deep-pick modifiers update at a stationary pointer");
    window.key_release(Key::LeftCtrl, RawInputModifiers::ALT, PhysicalKey::ControlLeft, None);
    assert_eq!(canvas.measurement_target(), Some(group));
    assert!(shell.button("SelectTool").focus());
    assert_eq!(
        canvas.measurement_target(),
        None,
        "a key-up delivered to another control cannot leave stale measurements"
    );
    assert_eq!(shell.editor.borrow().selection, [a]);
    assert_eq!(fixture.json(), before);
    assert_eq!(shell.editor.borrow().history_len(), 0);
}
