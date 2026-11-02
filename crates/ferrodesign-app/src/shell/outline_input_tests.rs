//! Exercise the authored shape, its clipped child and hidden geometry through
//! the native canvas. The same visible geometry supplies pointer hit testing.
use super::*;
use ferroui_base::{input::PhysicalKey, PixelRect};

#[test]
fn outline_shortcut_paints_and_picks_clipped_and_hidden_artwork_without_editing() {
    let f = Fixture::new(false);
    let shell = &f.shell;
    let canvas = &shell.canvas;
    let window = &f.window;
    let mut doc = core::Document::new("Outline view");
    let mut frame = Node::new(NodeKind::Frame, "Clipping frame", 50., 40., 100., 90.)
        .with_fill(core::Color::rgb(228, 50, 44));
    frame.clip_content = true;
    let frame = doc.insert_node(1, frame).unwrap();
    let child = doc
        .insert_node(
            frame,
            Node::new(NodeKind::Ellipse, "Overflow", 70., 20., 80., 60.)
                .with_fill(core::Color::rgb(42, 165, 85)),
        )
        .unwrap();
    let mut hidden = Node::new(NodeKind::Rectangle, "Hidden artwork", 240., 50., 80., 60.);
    hidden.visible = false;
    let hidden = doc.insert_node(1, hidden).unwrap();
    *shell.editor.borrow_mut() = Editor::new(doc);
    shell.refresh();
    window.capture_rendered_frame().unwrap();
    let v = core::Viewport {
        zoom: 1.,
        pan: Point::new(32., 32.),
        ..canvas.viewport.get()
    };
    canvas.viewport.set(v);
    canvas.set_tool(crate::canvas::Tool::Select);
    let at = |x, y| {
        let p = v.world_to_screen(Point::new(x, y));
        canvas
            .translate_point(ferroui_base::Point::new(p.x, p.y), window)
            .unwrap()
    };
    let patch = |x, y| {
        let image = window.capture_rendered_frame().unwrap();
        let p = at(x, y);
        let mut rgba = [0u8; 100];
        image.copy_pixels(
            PixelRect::new(p.x as i32 - 2, p.y as i32 - 2, 5, 5),
            &mut rgba,
            20,
        );
        rgba
    };
    let has_edge = |x, y| {
        patch(x, y)
            .chunks_exact(4)
            .any(|c| c[0].max(c[1]).max(c[2]) < 220)
    };
    let click = |x, y| {
        let p = at(x, y);
        window.mouse_move(p, RawInputModifiers::NONE);
        window.mouse_down(p, MouseButton::Left, RawInputModifiers::NONE);
        window.mouse_up(p, MouseButton::Left, RawInputModifiers::NONE);
    };
    let toggle = || {
        let m = RawInputModifiers::CONTROL | RawInputModifiers::SHIFT;
        window.key_press(Key::O, m, PhysicalKey::O, None);
        window.key_release(Key::O, m, PhysicalKey::O, None);
    };
    let initial = f.json();
    let filled = patch(80., 85.);
    toggle();
    assert!(canvas.show_outlines.get());
    assert!(shell.control::<CheckBox>("ZoomOutlines").is_checked() == Some(true));
    assert!(shell.command("outline-bounds"));
    assert!(patch(80., 85.)
        .chunks_exact(4)
        .all(|p| p[..3].iter().all(|v| *v >= 250)));
    assert!(
        has_edge(200., 90.),
        "the clipped part of the ellipse must appear"
    );
    assert!(
        !has_edge(160., 60. + 3.),
        "a point inside the ellipse must stay unfilled"
    );
    click(200., 90.);
    assert_eq!(shell.editor.borrow().selection, [child]);
    assert!(!has_edge(280., 50.));
    assert!(shell.command("outline-hidden"));
    assert!(has_edge(280., 50.));
    click(280., 50.);
    assert_eq!(shell.editor.borrow().selection, [hidden]);
    shell.editor.borrow_mut().clear_selection();
    canvas.invalidate_visual();
    toggle();
    assert!(!canvas.show_outlines.get());
    assert_eq!(filled, patch(80., 85.));
    assert_eq!(shell.editor.borrow().history_len(), 0);
    assert_eq!(f.json(), initial);
}
