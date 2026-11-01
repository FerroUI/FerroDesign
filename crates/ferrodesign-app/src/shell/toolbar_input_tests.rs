//! Test the actual Fluent presenter and headless pointer/keyboard devices. Reading
//! Button.Background alone misses the template override that hides a selected glyph.
use super::*;
use ferroui_base::{PixelRect, input::PhysicalKey, platform::PixelFormat};
use ferroui_controls::{MenuItem, automation::AutomationProperties};

#[test]
fn native_dock_feedback_keyboard_and_family_choices() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let window = &fixture.window;
    let before = fixture.json();
    let text = shell.button("TextTool");
    assert!(shell.command("text"));

    let render_tile = || {
        let bitmap = window.capture_rendered_frame().unwrap();
        let sample = text.translate_point(ferroui_base::Point::new(8.0, 8.0), window).unwrap();
        let mut bytes = [0; 4];
        bitmap.copy_pixels(PixelRect::new(sample.x as i32, sample.y as i32, 1, 1), &mut bytes, 4);
        if bitmap.format() == Some(PixelFormat::BGRA8888) { bytes.swap(0, 2); }
        bytes
    };
    assert_eq!(render_tile(), [8, 122, 196, 255], "selected Text is blue at rest");
    let center = text.translate_point(ferroui_base::Point::new(17.0, 20.0), window).unwrap();
    window.mouse_move(center, RawInputModifiers::NONE);
    assert_eq!(render_tile(), [6, 106, 170, 255], "hover retains an accent behind the white glyph");
    window.mouse_down(center, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(render_tile(), [6, 87, 138, 255], "press darkens the same tile");
    window.mouse_up(center, MouseButton::Left, RawInputModifiers::NONE);

    let press = |key, physical| {
        window.key_press(key, RawInputModifiers::NONE, physical, None);
        window.key_release(key, RawInputModifiers::NONE, physical, None);
    };
    assert!(shell.canvas.focus());
    press(Key::F6, PhysicalKey::F6);
    assert!(text.is_focused());
    press(Key::Right, PhysicalKey::ArrowRight);
    assert!(shell.button("CommentTool").is_focused());
    press(Key::End, PhysicalKey::End);
    assert!(shell.button("ActionsButton").is_focused());
    press(Key::Home, PhysicalKey::Home);
    assert!(shell.button("SelectTool").is_focused());
    press(Key::Down, PhysicalKey::ArrowDown);
    let menu = shell.button("MoveToolMenu").flyout().unwrap();
    assert!(menu.is_open());
    menu.hide();
    assert!(shell.button("SelectTool").focus());
    press(Key::Escape, PhysicalKey::Escape);
    assert!(shell.canvas.is_focused());

    assert!(shell.command("scale"));
    assert!(shell.control::<MenuItem>("ScaleChoice").is_checked());
    assert!(!shell.control::<MenuItem>("MoveChoice").is_checked());
    assert_eq!(AutomationProperties::get_name(&shell.button("SelectTool")).as_deref(), Some("Scale"));
    assert_eq!(AutomationProperties::get_item_status(&shell.button("SelectTool")).as_deref(), Some("Active tool"));
    assert_eq!(fixture.json(), before);
}

#[test]
fn narrow_dock_fits_seven_touch_targets_and_preserves_native_family_navigation() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let window = &fixture.window;
    let before = fixture.json();
    assert!(shell.command("ellipse"));
    window.set_width(320.0);
    window.set_height(650.0);
    window.capture_rendered_frame().unwrap();
    let names = ["MoveToolMenu", "RegionToolMenu", "ShapeToolMenu", "DrawingToolMenu", "TextTool", "CommentTool", "ActionsButton"];
    let mut right = 0.0;
    for name in names {
        let button = shell.button(name);
        let start = button.translate_point(ferroui_base::Point::default(), window).unwrap();
        let bounds = button.bounds();
        assert!(bounds.width >= 44.0 && bounds.height >= 44.0, "{name} size {bounds:?}");
        assert!(start.x >= right && start.x + bounds.width <= 320.0, "{name} x={} width={}", start.x, bounds.width);
        right = start.x + bounds.width;
    }
    let shape = shell.button("ShapeToolMenu");
    assert!(!shell.button("RectTool").is_visible());
    assert_eq!(AutomationProperties::get_item_status(&shape).as_deref(), Some("Ellipse"));
    assert!(shell.canvas.focus());
    let press = |key, physical| {
        window.key_press(key, RawInputModifiers::NONE, physical, None);
        window.key_release(key, RawInputModifiers::NONE, physical, None);
    };
    press(Key::F6, PhysicalKey::F6);
    assert!(shape.is_focused());
    press(Key::Left, PhysicalKey::ArrowLeft);
    assert!(shell.button("RegionToolMenu").is_focused());
    press(Key::Home, PhysicalKey::Home);
    assert!(shell.button("MoveToolMenu").is_focused());
    press(Key::End, PhysicalKey::End);
    assert!(shell.button("ActionsButton").is_focused());
    fixture.click_button(&shape, RawInputModifiers::NONE);
    let flyout = shape.flyout().unwrap();
    assert!(flyout.is_open());
    assert!(shell.control::<MenuItem>("EllipseChoice").is_checked());
    flyout.hide();
    window.set_width(980.0);
    window.capture_rendered_frame().unwrap();
    assert!(shell.button("RectTool").is_effectively_visible());
    assert_eq!(AutomationProperties::get_name(&shell.button("RectTool")).as_deref(), Some("Ellipse"));
    assert_eq!(fixture.json(), before);
}
