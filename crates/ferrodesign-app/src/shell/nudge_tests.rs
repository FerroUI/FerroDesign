use super::*;
use ferroui_base::input::PhysicalKey;

#[test]
fn nudge_preferences_edit_validate_cancel_and_apply_to_real_canvas_and_inspector_keys() {
    let f = Fixture::new(false);
    let s = &f.shell;
    s.canvas.nudge.set(platform::NudgeAmounts::default());
    let initial = f.json();
    let control = |name: &str| {
        s.root
            .get_visual_descendants()
            .find_map(|v| {
                v.cast::<Control>()
                    .filter(|c| c.name().as_deref() == Some(name))
            })
            .unwrap()
    };
    let field = |name: &str| control(name).cast::<TextBox>().unwrap();
    let key = |key, mods, physical| {
        f.window.key_press(key, mods, physical, None);
        f.window.key_release(key, mods, physical, None);
        f.stop_save_timer();
    };
    let render = || {
        f.window.capture_rendered_frame().unwrap();
    };
    render();
    assert!(s.command("nudge-settings"));
    render();
    let small = field("NudgeSmall");
    let big = field("NudgeBig");
    assert!(small.is_focused());
    assert_eq!(small.selected_text(), "1");
    small.set_text(Some("0.125"));
    big.set_text(Some("-2"));
    f.key(&small, Key::Enter);
    assert!(big.is_focused() && control("NudgeError").is_visible());
    assert_eq!(s.canvas.nudge.get(), platform::NudgeAmounts::default());
    big.set_text(Some("8.5"));
    f.key(&big, Key::Enter);
    render();
    assert!(s.canvas.is_focused());
    assert_eq!(s.editor.borrow().history_len(), 0);
    assert_eq!(f.json(), initial);
    let original = s.editor.borrow().document.nodes[&f.id].x;
    key(Key::Right, RawInputModifiers::NONE, PhysicalKey::ArrowRight);
    key(Key::Left, RawInputModifiers::SHIFT, PhysicalKey::ArrowLeft);
    assert_eq!(
        s.editor.borrow().document.nodes[&f.id].x,
        original + 0.125 - 8.5
    );
    assert!(s.command("undo"));
    assert!(s.command("undo"));
    assert_eq!(f.json(), initial);
    let x = s.field("XField");
    x.focus();
    key(Key::Up, RawInputModifiers::SHIFT, PhysicalKey::ArrowUp);
    assert_eq!(s.editor.borrow().document.nodes[&f.id].x, original + 8.5);
    assert!(s.command("undo"));
    assert_eq!(f.json(), initial);
    f.window.set_width(320.);
    f.window.set_height(568.);
    render();
    assert!(s.command("nudge-settings"));
    render();
    for name in [
        "NudgeSmall",
        "NudgeBig",
        "NudgeReset",
        "NudgeDone",
        "CloseModalButton",
    ] {
        let c = control(name);
        assert!(
            c.bounds().height >= 44. && c.bounds().width >= 44.,
            "{name}: {:?}",
            c.bounds()
        );
    }
    field("NudgeSmall").set_text(Some("3"));
    key(Key::Escape, RawInputModifiers::NONE, PhysicalKey::Escape);
    assert_eq!(
        platform::NudgeAmounts::load(),
        platform::NudgeAmounts {
            small: 0.125,
            big: 8.5
        }
    );
    assert!(s.canvas.is_focused());
    assert!(s.command("nudge-settings"));
    render();
    f.click_button(
        &control("NudgeReset").cast::<Button>().unwrap(),
        RawInputModifiers::NONE,
    );
    assert_eq!(field("NudgeSmall").text().as_deref(), Some("1"));
    assert_eq!(field("NudgeBig").text().as_deref(), Some("10"));
    f.click_button(
        &control("CloseModalButton").cast::<Button>().unwrap(),
        RawInputModifiers::NONE,
    );
    assert!(s.canvas.is_focused());
    assert_eq!(platform::NudgeAmounts::load(), Default::default());
    assert_eq!(f.json(), initial);
}
