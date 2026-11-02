use super::*;
use ferroui_base::input::PhysicalKey;
use ferroui_controls::automation::AutomationProperties;

#[test]
fn native_layer_search_is_scoped_reversible_and_preserves_other_page_disclosure() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let mut document = core::Document::new("Layer search");
    let page = document.pages[0].id;
    let other_page = document.add_page("Other page");
    let insert = |doc: &mut core::Document, parent, kind, name| {
        doc.insert_node(parent, Node::new(kind, name, 0.0, 0.0, 100.0, 100.0)).unwrap()
    };
    let frame = insert(&mut document, page, NodeKind::Frame, "Card");
    let title = insert(&mut document, frame, NodeKind::Text, "Card title");
    let component = insert(&mut document, other_page, NodeKind::Component, "Menu");
    insert(&mut document, component, NodeKind::Text, "Other title");
    *shell.editor.borrow_mut() = Editor::new(document);
    shell.editor.borrow_mut().select(title, false);
    *shell.expanded.borrow_mut() = BTreeSet::from([component]);
    shell.refresh();
    let initial = fixture.json();
    let rows = || shell.layer_projection.borrow().rows().iter().map(|row| row.id).collect::<Vec<_>>();
    let settle = || {
        Dispatcher::ui_thread().run_jobs(None);
        fixture.window.capture_rendered_frame().unwrap();
    };
    let press = |key, modifiers, physical| {
        fixture.window.key_press(key, modifiers, physical, None);
        fixture.window.key_release(key, modifiers, physical, None);
        settle();
    };
    let search = shell.field("LayerSearch");
    let tree = shell.button("CollapseLayersButton");
    assert_eq!(rows(), [frame]);
    assert_eq!(AutomationProperties::get_name(&tree).as_deref(), Some("Expand all layers"));
    assert!(shell.canvas.focus());
    press(Key::F, RawInputModifiers::CONTROL, PhysicalKey::F);
    assert!(search.is_focused());
    search.set_text(Some("  TITLE  "));
    settle();
    assert_eq!(rows(), [frame, title]);
    assert_eq!(shell.layer_projection.borrow().tree_action(&shell.editor.borrow()), None);
    assert!(!tree.is_visible());
    assert!(!shell.command("collapse-layers"));
    assert!(shell.button("ClearLayerSearch").is_effectively_visible());

    press(Key::Escape, RawInputModifiers::NONE, PhysicalKey::Escape);
    assert!(search.is_focused());
    assert_eq!(rows(), [frame]);
    assert_eq!(*shell.expanded.borrow(), BTreeSet::from([component]));
    press(Key::Escape, RawInputModifiers::NONE, PhysicalKey::Escape);
    assert!(shell.canvas.is_focused());
    assert_eq!(shell.editor.borrow().selection, [title]);
    assert!(shell.command("collapse-layers"));
    assert_eq!(rows(), [frame, title]);
    assert_eq!(*shell.expanded.borrow(), BTreeSet::from([frame, component]));
    assert_eq!(AutomationProperties::get_name(&tree).as_deref(), Some("Selected layers remain visible"));
    assert!(!tree.is_enabled());
    assert!(!shell.command("collapse-layers"));
    assert_eq!(rows(), [frame, title]);
    shell.editor.borrow_mut().clear_selection();
    shell.refresh_selection();
    assert!(tree.is_enabled());
    assert_eq!(AutomationProperties::get_name(&tree).as_deref(), Some("Collapse all layers"));
    assert!(shell.command("collapse-layers"));
    assert_eq!(*shell.expanded.borrow(), BTreeSet::from([component]));

    shell.editor.borrow_mut().select(title, false);
    shell.command("assets");
    assert_eq!(search.placeholder_text().as_deref(), Some("Search components…"));
    assert_eq!(AutomationProperties::get_name(&search).as_deref(), Some("Search components"));
    assert_eq!(rows(), [component]);
    assert!(!tree.is_visible());
    search.set_text(Some("Missing"));
    settle();
    assert!(rows().is_empty());
    assert!(shell.control::<TextBlock>("EmptyLayers").text().unwrap().contains("No matching components"));
    fixture.click_button(&shell.button("ClearLayerSearch"), RawInputModifiers::NONE);
    settle();
    assert!(search.is_focused());
    assert_eq!(rows(), [component]);
    assert_eq!(*shell.expanded.borrow(), BTreeSet::from([component]));
    fixture.window.set_width(390.0);
    fixture.window.set_height(844.0);
    settle();
    assert!(shell.canvas.focus());
    press(Key::F, RawInputModifiers::META, PhysicalKey::F);
    assert!(search.is_focused());
    assert!(shell.control::<Border>("LeftPanel").is_effectively_visible());
    search.set_text(Some("title"));
    settle();
    press(Key::Escape, RawInputModifiers::NONE, PhysicalKey::Escape);
    assert!(search.is_focused());
    assert_eq!(rows(), [frame]);
    press(Key::Escape, RawInputModifiers::NONE, PhysicalKey::Escape);
    assert!(!shell.control::<Border>("LeftPanel").is_effectively_visible());
    assert!(shell.canvas.is_focused());
    assert_eq!(shell.editor.borrow().selection, [title]);
    assert_eq!(fixture.json(), initial);
    assert_eq!(shell.editor.borrow().history_len(), 0);
}

