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

#[test]
fn native_page_copy_and_delete_restore_active_artwork_and_camera_in_one_command() {
    let f = Fixture::new(false);
    let shell = &f.shell;
    let draw = || { f.window.capture_rendered_frame().unwrap(); };
    draw();
    assert!(!shell.control::<MenuItem>("PageDelete").is_enabled());
    assert!(!shell.command("delete-page"));
    let source = shell.editor.borrow().active_page;
    let original = f.json();
    shell.canvas.set_zoom(0.625);
    let source_camera = shell.canvas.viewport.get();
    let width = shell.field("WidthField");
    assert!(width.focus());
    width.set_text(Some("1/0"));
    assert!(!shell.command("duplicate-page"));
    assert_eq!(f.json(), original);
    assert!(width.focus());
    f.key(&width, Key::Escape);
    activate_menu(&f, "PageDuplicate");
    draw();
    let copy = shell.editor.borrow().active_page;
    assert_ne!(copy, source);
    assert_eq!(shell.canvas.viewport.get(), source_camera);
    assert_eq!(row_at(&f, 1).is_checked(), Some(true));
    assert_eq!(shell.editor.borrow().history_len(), 1);
    assert!(shell.control::<MenuItem>("PageDelete").is_enabled());
    let copied = f.json();

    shell.canvas.set_zoom(2.5);
    let copy_camera = shell.canvas.viewport.get();
    activate_menu(&f, "PageDelete");
    draw();
    assert_eq!(shell.editor.borrow().active_page, source);
    assert_eq!(shell.canvas.viewport.get(), source_camera);
    assert!(shell.canvas.is_focused());
    assert_eq!(shell.editor.borrow().history_len(), 2);
    assert!(!shell.control::<MenuItem>("PageDelete").is_enabled());
    assert!(shell.command("undo"));
    draw();
    assert_eq!(shell.editor.borrow().active_page, copy);
    assert_eq!(shell.canvas.viewport.get(), copy_camera);
    assert_eq!(f.json(), copied);
    assert!(shell.command("undo"));
    draw();
    assert_eq!(f.json(), original);
    assert_eq!(shell.editor.borrow().selection, [f.id]);
    assert_eq!(shell.canvas.viewport.get(), source_camera);
    assert!(shell.command("redo"));
    draw();
    assert_eq!(shell.canvas.viewport.get(), copy_camera);
    assert_eq!(f.json(), copied);
}

