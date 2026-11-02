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
