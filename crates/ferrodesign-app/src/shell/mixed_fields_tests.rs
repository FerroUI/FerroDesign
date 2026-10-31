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
