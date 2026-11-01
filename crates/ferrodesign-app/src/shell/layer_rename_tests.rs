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

struct BatchFixture {
    ui: Fixture,
    bottom: NodeId,
    frame: NodeId,
    child: NodeId,
    locked_child: NodeId,
}

impl BatchFixture {
    fn new() -> Self {
        let ui = Fixture::new(false);
        let bottom = ui.id;
        let (frame, child, locked_child) = {
            let mut e = ui.shell.editor.borrow_mut();
            let page = e.active_page;
            e.document.nodes.get_mut(&bottom).unwrap().name = "Å12–底".into();
            let node = |kind, name| Node::new(kind, name, 0.0, 0.0, 100.0, 80.0);
            let frame = e.document.insert_node(page, node(NodeKind::Frame, "Frame–Ω9✨")).unwrap();
            let child = e.document.insert_node(frame, node(NodeKind::Rectangle, "Button–λ2")).unwrap();
            let mut locked = node(NodeKind::Frame, "Locked");
            locked.locked = true;
            let locked = e.document.insert_node(page, locked).unwrap();
            let locked_child = e.document.insert_node(locked, node(NodeKind::Rectangle, "Child–Z8")).unwrap();
            // Selection order deliberately differs from tree order. The parent lock
            // must protect the child, even when the parent itself is not selected.
            e.selection = vec![child, locked_child, bottom, frame];
            e.refresh();
            (frame, child, locked_child)
        };
        ui.shell.refresh();
        Self { ui, bottom, frame, child, locked_child }
    }

    fn control<T: ObjectType>(&self, name: &str) -> Ref<T> {
        self.ui.shell.root.get_visual_descendants().find_map(|v| {
            v.cast::<Control>().filter(|c| c.name().as_deref() == Some(name))?.cast::<T>()
        }).unwrap_or_else(|| panic!("Missing native rename control {name}"))
    }

    fn open(&self) -> Ref<TextBox> {
        use ferroui_base::input::PhysicalKey;
        assert!(self.ui.shell.canvas.focus());
        self.ui.window.key_press(Key::F2, RawInputModifiers::NONE, PhysicalKey::F2, None);
        self.ui.window.key_release(Key::F2, RawInputModifiers::NONE, PhysicalKey::F2, None);
        let replace = self.control::<TextBox>("RenameReplacement");
        assert!(replace.is_focused());
        assert_eq!(replace.selected_text(), "$&");
        replace
    }
}

#[test]
fn bulk_rename_native_f2_previews_unicode_captures_and_numbers_layers_in_tree_order() {
    let f = BatchFixture::new();
    let shell = &f.ui.shell;
    let before = f.ui.json();
    let selection = shell.editor.borrow().selection.clone();
    let replace = f.open();
    f.control::<TextBox>("RenameMatch").set_text(Some(r"(\p{L})([0-9]+)"));
    f.control::<TextBox>("RenameStart").set_text(Some("9"));
    replace.set_text(Some("$2-$1-$$-$nn-$NN-$&"));
    Dispatcher::ui_thread().run_jobs(None);
    let preview = f.control::<TextBlock>("RenamePreview").text().unwrap();
    assert_eq!(preview, "3 of 3 names will change\nFrame–Ω9✨  →  Frame–9-Ω-$-09-11-Ω9✨\nButton–λ2  →  Button–2-λ-$-10-10-λ2\nÅ12–底  →  12-Å-$-11-09-Å12–底");
    assert_eq!(f.ui.json(), before);
    assert!(f.control::<Button>("RenameApply").is_enabled());
    // Replace the plan and immediately press Enter, before deferred TextChanged.
    replace.set_text(Some("$2.$1.$nn/$NN"));
    f.ui.key(&replace, Key::Enter);
    let after = f.ui.json();
    {
        let e = shell.editor.borrow();
        assert_eq!(e.document.nodes[&f.frame].name, "Frame–9.Ω.09/11✨");
        assert_eq!(e.document.nodes[&f.child].name, "Button–2.λ.10/10");
        assert_eq!(e.document.nodes[&f.bottom].name, "12.Å.11/09–底");
        assert_eq!(e.document.nodes[&f.locked_child].name, "Child–Z8");
        assert_eq!(e.selection, selection);
        assert_eq!(e.history_len(), 1);
    }
    assert!(shell.canvas.is_focused());
    assert!(shell.command("undo"));
    assert_eq!(f.ui.json(), before);
    assert!(shell.command("redo"));
    assert_eq!(f.ui.json(), after);
}

#[test]
fn bulk_rename_native_no_op_invalid_and_escape_preserve_the_document_and_redo() {
    let f = BatchFixture::new();
    let shell = &f.ui.shell;
    let before = f.ui.json();
    let replace = f.open();
    replace.set_text(Some("Temp $n"));
    f.ui.key(&replace, Key::Enter);
    let after = f.ui.json();
    assert!(shell.command("undo"));
    let replace = f.open();
    let preview = f.control::<TextBlock>("RenamePreview");
    let apply = f.control::<Button>("RenameApply");
    let find = f.control::<TextBox>("RenameMatch");
    let start = f.control::<TextBox>("RenameStart");
    for (pattern, template, base, message) in [
        ("", "$&", "1", "No names would change."),
        ("^absent$", "Title", "1", "No names would change."),
        ("[", "Title", "1", "Invalid match."),
        ("", "Title", "-1", "nonnegative starting number"),
        ("", "  ", "1", "empty name"),
    ] {
        find.set_text(Some(pattern));
        replace.set_text(Some(template));
        start.set_text(Some(base));
        f.ui.key(&replace, Key::Enter);
        Dispatcher::ui_thread().run_jobs(None);
        assert!(!apply.is_enabled());
        assert!(preview.text().unwrap().contains(message));
        assert!(shell.control::<Border>("ModalBackdrop").is_visible());
        assert_eq!(shell.editor.borrow().history_len(), 0);
        assert_eq!(f.ui.json(), before);
    }
    replace.set_text(Some("Do not apply $n"));
    f.ui.key(&replace, Key::Escape);
    assert!(shell.canvas.is_focused());
    assert_eq!(f.ui.json(), before);
    assert!(shell.command("redo"));
    assert_eq!(f.ui.json(), after);
}

#[test]
fn bulk_rename_stale_targets_never_replace_new_names_selection_or_documents() {
    let f = BatchFixture::new();
    let shell = &f.ui.shell;
    let replace = f.open();
    replace.set_text(Some("Stale $n"));
    shell.editor.borrow_mut().update_node(f.frame, "External rename", |n| n.name = "New name".into());
    let external = f.ui.json();
    f.ui.key(&replace, Key::Enter);
    assert_eq!(f.ui.json(), external);
    assert_eq!(shell.editor.borrow().history_len(), 1);

    let replace = f.open();
    replace.set_text(Some("Wrong targets $n"));
    shell.editor.borrow_mut().selection = vec![f.child, f.bottom];
    f.ui.key(&replace, Key::Enter);
    assert_eq!(f.ui.json(), external);
    assert_eq!(shell.editor.borrow().history_len(), 1);

    let replace = f.open();
    replace.set_text(Some("Wrong file $n"));
    let doc = shell.editor.borrow().document.clone();
    let mut replacement = Editor::new(doc);
    replacement.selection = vec![f.child, f.bottom];
    *shell.editor.borrow_mut() = replacement;
    f.ui.key(&replace, Key::Enter);
    assert_eq!(f.ui.json(), external);
    assert_eq!(shell.editor.borrow().history_len(), 0);
}
