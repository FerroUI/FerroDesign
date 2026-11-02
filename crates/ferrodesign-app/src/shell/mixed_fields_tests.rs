use super::*;

#[test]
fn mixed_inspector_fields_batch_edit_and_restore_shared_values_through_native_input() {
    let fixture = Fixture::with_kind(false, NodeKind::Rectangle);
    let shell = &fixture.shell;
    let second = {
        let mut editor = shell.editor.borrow_mut();
        let page = editor.active_page;
        let mut other = editor.document.nodes[&fixture.id].clone();
        other.name = "Second".into();
        other.width = 42.0;
        other.corner_radius = 5.0;
        other.opacity = 0.5;
        other.constraints.horizontal = core::Constraint::End;
        other.clip_content = !other.clip_content;
        other.fills = vec![Fill::solid(Color::rgb(20, 30, 180))];
        let id = editor.document.insert_node(page, other).unwrap();
        editor.refresh();
        editor.selection = vec![fixture.id, id];
        id
    };
    shell.refresh();
    let before = fixture.json();
    for name in ["NameField", "WidthField", "RadiusField", "OpacityField", "FillField"] {
        let field = shell.field(name);
        assert_eq!(field.placeholder_text().as_deref(), Some("Mixed"), "{name}");
        assert_eq!(field.text().as_deref(), Some(""), "{name}");
    }
    assert_eq!(shell.field("HeightField").text().as_deref(), Some("120"));
    assert_eq!(shell.control::<CheckBox>("ClipCheck").is_checked(), None);
    let horizontal = shell.control::<ComboBox>("HorizontalConstraint");
    assert_eq!(horizontal.selected_index(), -1);
    assert_eq!(horizontal.placeholder_text().as_deref(), Some("Mixed"));
    assert!(!shell.button("ConstraintLeft").classes().contains("active"));
    assert!(!shell.button("ConstraintRight").classes().contains("active"));
    let radius = shell.field("RadiusField");
    assert!(radius.focus());
    fixture.key(&radius, Key::Enter);
    assert!(!shell.control::<Border>("Toast").is_visible());
    assert_eq!(fixture.json(), before);

    let width = shell.field("WidthField");
    assert!(width.focus());
    width.set_text(Some("93"));
    fixture.key(&width, Key::Escape);
    assert_eq!(width.text().as_deref(), Some(""));
    assert_eq!(width.placeholder_text().as_deref(), Some("Mixed"));
    assert_eq!(fixture.json(), before);

    // The first selected layer already has this value. Typing it into a mixed
    // field must still resize the other layer in a single undo transaction.
    width.set_text(Some("120"));
    fixture.key(&width, Key::Enter);
    assert_eq!(shell.editor.borrow().document.nodes[&second].width, 120.0);
    assert_eq!(width.text().as_deref(), Some("120"));
    assert!(width.placeholder_text().is_none());
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);
    assert_eq!(width.placeholder_text().as_deref(), Some("Mixed"));

    shell.editor.borrow_mut().select(second, false);
    shell.refresh_selection();
    assert_eq!(shell.field("NameField").text().as_deref(), Some("Second"));
    assert_eq!(width.text().as_deref(), Some("42"));
    assert!(width.placeholder_text().is_none());
    assert_eq!(horizontal.selected_index(), constraint_index(core::Constraint::End));
    assert!(horizontal.placeholder_text().is_none());
    assert!(shell.button("ConstraintRight").classes().contains("active"));
}

#[test]
fn mixed_visibility_and_lock_actions_set_one_state_in_one_history_entry() {
    use ferroui_controls::automation::AutomationProperties;
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let other = {
        let mut editor = shell.editor.borrow_mut();
        let mut node = editor.document.nodes[&fixture.id].clone();
        node.visible = false;
        node.locked = true;
        let page = editor.active_page;
        let id = editor.document.insert_node(page, node).unwrap();
        editor.refresh();
        editor.selection = vec![id, fixture.id];
        id
    };
    shell.refresh();
    let labels = || [
        AutomationProperties::get_name(&shell.button("VisibleButton")).unwrap(),
        AutomationProperties::get_name(&shell.button("LockedButton")).unwrap(),
    ];
    assert_eq!(labels(), ["Show all selected layers", "Lock all selected layers"]);
    let before = fixture.json();
    for (button, label) in [("VisibleButton", "Hide selection"), ("LockedButton", "Unlock selection")] {
        let button = shell.button(button);
        fixture.click_button(&button, RawInputModifiers::NONE);
        assert_eq!(AutomationProperties::get_name(&button).as_deref(), Some(label));
    }
    assert!([other, fixture.id].iter().all(|id| {
        let editor = shell.editor.borrow();
        editor.document.nodes[id].visible && editor.document.nodes[id].locked
    }));
    assert_eq!(shell.editor.borrow().history_len(), 2);
    assert!(shell.command("undo"));
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);
    assert_eq!(labels(), ["Show all selected layers", "Lock all selected layers"]);
}

