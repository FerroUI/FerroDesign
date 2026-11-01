use super::*;
use ferroui_base::input::PhysicalKey;
use ferroui_controls::{MenuItem, automation::AutomationProperties, primitives::ToggleButton};

fn row_at(fixture: &Fixture, index: usize) -> Ref<ToggleButton> {
    fixture.shell.control::<StackPanel>("PagesHost").children().get(index)
        .cast::<Grid>().unwrap().children().get(0).cast::<ToggleButton>().unwrap()
}

fn field(fixture: &Fixture) -> Ref<TextBox> {
    fixture.shell.root.get_visual_descendants().find_map(|v| v.cast::<TextBox>()
        .filter(|f| f.name().as_deref() == Some("PageRenameEditor"))).unwrap()
}

fn press(fixture: &Fixture, key: Key, physical: PhysicalKey) {
    fixture.window.key_press(key, RawInputModifiers::NONE, physical, None);
    fixture.window.key_release(key, RawInputModifiers::NONE, physical, None);
}

fn activate_menu(fixture: &Fixture, name: &str) {
    fixture.click_button(&fixture.shell.button("PageActionsButton"), RawInputModifiers::NONE);
    fixture.window.capture_rendered_frame().unwrap();
    let item = fixture.shell.control::<MenuItem>(name);
    assert!(item.is_effectively_visible() && item.is_enabled(), "{name} is available");
    let bounds = item.bounds();
    let point = item.point_to_screen(ferroui_base::Point::new(bounds.width / 2.0, bounds.height / 2.0));
    let window = TopLevel::get_top_level(Some(&item)).unwrap().cast::<Window>();
    // An overlay popup shares the physical headless window's native input route.
    let window = window.as_ref().unwrap_or(&fixture.window);
    let p = window.point_to_client(point);
    window.mouse_move(p, RawInputModifiers::NONE);
    window.mouse_down(p, MouseButton::Left, RawInputModifiers::NONE);
    window.mouse_up(p, MouseButton::Left, RawInputModifiers::NONE);
    Dispatcher::ui_thread().run_jobs(None);
}

#[test]
fn native_pages_rename_from_the_row_and_menu_keeps_selection_and_history() {
    let f = Fixture::new(false);
    let shell = &f.shell;
    let active = shell.editor.borrow().active_page;
    let original = f.json();
    let row = row_at(&f, 0);
    let name = shell.editor.borrow().document.page(active).unwrap().name.clone();
    assert_eq!(AutomationProperties::get_name(&row), Some(name.clone()));
    assert_eq!(row.is_checked(), Some(true));
    assert!(row.focus());
    press(&f, Key::F2, PhysicalKey::F2);
    let edit = field(&f);
    assert!(edit.is_focused());
    assert_eq!(edit.selection_end() - edit.selection_start(), name.chars().count() as i32);
    let updated = "Accounts – monthly balances with a deliberately long label";
    edit.set_text(Some(updated));
    press(&f, Key::Enter, PhysicalKey::Enter);
    assert_eq!(shell.editor.borrow().document.page(active).unwrap().name, updated);
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert_eq!(shell.editor.borrow().selection.as_slice(), [f.id]);
    assert!(shell.canvas.is_focused());
    assert_eq!(row_at(&f, 0), row, "a rename keeps the realized row");
    assert_eq!(AutomationProperties::get_name(&row).as_deref(), Some(updated));
    assert_eq!(shell.control::<TextBlock>("CompactPageTitle").text().as_deref(), Some(updated));
    assert!(shell.command("undo"));
    assert_eq!(f.json(), original);
    f.click_button(&row, RawInputModifiers::NONE);
    f.click_button(&row, RawInputModifiers::NONE);
    let edit = field(&f);
    edit.set_text(Some("discard"));
    press(&f, Key::Escape, PhysicalKey::Escape);
    assert_eq!(f.json(), original);
    assert!(shell.command("redo"));
    activate_menu(&f, "PageRename");
    assert!(field(&f).is_focused(), "closing the native flyout must leave focus in its editor");
    field(&f).set_text(Some("Keyboard and pointer"));
    shell.canvas.focus();
    assert_eq!(shell.editor.borrow().document.page(active).unwrap().name, "Keyboard and pointer");
}

#[test]
fn native_pages_reorder_preserves_page_identity_camera_and_exact_undo() {
    let f = Fixture::new(false);
    let shell = &f.shell;
    let active = shell.editor.borrow().active_page;
    let extra = shell.editor.borrow_mut().document.add_page("Prototype");
    shell.editor.borrow_mut().refresh();
    shell.refresh();
    f.window.capture_rendered_frame().unwrap();
    let original = f.json();
    let camera = shell.canvas.viewport.get();
    let first = row_at(&f, 0);
    let second = row_at(&f, 1);
    assert_eq!(second.is_checked(), Some(false));
    assert!(!shell.control::<MenuItem>("PageMoveUp").is_enabled());
    activate_menu(&f, "PageMoveDown");
    let ids = || shell.editor.borrow().document.pages.iter().map(|p| p.id).collect::<Vec<_>>();
    assert_eq!(ids(), [extra, active]);
    assert_eq!(row_at(&f, 1), first);
    assert_eq!(row_at(&f, 0), second);
    assert_eq!(shell.editor.borrow().active_page, active);
    assert_eq!(shell.editor.borrow().selection, [f.id]);
    assert_eq!(shell.canvas.viewport.get(), camera);
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert!(!shell.control::<MenuItem>("PageMoveDown").is_enabled());
    assert!(shell.canvas.is_focused(), "page menu activation restores the canvas shortcut scope");
    f.window.key_press(Key::Z, RawInputModifiers::CONTROL, PhysicalKey::Z, None);
    f.window.key_release(Key::Z, RawInputModifiers::CONTROL, PhysicalKey::Z, None);
    assert_eq!(f.json(), original);
    assert!(first.is_tab_stop() && !second.is_tab_stop());
    assert!(first.focus());
    press(&f, Key::End, PhysicalKey::End);
    assert_eq!(shell.editor.borrow().active_page, extra);
    assert!(second.is_focused() && second.is_tab_stop() && !first.is_tab_stop());
    press(&f, Key::Up, PhysicalKey::ArrowUp);
    assert_eq!(shell.editor.borrow().active_page, active);
    assert!(first.is_focused());
    assert_eq!(f.json(), original);
}