#[test]
fn native_page_drag_reorders_on_release_keeps_canvas_state_and_cancels_at_edges() {
    let f = Fixture::new(false);
    let s = &f.shell;
    let active = s.editor.borrow().active_page;
    for i in 0..22 {
        let name = if i == 2 { "A page name long enough to be visibly ellipsized in the page list".to_owned() }
            else { format!("Scene {i}") };
        s.editor.borrow_mut().document.add_page(name);
    }
    s.editor.borrow_mut().refresh();
    s.refresh();
    f.window.capture_rendered_frame().unwrap();
    let view = s.control::<ferroui_controls::ScrollViewer>("PagesViewport");
    let camera = s.canvas.viewport.get();
    let before = f.json();
    let ids = || s.editor.borrow().document.pages.iter().map(|p| p.id).collect::<Vec<_>>();
    let order = ids();
    let point = |index, fraction| {
        let row = row_at(&f, index);
        row.translate_point(ferroui_base::Point::new(20., row.bounds().height * fraction), &f.window).unwrap()
    };
    let start = point(1, 0.5);
    let third = point(2, 0.8);
    let fourth = point(3, 0.8);
    let down = |p| {
        f.window.mouse_move(p, RawInputModifiers::NONE);
        f.window.mouse_down(p, MouseButton::Left, RawInputModifiers::NONE);
    };
    let move_to = |p| f.window.mouse_move(p, RawInputModifiers::LEFT_MOUSE_BUTTON);
    let up = |p| f.window.mouse_up(p, MouseButton::Left, RawInputModifiers::NONE);
    let insertion_ink = || {
        let frame = f.window.capture_rendered_frame().unwrap();
        let edge = point(3, 0.);
        let mut rgba = [0; 32 * 4 * 4];
        frame.copy_pixels(ferroui_base::PixelRect::new(edge.x as i32, edge.y as i32 - 1, 32, 4), &mut rgba, 32 * 4);
        let bgra = frame.format() == Some(ferroui_base::platform::PixelFormat::BGRA8888);
        rgba.chunks_exact(4).filter(|p| {
            let (r, b) = if bgra { (p[2], p[0]) } else { (p[0], p[2]) };
            r < 90 && p[1] > 90 && b > p[1].saturating_add(40)
        }).count()
    };
    let short = row_at(&f, 1);
    let long = row_at(&f, 3);
    ferroui_controls::ToolTip::set_is_open(&short, true);
    assert!(!ferroui_controls::ToolTip::get_is_open(&short), "a readable title has no popup over adjacent rows");
    ferroui_controls::ToolTip::set_is_open(&long, true);
    assert!(ferroui_controls::ToolTip::get_is_open(&long), "the full clipped name remains available on hover");
    ferroui_controls::ToolTip::set_is_open(&long, false);
    assert_eq!(insertion_ink(), 0);
    down(start); move_to(third);
    ferroui_controls::ToolTip::set_is_open(&long, true);
    assert!(!ferroui_controls::ToolTip::get_is_open(&long));
    assert!(!ferroui_controls::ToolTip::get_service_enabled(&long));
    assert!(s.has_page_drag());
    assert!(insertion_ink() >= 32, "the native insertion line spans the top edge of the target row");
    assert_eq!(f.json(), before);
    up(fourth);
    assert!(ferroui_controls::ToolTip::get_service_enabled(&long));
    let mut expected = order.clone();
    let moved = expected.remove(1);
    expected.insert(3, moved);
    assert_eq!(ids(), expected);
    assert_eq!(s.editor.borrow().active_page, active);
    assert_eq!(s.editor.borrow().selection, [f.id]);
    assert_eq!(s.canvas.viewport.get(), camera);
    assert_eq!(s.editor.borrow().history_len(), 1);
    assert!(s.command("undo"));
    f.window.capture_rendered_frame().unwrap();
    assert_eq!(f.json(), before);

    let edge = view.translate_point(ferroui_base::Point::new(20., view.bounds().height - 1.), &f.window).unwrap();
    let outside = view.translate_point(ferroui_base::Point::new(-9., view.bounds().height - 1.), &f.window).unwrap();
    down(start); move_to(edge);
    assert!(s.scroll_page_drag() && view.offset().y > 0.);
    assert_eq!(f.json(), before);
    press(&f, Key::Escape, PhysicalKey::Escape);
    assert!(!s.has_page_drag());
    up(edge);
    assert_eq!(f.json(), before);
    view.set_offset(ferroui_base::Vector::default());
    f.window.capture_rendered_frame().unwrap();
    down(start); move_to(third); up(outside);
    assert_eq!(f.json(), before);
    assert_eq!(s.editor.borrow().history_len(), 0);
    assert!(s.editor.borrow().can_redo());
    down(start); move_to(third);
    assert!(s.command("redo"));
    up(third);
    assert_eq!(f.json(), before);
    assert!(s.editor.borrow().can_redo());
    down(start); move_to(edge);
    for _ in 0..8 { assert!(s.scroll_page_drag()); }
    up(edge);
    assert!(ids().iter().position(|id| *id == order[1]).unwrap() > 4);
    assert_eq!(s.editor.borrow().active_page, active);
    assert_eq!(s.editor.borrow().history_len(), 1);
}