#[test]
fn geometry_equations_commit_once_from_mixed_drafts_and_preserve_variable_undo() {
    let fixture = Fixture::with_kind(false, NodeKind::Rectangle);
    let shell = &fixture.shell;
    let (second, locked) = {
        let mut editor = shell.editor.borrow_mut();
        for (key, id, value) in [("width", 900, 120.25), ("height", 901, 120.0)] {
            editor.document.variables.insert(id, core::Variable {
                id, name: key.into(), collection: "Dimensions".into(),
                values: BTreeMap::from([("Default".into(), core::VariableValue::Number(value))]),
            });
            editor.document.nodes.get_mut(&fixture.id).unwrap().variable_bindings.insert(key.into(), id);
        }
        editor.document.next_id = 902;
        editor.document.nodes.get_mut(&fixture.id).unwrap().width = 120.25;
        let other = Node::new(NodeKind::Rectangle, "Other", 160.0, 20.0, 64.5, 120.0);
        let mut pinned = other.clone();
        pinned.locked = true;
        pinned.width = 200.0;
        let second = editor.document.insert_node(1, other).unwrap();
        let locked = editor.document.insert_node(1, pinned).unwrap();
        editor.refresh();
        editor.selection = vec![fixture.id, second, locked];
        (second, locked)
    };
    shell.refresh();
    let before = fixture.json();
    let width = shell.field("WidthField");
    assert!(width.focus());
    width.set_text(Some("(Mixed / 2) + 6"));
    fixture.key(&width, Key::Enter);
    let after = fixture.json();
    {
        let editor = shell.editor.borrow();
        assert_eq!(editor.history_len(), 1);
        let nodes = &editor.document.nodes;
        assert_eq!((nodes[&fixture.id].width, nodes[&second].width, nodes[&locked].width), (66.125, 38.25, 200.0));
        assert_eq!(nodes[&fixture.id].variable_bindings, BTreeMap::from([("height".into(), 901)]));
    }
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);
    // The first result is valid, but the second denominator is zero.
    assert!(width.focus());
    width.set_text(Some("1 / (x - 64.5)"));
    fixture.key(&width, Key::Enter);
    assert!(width.is_focused());
    assert_eq!(width.text().as_deref(), Some("1 / (x - 64.5)"));
    assert_eq!(fixture.json(), before);
    fixture.key(&width, Key::Escape);
    assert_eq!(width.text().as_deref(), Some(""));
    width.set_text(Some("x"));
    fixture.key(&width, Key::Enter);
    assert_eq!(fixture.json(), before);
    assert!(shell.command("redo"));
    assert_eq!(fixture.json(), after);
    shell.editor.borrow_mut().selection = vec![locked];
    shell.refresh_selection();
    for name in ["XField", "YField", "WidthField", "HeightField", "RotationField"] {
        assert!(!shell.field(name).is_enabled());
    }
}

#[test]
fn geometry_equations_support_absolute_signed_values_and_one_relative_blur() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let before = fixture.json();
    let x = shell.field("XField");
    assert!(x.focus());
    x.set_text(Some("-1.25e1"));
    fixture.key(&x, Key::Enter);
    assert_eq!(shell.editor.borrow().document.nodes[&fixture.id].x, -12.5);
    assert!(x.focus());
    x.set_text(Some("+ 10"));
    assert!(shell.canvas.focus());
    assert_eq!(shell.editor.borrow().document.nodes[&fixture.id].x, -2.5);
    assert!(shell.flush_inspector_drafts());
    assert_eq!(shell.editor.borrow().history_len(), 2);
    let rotation = shell.field("RotationField");
    assert!(rotation.focus());
    rotation.set_text(Some("(5 + 3)^2"));
    fixture.key(&rotation, Key::Enter);
    assert_eq!(shell.editor.borrow().document.nodes[&fixture.id].rotation, 64.0);
    for _ in 0..3 { assert!(shell.command("undo")); }
    assert_eq!(fixture.json(), before);
}