#[test]
fn native_pages_dismiss_blank_names_and_never_retarget_a_pending_rename() {
    let f = Fixture::new(false);
    let shell = &f.shell;
    let original = f.json();
    let row = row_at(&f, 0);
    assert!(row.focus());
    press(&f, Key::F2, PhysicalKey::F2);
    field(&f).set_text(Some("   "));
    shell.canvas.focus();
    assert_eq!(f.json(), original);
    assert_eq!(shell.editor.borrow().history_len(), 0);

    assert!(row.focus());
    press(&f, Key::F2, PhysicalKey::F2);
    let stale = field(&f);
    stale.set_text(Some("old draft"));
    let same_ids = shell.editor.borrow().document.clone();
    *shell.editor.borrow_mut() = Editor::new(same_ids);
    press(&f, Key::Enter, PhysicalKey::Enter);
    assert_eq!(f.json(), original);
    assert_eq!(shell.editor.borrow().history_len(), 0);
}

#[test]
fn native_page_cameras_restore_on_navigation_and_history_and_keep_world_center_after_resize() {
    let f = Fixture::new(false);
    let shell = &f.shell;
    let render = || { f.window.capture_rendered_frame().unwrap(); };
    render();
    let first = shell.editor.borrow().active_page;
    shell.canvas.set_zoom(0.375);
    let camera_one = shell.canvas.viewport.get();
    let original = f.json();
    let width = shell.field("WidthField");
    assert!(width.focus());
    width.set_text(Some("1/0"));
    assert!(!shell.command("add-page"));
    assert_eq!(shell.editor.borrow().active_page, first);
    assert_eq!(shell.canvas.viewport.get(), camera_one);
    assert!(width.focus());
    f.key(&width, Key::Escape);
    assert!(shell.command("add-page"));
    render();
    let second = shell.editor.borrow().active_page;
    assert_ne!(first, second);
    assert_eq!(shell.canvas.viewport.get().zoom, core::Viewport::default().zoom);
    let two_pages = f.json();
    shell.canvas.set_zoom(4.0);
    let camera_two = shell.canvas.viewport.get();

    assert!(row_at(&f, 1).focus());
    press(&f, Key::Home, PhysicalKey::Home);
    render();
    assert_eq!(shell.canvas.viewport.get(), camera_one);
    assert_eq!(shell.editor.borrow().history_len(), 1);
    f.window.set_width(1240.0);
    render();
    press(&f, Key::End, PhysicalKey::End);
    render();
    let restored = shell.canvas.viewport.get();
    let center = |v: core::Viewport| v.screen_to_world(Point::new(v.width / 2.0, v.height / 2.0));
    assert_ne!(restored.width, camera_two.width);
    assert_eq!(restored.zoom, camera_two.zoom);
    assert_eq!(center(restored), center(camera_two));
    assert_eq!(f.json(), two_pages);

    assert!(shell.command("undo"));
    render();
    assert_eq!(shell.editor.borrow().active_page, first);
    assert_eq!(shell.canvas.viewport.get().zoom, camera_one.zoom);
    assert_eq!(f.json(), original);
    assert!(shell.command("redo"));
    render();
    assert_eq!(shell.editor.borrow().active_page, second, "Redo reopens the added page");
    assert_eq!(shell.canvas.viewport.get(), restored, "Redo reopens that same page's camera");
    assert_eq!(f.json(), two_pages);
    // Undo may release the page ID for a different page on a new history branch.
    shell.canvas.set_zoom(3.5);
    assert!(shell.command("undo"));
    render();
    assert!(shell.command("add-page"));
    render();
    assert_eq!(shell.editor.borrow().active_page, second);
    assert_eq!(shell.canvas.viewport.get().zoom, core::Viewport::default().zoom);
}

#[test]
fn native_page_cameras_reset_on_same_id_document_import_and_history_crossings() {
    let f = Fixture::new(false);
    let shell = &f.shell;
    let render = || { f.window.capture_rendered_frame().unwrap(); };
    render();
    let authored_fit = shell.canvas.viewport.get();
    let mut imported = shell.editor.borrow().document.clone();
    imported.name = "Different file, same IDs".into();
    let shape = imported.nodes.get_mut(&f.id).unwrap();
    shape.x += 4000.0;
    shape.width *= 3.0;

    shell.canvas.set_zoom(0.25);
    let manual = shell.canvas.viewport.get();
    assert!(!shell.import_document("{", "broken"));
    render();
    assert_eq!(shell.canvas.viewport.get(), manual);
    assert!(shell.import_document(&imported.to_json().unwrap(), "imported"));
    render();
    let imported_fit = shell.canvas.viewport.get();
    assert_ne!(imported_fit, manual);
    assert_ne!(imported_fit, authored_fit);
    shell.canvas.set_zoom(2.5);
    assert!(shell.command("undo"));
    render();
    assert_eq!(shell.canvas.viewport.get(), authored_fit);
    assert!(shell.command("redo"));
    render();
    assert_eq!(shell.canvas.viewport.get(), imported_fit);
}
