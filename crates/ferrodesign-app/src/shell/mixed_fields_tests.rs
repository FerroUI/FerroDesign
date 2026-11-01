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