#[test]
fn geometry_field_steps_preserve_authored_precision_mixed_values_and_exact_history() {
    use ferroui_base::input::PhysicalKey;
    let fixture = Fixture::with_kind(false, NodeKind::Rectangle);
    let shell = &fixture.shell;
    let second = {
        let mut editor = shell.editor.borrow_mut();
        editor.document.nodes.get_mut(&fixture.id).unwrap().width = 120.125;
        let second = editor.document.insert_node(1,
            Node::new(NodeKind::Rectangle, "Other", 160.0, 20.0, 48.625, 24.0)).unwrap();
        editor.refresh();
        second
    };
    shell.refresh();
    let before = fixture.json();
    let field = shell.field("WidthField");
    let press = |key, physical, modifiers| {
        assert!(field.is_focused());
        fixture.window.key_press(key, modifiers, physical, None);
        fixture.window.key_release(key, modifiers, physical, None);
        fixture.stop_save_timer();
    };
    assert!(field.focus());
    // The display rounds the stored fraction to two places.
    assert_ne!(field.text().as_deref(), Some("120.125"));
    press(Key::Up, PhysicalKey::ArrowUp, RawInputModifiers::NONE);
    assert_eq!(shell.editor.borrow().document.nodes[&fixture.id].width, 121.125);
    assert!(field.is_focused());
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);

    shell.editor.borrow_mut().selection = vec![fixture.id, second];
    shell.refresh_selection();
    assert!(field.focus());
    assert_eq!(field.placeholder_text().as_deref(), Some("Mixed"));
    press(Key::Down, PhysicalKey::ArrowDown, RawInputModifiers::SHIFT);
    let widths = || {
        let editor = shell.editor.borrow();
        [editor.document.nodes[&fixture.id].width, editor.document.nodes[&second].width]
    };
    assert_eq!(widths(), [110.125, 38.625]);
    assert_eq!(field.placeholder_text().as_deref(), Some("Mixed"));
    assert!(field.is_focused());
    assert_eq!(shell.editor.borrow().history_len(), 1);
    field.set_text(Some("Mixed / 2"));
    press(Key::Up, PhysicalKey::ArrowUp, RawInputModifiers::ALT);
    assert_eq!(widths(), [110.125 / 2.0 + 0.1, 38.625 / 2.0 + 0.1]);
    assert_eq!(shell.editor.borrow().history_len(), 2);
    assert!(shell.flush_inspector_drafts());
    assert_eq!(shell.editor.borrow().history_len(), 2);
    assert!(shell.command("undo"));
    assert_eq!(widths(), [110.125, 38.625]);
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);
}

#[test]
fn geometry_field_step_rejects_invalid_drafts_and_skips_no_op_limits() {
    let fixture = Fixture::with_kind(false, NodeKind::Rectangle);
    let shell = &fixture.shell;
    let field = shell.field("WidthField");
    assert!(field.focus());
    let before = fixture.json();
    field.set_text(Some("1/0"));
    fixture.key(&field, Key::Up);
    assert_eq!(field.text().as_deref(), Some("1/0"));
    assert_eq!(fixture.json(), before);
    assert_eq!(shell.editor.borrow().history_len(), 0);
    fixture.key(&field, Key::Escape);
    field.set_text(Some("2"));
    fixture.key(&field, Key::Down);
    assert_eq!(field.text().as_deref(), Some("1"));
    assert_eq!(shell.editor.borrow().history_len(), 1);
    fixture.key(&field, Key::Down);
    assert_eq!(shell.editor.borrow().history_len(), 1);
    fixture.key(&field, Key::Enter);
    assert!(shell.canvas.is_focused());
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);
}

