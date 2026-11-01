//! The native playback bar must stay outside the authored hit surface, including on phones.
use super::*;
use ferroui_base::input::PhysicalKey;

#[test]
fn phone_preview_controls_follow_navigation_overlays_and_restore_the_editing_session() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let mut doc = core::Document::new("Checkout");
    let page = doc.pages[0].id;
    let mut frame = |name, x| doc.insert_node(page, Node::new(NodeKind::Frame, name, x, 0.0, 180.0, 300.0)).unwrap();
    let home = frame("Home", 0.0);
    let review = frame("Review order", 220.0);
    let sheet = frame("Confirmation", 440.0);
    let rule = |action| core::Interaction {
        action,
        trigger: core::Trigger::Click,
        transition: core::Transition { kind: core::TransitionKind::Instant, ..Default::default() },
        delay_ms: 0,
        key: None,
    };
    doc.nodes.get_mut(&home).unwrap().interactions.push(rule(core::PrototypeAction::NavigateTo { target: review }));
    doc.nodes.get_mut(&review).unwrap().interactions.push(rule(core::PrototypeAction::OpenOverlay {
        target: sheet, position: Point::new(4.0, 8.0),
    }));
    doc.nodes.get_mut(&sheet).unwrap().interactions.push(core::Interaction {
        trigger: core::Trigger::Key,
        key: Some("R".into()),
        ..rule(core::PrototypeAction::CloseOverlay)
    });
    *shell.editor.borrow_mut() = Editor::new(doc);
    shell.editor.borrow_mut().select(home, false);
    shell.refresh();
    fixture.window.set_width(320.0);
    fixture.window.set_height(650.0);
    let render = || fixture.window.capture_rendered_frame().unwrap();
    render();
    let camera = shell.canvas.viewport.get();
    let authored = fixture.json();
    let back = shell.button("PreviewBack");
    let next = shell.button("PreviewNext");
    let restart = shell.button("PreviewRestart");
    let done = shell.button("PreviewExit");
    let canvas = shell.control::<Grid>("CanvasHost");
    let bar = shell.control::<Border>("PreviewControls");
    let title = shell.control::<TextBlock>("PreviewFrameTitle");
    let click = |button: &Button| { fixture.click_button(button, RawInputModifiers::NONE); render(); };

    assert!(!bar.is_visible());
    assert!(shell.command("present"));
    render();
    assert!(bar.is_visible() && !back.is_enabled());
    assert_eq!(title.text().as_deref(), Some("Home"));
    let bar_y = bar.translate_point(ferroui_base::Point::default(), &fixture.window).unwrap().y;
    assert_eq!(canvas.bounds().height, bar_y);
    let fit = shell.canvas.viewport.get();
    assert!(fit.zoom * 180.0 >= fit.width - 24.0 && fit.zoom * 300.0 <= bar_y);
    fixture.window.set_width(650.0);
    fixture.window.set_height(320.0);
    render();
    let landscape = shell.canvas.viewport.get();
    assert!(landscape.zoom * 300.0 < landscape.height && landscape.zoom * 300.0 >= landscape.height - 24.0);
    fixture.window.set_width(320.0);
    fixture.window.set_height(650.0);
    render();
    assert_eq!(shell.canvas.viewport.get(), fit);
    for button in [&back, &next, &restart, &done] {
        assert!(button.bounds().width >= 44.0 && button.bounds().height >= 44.0);
    }

    let key = |key, physical, modifiers| {
        fixture.window.key_press(key, modifiers, physical, None);
        fixture.window.key_release(key, modifiers, physical, None);
        render();
    };
    let plain = RawInputModifiers::NONE;
    key(Key::Tab, PhysicalKey::Tab, RawInputModifiers::SHIFT);
    assert!(done.is_focused(), "Reverse traversal starts at Done");
    key(Key::Tab, PhysicalKey::Tab, plain);
    assert!(shell.canvas.is_focused(), "Tab from Done returns to the prototype");
    key(Key::Tab, PhysicalKey::Tab, plain);
    assert!(next.is_focused(), "Tab skips the unavailable previous screen");
    key(Key::Tab, PhysicalKey::Tab, plain);
    assert!(restart.is_focused());
    key(Key::Tab, PhysicalKey::Tab, RawInputModifiers::SHIFT);
    assert!(next.is_focused());
    key(Key::Enter, PhysicalKey::Enter, plain);
    assert_eq!(title.text().as_deref(), Some("Review order"));
    key(Key::Left, PhysicalKey::ArrowLeft, plain);
    assert_eq!(title.text().as_deref(), Some("Home"));
    key(Key::Right, PhysicalKey::ArrowRight, plain);
    assert_eq!(title.text().as_deref(), Some("Review order"));
    key(Key::R, PhysicalKey::R, plain);
    assert_eq!(title.text().as_deref(), Some("Home"));
    assert!(shell.prototype.dispatch(home, core::Trigger::Click));
    assert!(shell.prototype.dispatch(review, core::Trigger::Click));
    render();
    assert_eq!(title.text().as_deref(), Some("Confirmation"));
    assert!(back.is_enabled());
    assert!(!next.is_enabled(), "An overlay owns the input surface");
    key(Key::R, PhysicalKey::R, plain);
    assert_eq!(title.text().as_deref(), Some("Review order"), "The overlay's authored R wins over restart");
    assert!(shell.prototype.dispatch(review, core::Trigger::Click));
    key(Key::Escape, PhysicalKey::Escape, plain);
    assert_eq!(title.text().as_deref(), Some("Review order"));
    click(&back);
    assert_eq!(title.text().as_deref(), Some("Home"));
    assert!(!back.is_enabled());
    assert!(shell.prototype.dispatch(home, core::Trigger::Click));
    render();
    click(&restart);
    assert_eq!(title.text().as_deref(), Some("Home"));
    assert!(!back.is_enabled());
    click(&done);
    assert!(!bar.is_visible() && shell.canvas.is_focused());
    assert_eq!(shell.editor.borrow().selection, [home]);
    assert_eq!(shell.canvas.viewport.get(), camera);
    assert_eq!(fixture.json(), authored);
    assert_eq!(shell.editor.borrow().history_len(), 0);
}

