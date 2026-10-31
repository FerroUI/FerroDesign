//! Exercise S, canvas capture, the native inspector, and the decoded exported PNG.
use super::*;
use ferroui_base::input::PhysicalKey;
use ferroui_base::media::imaging::Bitmap;
use ferroui_base::{PixelRect, PixelSize};

#[test]
fn draw_slice_with_keyboard_and_pointer_and_export_composited_pixels() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let mut doc = core::Document::new("Export crop");
    let page = doc.pages[0].id;
    for (name, x, color) in [("Black", 20.0, Color::BLACK), ("White", 30.0, Color::WHITE)] {
        doc.insert_node(page, Node::new(NodeKind::Rectangle, name, x, 20.0, 25.0, 20.0)
            .with_fill(color)).unwrap();
    }
    *shell.editor.borrow_mut() = Editor::new(doc);
    shell.refresh();
    shell.command("fit");
    shell.command("actual-size");
    fixture.window.capture_rendered_frame().unwrap();
    shell.canvas.focus();
    fixture.window.key_press(Key::S, RawInputModifiers::NONE, PhysicalKey::S, Some("s"));
    fixture.window.key_release(Key::S, RawInputModifiers::NONE, PhysicalKey::S, Some("s"));
    assert_eq!(shell.canvas.tool.get(), Tool::Slice);
    let vp = shell.canvas.viewport.get();
    let window_point = |p: Point| {
        let local = vp.world_to_screen(p);
        shell.canvas.translate_point(ferroui_base::Point::new(local.x, local.y), &fixture.window).unwrap()
    };
    let start = window_point(Point::new(10.0, 10.0));
    let end = window_point(Point::new(70.0, 50.0));
    fixture.window.mouse_move(start, RawInputModifiers::NONE);
    fixture.window.mouse_down(start, MouseButton::Left, RawInputModifiers::NONE);
    fixture.window.mouse_move(end, RawInputModifiers::LEFT_MOUSE_BUTTON);
    fixture.window.mouse_up(end, MouseButton::Left, RawInputModifiers::NONE);

    let id = shell.editor.borrow().selection[0];
    assert_eq!(shell.editor.borrow().document.nodes[&id].kind, NodeKind::Slice);
    assert!(shell.control::<Control>("SliceHint").is_effectively_visible());
    assert!(!shell.control::<Control>("ArtworkOptions").is_effectively_visible());
    let bytes = shell.canvas.export_png_for(&[id]).unwrap();
    let image = Bitmap::from_stream(&mut std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(image.pixel_size(), PixelSize::new(60, 40));
    let mut pixels = vec![0; 60 * 40 * 4];
    image.copy_pixels(PixelRect::new(0, 0, 60, 40), &mut pixels, 60 * 4);
    let pixel = |x, y| {
        let offset: usize = (y * 60 + x) * 4;
        &pixels[offset..offset + 4]
    };
    assert_eq!(pixel(2usize, 2usize)[3], 0, "crop padding must remain transparent");
    assert_eq!(pixel(15, 15), [0, 0, 0, 255]);
    assert_eq!(pixel(25, 15), [255, 255, 255, 255], "overlap follows document stacking order");
    assert_eq!(shell.editor.borrow().history_len(), 1);
    shell.command("undo");
    assert!(!shell.editor.borrow().document.nodes.contains_key(&id));
}