#[test]
fn native_mixed_text_formats_converge_preserve_existing_weights_and_report_alignment() {
    use ferroui_controls::automation::AutomationProperties;
    let f = Fixture::new(false);
    let shell = &f.shell;
    let (regular, locked) = {
        let mut editor = shell.editor.borrow_mut();
        let first = editor.document.nodes.get_mut(&f.id).unwrap();
        first.kind = NodeKind::Text;
        first.font_weight = 800;
        first.italic = true;
        first.text_align = TextAlign::Right;
        let mut regular = Node::new(NodeKind::Text, "Regular", 160.0, 20.0, 120.0, 60.0);
        regular.font_weight = 450;
        regular.italic = false;
        let mut pinned = regular.clone();
        pinned.locked = true;
        let second = editor.document.insert_node(1, regular).unwrap();
        let locked = editor.document.insert_node(1, pinned).unwrap();
        editor.selection = vec![f.id, second];
        editor.refresh();
        (second, locked)
    };
    shell.refresh();
    let before = f.json();
    let status = |name| AutomationProperties::get_item_status(&shell.button(name)).unwrap();
    let click = |name| {
        let button = shell.button(name);
        button.bring_into_view();
        f.window.capture_rendered_frame().unwrap();
        f.click_button(&button, RawInputModifiers::NONE);
        f.stop_save_timer();
    };
    assert_eq!(status("BoldButton"), "Mixed");
    assert_eq!(status("ItalicButton"), "Mixed");
    assert_eq!(status("TextLeftButton"), "Mixed");
    assert_eq!(status("TextCenterButton"), "Off");
    click("BoldButton");
    assert_eq!(status("BoldButton"), "On");
    assert_eq!(shell.editor.borrow().document.nodes[&f.id].font_weight, 800);
    assert_eq!(shell.editor.borrow().document.nodes[&regular].font_weight, 700);
    click("ItalicButton");
    assert_eq!(status("ItalicButton"), "On");
    click("TextCenterButton");
    assert_eq!(status("TextCenterButton"), "On");
    assert!(!shell.button("TextLeftButton").classes().contains("active"));
    click("TextCenterButton");
    assert_eq!(shell.editor.borrow().history_len(), 3, "Selecting the current alignment is a no-op");
    for _ in 0..3 { assert!(shell.command("undo")); }
    assert_eq!(f.json(), before);
    shell.editor.borrow_mut().selection = vec![f.id, regular, locked];
    shell.refresh_selection();
    click("ItalicButton");
    assert!(!shell.editor.borrow().document.nodes[&locked].italic);
    assert_eq!(status("ItalicButton"), "Mixed");
    assert!(shell.command("undo"));
    assert_eq!(f.json(), before);
    shell.editor.borrow_mut().selection = vec![locked];
    shell.refresh_selection();
    assert!(!shell.button("ItalicButton").is_enabled());
    assert!(!shell.command("bold"));
    assert_eq!(f.json(), before);
}