#[test]
fn presentation_starts_at_a_visible_board_and_reports_empty_pages() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let mut doc = core::Document::new("Board order");
    let page = doc.pages[0].id;
    let mut add = |kind, name, x| doc.insert_node(page, Node::new(kind, name, x, 0.0, 140.0, 260.0)).unwrap();
    let loose = add(NodeKind::Rectangle, "Decoration", -100.0);
    let hidden = add(NodeKind::Frame, "Hidden flow", 0.0);
    let right = add(NodeKind::Frame, "Second", 300.0);
    let left = add(NodeKind::Frame, "First", 100.0);
    doc.nodes.get_mut(&hidden).unwrap().visible = false;
    doc.pages[0].prototype_start = Some(hidden);
    *shell.editor.borrow_mut() = Editor::new(doc);
    shell.refresh();
    assert!(shell.command("present"));
    assert_eq!(shell.prototype.current(), Some(left));
    assert!(shell.command("prototype-next"));
    assert_eq!(shell.prototype.current(), Some(right));
    assert!(shell.command("prototype-exit"));
    for id in [left, right] {
        shell.editor.borrow_mut().document.nodes.get_mut(&id).unwrap().visible = false;
    }
    assert!(shell.command("present"), "Documents of loose artwork remain playable");
    assert_eq!(shell.prototype.current(), Some(loose));
    assert!(shell.command("prototype-exit"));
    shell.editor.borrow_mut().document.nodes.get_mut(&loose).unwrap().visible = false;
    assert!(!shell.command("present"));
    assert!(!shell.prototype.is_active());
}