#[test]
fn layer_rows_move_on_release_and_cancel_without_touching_the_document() {
    use ferroui_base::input::MouseButton;
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let mut doc = core::Document::new("Row drag");
    let page = doc.pages[0].id;
    let group = doc.insert_node(page, Node::new(NodeKind::Group, "Destination", 200., 95., 150., 80.)).unwrap();
    doc.nodes.get_mut(&group).unwrap().rotation = 30.;
    let source = doc.insert_node(page, Node::new(NodeKind::Rectangle, "Source", 20., 14., 54., 32.)).unwrap();
    let guard = doc.insert_node(page, Node::new(NodeKind::Frame, "Immovable", 100., 70., 40., 40.)).unwrap();
    doc.nodes.get_mut(&guard).unwrap().locked = true;
    *shell.editor.borrow_mut() = Editor::new(doc);
    shell.editor.borrow_mut().selection = vec![guard];
    shell.refresh();
    fixture.window.capture_rendered_frame().unwrap();
    let point = |id| {
        let row = shell.layer_buttons.borrow().get(&id).unwrap().upgrade().unwrap();
        row.translate_point(ferroui_base::Point::new(row.bounds().width * 0.5, row.bounds().height * 0.5), &fixture.window).unwrap()
    };
    let start = point(source);
    let target = point(group);
    let down = || {
        fixture.window.mouse_move(start, RawInputModifiers::NONE);
        fixture.window.mouse_down(start, MouseButton::Left, RawInputModifiers::NONE);
        fixture.window.mouse_move(target, RawInputModifiers::LEFT_MOUSE_BUTTON);
    };
    let before = fixture.json();
    down();
    assert_eq!(fixture.json(), before);
    assert_eq!(shell.editor.borrow().selection, [guard]);
    fixture.window.key_press(Key::Escape, RawInputModifiers::NONE, PhysicalKey::Escape, None);
    fixture.window.key_release(Key::Escape, RawInputModifiers::NONE, PhysicalKey::Escape, None);
    fixture.window.mouse_up(target, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(fixture.json(), before);
    down();
    fixture.window.mouse_up(target, MouseButton::Left, RawInputModifiers::NONE);
    assert_eq!(shell.editor.borrow().document.nodes[&source].parent, Some(group));
    assert_eq!(shell.editor.borrow().selection, [source]);
    assert!(shell.expanded.borrow().contains(&group));
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);
    assert_eq!(shell.editor.borrow().selection, [guard]);
}