#[test]
fn geometry_scrub_previews_mixed_values_and_commits_the_release_sample_once() {
    let f = Fixture::with_kind(false, NodeKind::Rectangle);
    let shell = &f.shell;
    let (second, pinned) = {
        let mut e = shell.editor.borrow_mut();
        let first = e.document.nodes.get_mut(&f.id).unwrap();
        first.rotation = 23.0;
        first.width = 120.125;
        let other = Node::new(NodeKind::Rectangle, "Peer", 210.0, 75.0, 46.5, 40.0);
        let second = e.document.insert_node(1, other.clone()).unwrap();
        let mut pinned = other;
        pinned.locked = true;
        let pinned = e.document.insert_node(1, pinned).unwrap();
        e.selection = vec![f.id, second, pinned];
        e.refresh();
        (second, pinned)
    };
    shell.refresh();
    let field = shell.field("WidthField");
    let label = shell.control::<TextBlock>("WidthScrub");
    field.bring_into_view();
    f.window.capture_rendered_frame().unwrap();
    let at = label.translate_point(ferroui_base::Point::new(label.bounds().width * 0.5, label.bounds().height * 0.5), &f.window).unwrap();
    let p = |dx| ferroui_base::Point::new(at.x + dx, at.y);
    let widths = || {
        let e = shell.editor.borrow();
        [e.document.nodes[&f.id].width, e.document.nodes[&second].width, e.document.nodes[&pinned].width]
    };
    let initial = f.json();
    let origin = shell.editor.borrow().document.world_transform(f.id).unwrap().apply(Point::default());
    let plain = RawInputModifiers::NONE;
    let drag = RawInputModifiers::LEFT_MOUSE_BUTTON;
    f.window.mouse_move(at, plain);
    f.window.mouse_down(at, MouseButton::Left, plain);
    f.window.mouse_move(p(2.0), drag);
    assert_eq!(f.json(), initial);
    f.window.mouse_move(p(8.0), drag);
    assert_eq!(widths(), [128.125, 54.5, 46.5]);
    assert_eq!(shell.editor.borrow().history_len(), 0);
    assert!(field.placeholder_text().unwrap().contains("+8"));
    f.window.mouse_move(p(9.0), drag | RawInputModifiers::SHIFT);
    assert_eq!(widths(), [138.125, 64.5, 46.5]);
    f.window.mouse_up(p(10.0), MouseButton::Left, RawInputModifiers::ALT);
    assert_eq!(widths(), [120.125 + 18.1, 46.5 + 18.1, 46.5]);
    let updated_origin = shell.editor.borrow().document.world_transform(f.id).unwrap().apply(Point::default());
    assert!(origin.distance(updated_origin) < 1e-8, "resizing preserves the transformed local origin");
    let after = f.json();
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert!(shell.command("undo"));
    assert_eq!(f.json(), initial);
    assert!(shell.command("redo"));
    assert_eq!(f.json(), after);
}

#[test]
fn geometry_scrub_click_escape_capture_loss_and_save_respect_history_boundaries() {
    use ferroui_base::{input::IPointer, interactivity::RoutingStrategies};
    let f = Fixture::with_kind(false, NodeKind::Rectangle);
    let shell = &f.shell;
    let field = shell.field("XField");
    let label = shell.control::<TextBlock>("XScrub");
    f.window.capture_rendered_frame().unwrap();
    let at = label.translate_point(ferroui_base::Point::new(label.bounds().width / 2.0, label.bounds().height / 2.0), &f.window).unwrap();
    let p = |dx,dy| ferroui_base::Point::new(at.x + dx, at.y + dy);
    let saved_pointer = Rc::new(RefCell::new(None::<Rc<dyn IPointer>>));
    let seen = saved_pointer.clone();
    field.add_handler_with(InputElement::pointer_pressed_event(), move |_,e| {
        seen.replace(Some(e.pointer().clone()));
    }, RoutingStrategies::BUBBLE, true);
    let down = || {
        f.window.mouse_move(at, RawInputModifiers::NONE);
        f.window.mouse_down(at, MouseButton::Left, RawInputModifiers::NONE);
    };
    let drag = RawInputModifiers::LEFT_MOUSE_BUTTON;
    let before = f.json();
    down();
    f.window.mouse_up(at, MouseButton::Left, RawInputModifiers::NONE);
    assert!(field.is_focused());
    assert_eq!(field.selected_text(), field.text().unwrap());
    assert_eq!(f.json(), before);
    down();
    f.window.mouse_move(p(12.0, 60.0), drag);
    assert_eq!(shell.editor.borrow().document.nodes[&f.id].x, 26.0, "down one speed tier halves the delta");
    f.key(&field, Key::Escape);
    f.window.mouse_up(p(12.0,60.0), MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(f.json(), before);
    down();
    f.window.mouse_move(p(15.0,0.0), drag);
    saved_pointer.borrow().as_ref().unwrap().capture(None);
    f.window.mouse_up(p(15.0,0.0), MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(f.json(), before);
    assert_eq!(shell.editor.borrow().history_len(), 0);
    down();
    f.window.mouse_move(p(9.0,0.0), drag);
    assert!(shell.command("commit-edit"));
    let saved: serde_json::Value = serde_json::from_str(&platform::load_document()).unwrap();
    assert_eq!(saved, serde_json::from_str::<serde_json::Value>(&f.json()).unwrap());
    f.window.mouse_up(p(24.0,0.0), MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(shell.editor.borrow().document.nodes[&f.id].x, 29.0);
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert!(shell.command("undo"));
    assert_eq!(f.json(), before);
}
