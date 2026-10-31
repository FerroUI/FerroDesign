use super::*;

fn rename_field(fixture: &Fixture) -> Ref<TextBox> {
    fixture.shell.root.get_visual_descendants().into_iter().find_map(|visual| {
        visual.cast::<TextBox>().filter(|field| field.name().as_deref() == Some("LayerRenameEditor"))
    }).expect("the name editor belongs to the realized Layers row")
}

fn press_f2(fixture: &Fixture) {
    let mut event = KeyEventArgs::new();
    event.set_routed_event(Some(InputElement::key_down_event()));
    event.key = Key::F2;
    fixture.shell.canvas.focus();
    fixture.shell.canvas.raise_event(&event);
    assert!(event.handled());
}

#[test]
fn layer_rename_selects_text_and_commits_exactly_once_or_cancels() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let before = fixture.json();
    let viewport = shell.canvas.viewport.get();
    press_f2(&fixture);
    let field = rename_field(&fixture);
    assert_eq!(field.selection_start(), 0);
    assert_eq!(field.selection_end(), 6);
    field.set_text(Some("Customer total"));
    fixture.key(&field, Key::Enter);
    assert!(shell.canvas.is_focused());
    assert_eq!(shell.editor.borrow().selection, [fixture.id]);
    assert_eq!(shell.editor.borrow().document.nodes[&fixture.id].name, "Customer total");
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert_eq!(shell.canvas.viewport.get(), viewport);
    assert!(shell.command("undo"));
    assert_eq!(fixture.json(), before);

    press_f2(&fixture);
    let field = rename_field(&fixture);
    field.set_text(Some("discard me"));
    fixture.key(&field, Key::Escape);
    assert!(shell.canvas.is_focused());
    assert_eq!(fixture.json(), before);

    press_f2(&fixture);
    let field = rename_field(&fixture);
    field.set_text(Some(" "));
    shell.canvas.focus();
    assert_eq!(fixture.json(), before);
    assert!(shell.command("redo"), "cancel and empty names must preserve redo");
    assert_eq!(shell.editor.borrow().document.nodes[&fixture.id].name, "Customer total");
}

#[test]
fn layer_rename_double_clicks_a_realized_row_and_submits_on_blur() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let row = shell.layer_buttons.borrow()[&fixture.id].upgrade().unwrap();
    fixture.click_button(&row, RawInputModifiers::NONE);
    fixture.click_button(&row, RawInputModifiers::NONE);
    let field = rename_field(&fixture);
    assert!(field.is_focused());
    field.set_text(Some("Layer from pointer"));
    assert!(shell.flush_inspector_drafts());
    assert_eq!(shell.editor.borrow().document.nodes[&fixture.id].name, "Layer from pointer");
    assert_eq!(shell.editor.borrow().history_len(), 1);
}

#[test]
fn layer_rename_reveals_a_filtered_collapsed_virtualized_selection() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let mut document = core::Document::new("Virtualized rename");
    let page = document.pages[0].id;
    let frame = document.insert_node(page, Node::new(NodeKind::Frame, "Parent", 0.0, 0.0, 100.0, 100.0)).unwrap();
    let child = document.insert_node(frame, Node::new(NodeKind::Rectangle, "Child", 5.0, 5.0, 30.0, 30.0)).unwrap();
    for index in 0..350 {
        document.insert_node(page, Node::new(NodeKind::Rectangle, format!("Other {index}"), 100.0, 0.0, 20.0, 20.0)).unwrap();
    }
    *shell.editor.borrow_mut() = Editor::new(document);
    shell.expanded.borrow_mut().clear();
    shell.editor.borrow_mut().select(child, false);
    shell.field("LayerSearch").set_text(Some("Other"));
    shell.refresh();
    fixture.window.capture_rendered_frame().unwrap();
    assert!(!shell.layer_buttons.borrow().contains_key(&child));
    press_f2(&fixture);
    let field = rename_field(&fixture);
    assert!(field.is_focused());
    assert!(shell.expanded.borrow().contains(&frame));
    assert_eq!(shell.field("LayerSearch").text().unwrap_or_default(), "");
    assert_eq!(field.text().as_deref(), Some("Child"));
    assert!(shell.layer_buttons.borrow().values().filter(|b| b.upgrade().is_some()).count() < 80);
    field.set_text(Some("Child after scroll"));
    fixture.key(&field, Key::Enter);
    assert_eq!(shell.editor.borrow().document.nodes[&child].name, "Child after scroll");
}

#[test]
fn layer_rename_preserves_an_unrelated_edit_and_ignores_a_replacement_document() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    press_f2(&fixture);
    let field = rename_field(&fixture);
    field.set_text(Some("Approved name"));
    shell.editor.borrow_mut().update_node(fixture.id, "Move", |node| node.x += 5.0);
    fixture.key(&field, Key::Enter);
    assert_eq!(shell.editor.borrow().document.nodes[&fixture.id].name, "Approved name");
    assert_eq!(shell.editor.borrow().history_len(), 2);

    press_f2(&fixture);
    let field = rename_field(&fixture);
    field.set_text(Some("Stale draft"));
    let same_ids = shell.editor.borrow().document.clone();
    let mut replacement = Editor::new(same_ids);
    replacement.select(fixture.id, false);
    *shell.editor.borrow_mut() = replacement;
    fixture.key(&field, Key::Enter);
    assert_eq!(shell.editor.borrow().document.nodes[&fixture.id].name, "Approved name");
    assert_eq!(shell.editor.borrow().history_len(), 0);
}