#[test]
fn native_touch_page_hold_distinguishes_menu_reorder_and_scrolling() {
    let f = Fixture::new(false);
    let s = &f.shell;
    let active = s.editor.borrow().active_page;
    for i in 0..16 { s.editor.borrow_mut().document.add_page(format!("Touch scene {i}")); }
    s.editor.borrow_mut().refresh();
    s.refresh();
    f.window.set_width(390.);
    f.window.set_height(844.);
    f.window.capture_rendered_frame().unwrap();
    s.show_layers_panel();
    f.window.capture_rendered_frame().unwrap();
    for name in ["PageActionsButton", "AddPageButton"] {
        let b = s.button(name).bounds();
        assert!(b.width >= 44. && b.height >= 44., "{name} touch target {b:?}");
    }
    assert!(row_at(&f, 0).bounds().height >= 44.);
    let view = s.control::<ferroui_controls::ScrollViewer>("PagesViewport");
    let menu = s.button("PageActionsButton").flyout().unwrap();
    let point = |index, fraction| {
        let row = row_at(&f, index);
        row.translate_point(ferroui_base::Point::new(25., row.bounds().height * fraction), &f.window).unwrap()
    };
    let hold = |row| {
        let touch = f.window.touch_begin(point(row, 0.5), RawInputModifiers::NONE);
        let interval = row_at(&f, row).get_platform_settings().unwrap().hold_wait_duration();
        std::thread::sleep(interval.max(Duration::from_millis(350)) + Duration::from_millis(30));
        Dispatcher::ui_thread().run_jobs(None);
        touch
    };
    let before = f.json();
    let source = s.editor.borrow().document.pages[2].id;
    let canceled = f.window.touch_begin(point(0, 0.5), RawInputModifiers::NONE);
    canceled.dispose();
    assert!(s.page_drag.borrow().is_none());
    let a = hold(2);
    assert!(!menu.is_open());
    assert_eq!(s.editor.borrow().active_page, active);
    let target = point(0, 0.2);
    f.window.touch_move(a.as_ref(), target, RawInputModifiers::NONE);
    assert!(s.has_page_drag());
    assert_eq!(f.json(), before);
    f.window.touch_end(a.as_ref(), target, RawInputModifiers::NONE);
    assert_eq!(s.editor.borrow().document.pages[0].id, source);
    assert!(!menu.is_open());
    assert_eq!(s.editor.borrow().active_page, active);
    assert_eq!(s.editor.borrow().selection, [f.id]);
    assert!(s.command("undo"));
    f.window.capture_rendered_frame().unwrap();
    assert_eq!(f.json(), before);

    let stationary = s.editor.borrow().document.pages[1].id;
    let b = hold(1);
    assert!(!menu.is_open());
    f.window.touch_end(b.as_ref(), point(1, 0.5), RawInputModifiers::NONE);
    Dispatcher::ui_thread().run_jobs(None);
    assert!(menu.is_open());
    f.window.capture_rendered_frame().unwrap();
    for name in ["PageRename", "PageDuplicate", "PageMoveUp", "PageMoveDown", "PageDelete"] {
        let b = s.control::<MenuItem>(name).bounds();
        assert!(b.width >= 44. && b.height >= 44., "{name} touch target {b:?}");
    }
    assert_eq!(s.editor.borrow().active_page, stationary);
    assert_eq!(s.editor.borrow().history_len(), 0);
    menu.hide();
    f.window.capture_rendered_frame().unwrap();

    let from = point(2, 0.7);
    let c = f.window.touch_begin(from, RawInputModifiers::NONE);
    for dy in [12., 30., 52., 70.] {
        f.window.touch_move(c.as_ref(), ferroui_base::Point::new(from.x, from.y-dy), RawInputModifiers::NONE);
    }
    f.window.touch_end(c.as_ref(), ferroui_base::Point::new(from.x, from.y-70.), RawInputModifiers::NONE);
    f.window.capture_rendered_frame().unwrap();
    assert!(view.offset().y > 0.0, "a quick swipe scrolls the page list");
    assert!(!s.has_page_drag() && !menu.is_open());
    assert_eq!(f.json(), before);
    assert_eq!(s.editor.borrow().active_page, stationary);
}