#[test]
fn layer_keys_follow_filtered_and_virtualized_rows_and_protect_each_selected_path() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let mut doc = core::Document::new("Keyboard layers");
    let page = doc.pages[0].id;
    let insert = |doc: &mut core::Document, parent, kind, name: &str| {
        doc.insert_node(parent, Node::new(kind, name, 0., 0., 20., 20.)).unwrap()
    };
    let last = insert(&mut doc, page, NodeKind::Rectangle, "Footnote");
    for index in 0..160 {
        insert(&mut doc, page, NodeKind::Rectangle, &format!("Line {index}"));
    }
    let other = insert(&mut doc, page, NodeKind::Group, "Other");
    insert(&mut doc, other, NodeKind::Rectangle, "Unrelated");
    let first = insert(&mut doc, page, NodeKind::Frame, "Header");
    let title = insert(&mut doc, first, NodeKind::Text, "Primary title");
    let second = insert(&mut doc, page, NodeKind::Frame, "Sidebar");
    let subtitle = insert(&mut doc, second, NodeKind::Text, "Secondary title");
    *shell.editor.borrow_mut() = Editor::new(doc);
    *shell.expanded.borrow_mut() = BTreeSet::from([other, first, second]);
    shell.editor.borrow_mut().selection = vec![title, subtitle];
    shell.refresh();
    let settled = || {
        Dispatcher::ui_thread().run_jobs(None);
        fixture.window.capture_rendered_frame().unwrap();
    };
    settled();
    let before = fixture.json();
    let tree = shell.button("CollapseLayersButton");
    assert_eq!(AutomationProperties::get_name(&tree).as_deref(), Some("Collapse other layers"));
    fixture.click_button(&tree, RawInputModifiers::NONE);
    settled();
    assert_eq!(*shell.expanded.borrow(), BTreeSet::from([first, second]));
    assert_eq!(shell.editor.borrow().selection, [title, subtitle]);
    assert_eq!(AutomationProperties::get_name(&tree).as_deref(), Some("Expand all layers"));

    let press = |key, mods, physical| {
        fixture.window.key_press(key, mods, physical, None);
        fixture.window.key_release(key, mods, physical, None);
        settled();
    };
    let row = |id| shell.layer_buttons.borrow()[&id].upgrade().unwrap();
    fixture.click_button(&row(second), RawInputModifiers::NONE);
    assert!(row(second).is_focused());
    press(Key::Right, RawInputModifiers::NONE, PhysicalKey::ArrowRight);
    assert!(row(subtitle).is_focused());
    assert_eq!(shell.editor.borrow().selection, [subtitle]);
    press(Key::Down, RawInputModifiers::SHIFT, PhysicalKey::ArrowDown);
    assert_eq!(shell.editor.borrow().selection, [subtitle, first]);
    press(Key::Up, RawInputModifiers::SHIFT, PhysicalKey::ArrowUp);
    assert_eq!(shell.editor.borrow().selection, [subtitle]);
    press(Key::End, RawInputModifiers::CONTROL, PhysicalKey::End);
    assert!(row(last).is_focused());
    assert_eq!(shell.editor.borrow().selection, [subtitle]);
    press(Key::Space, RawInputModifiers::CONTROL, PhysicalKey::Space);
    assert_eq!(shell.editor.borrow().selection, [subtitle, last]);
    assert!(shell.layer_buttons.borrow().values().filter(|row| row.upgrade().is_some()).count() < 80);

    press(Key::Home, RawInputModifiers::NONE, PhysicalKey::Home);
    assert!(row(second).is_focused());
    press(Key::Left, RawInputModifiers::NONE, PhysicalKey::ArrowLeft);
    assert!(!shell.expanded.borrow().contains(&second));
    assert!(row(second).is_focused());
    press(Key::F, RawInputModifiers::CONTROL, PhysicalKey::F);
    let search = shell.field("LayerSearch");
    assert!(search.is_focused());
    search.set_text(Some("title"));
    settled();
    press(Key::Down, RawInputModifiers::NONE, PhysicalKey::ArrowDown);
    assert!(row(subtitle).is_focused());
    press(Key::Left, RawInputModifiers::NONE, PhysicalKey::ArrowLeft);
    assert!(row(second).is_focused());
    press(Key::Right, RawInputModifiers::NONE, PhysicalKey::ArrowRight);
    assert!(row(subtitle).is_focused());
    assert!(!shell.expanded.borrow().contains(&second));
    press(Key::Escape, RawInputModifiers::NONE, PhysicalKey::Escape);
    assert!(shell.canvas.is_focused());
    assert_eq!(fixture.json(), before);
    assert_eq!(shell.editor.borrow().history_len(), 0);
}
