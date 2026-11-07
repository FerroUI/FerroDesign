use super::*;
use ferroui_controls::{ScrollViewer, automation::AutomationProperties};

fn open(fixture: &Fixture) -> Ref<TextBox> {
    fixture.shell.command("actions");
    fixture.shell.control::<Border>("DialogSearchHost").child().unwrap().cast().unwrap()
}

fn displayed(shell: &EditorShell) -> Vec<Ref<Button>> {
    let content = shell.control::<StackPanel>("ModalContent");
    let list = content.children().get(0).cast::<StackPanel>().unwrap();
    list.children().to_vec().into_iter().filter_map(|control| control.cast::<Button>())
        .filter(|button| button.is_visible()).collect()
}

#[test]
fn quick_actions_rank_shortcuts_in_reading_order_and_run_the_current_query() {
    let fixture = Fixture::new(false);
    let search = open(&fixture);
    let original = fixture.json();
    let rows = displayed(&fixture.shell);
    let ids: BTreeSet<_> = rows.iter().map(|button| button.name().unwrap()).collect();
    assert_eq!(ids.len(), rows.len(), "each shared menu command has one result");
    for id in ["Action-export-svg", "Action-distribute-horizontal", "Action-rectangle"] {
        assert!(ids.contains(id));
    }

    search.set_text(Some("R"));
    Dispatcher::ui_thread().run_jobs(None);
    let rows = displayed(&fixture.shell);
    assert_eq!(rows[0].name().as_deref(), Some("Action-rectangle"));
    assert!(rows[0].classes().contains("active"));
    assert_eq!(AutomationProperties::get_accelerator_key(&rows[0]).as_deref(), Some("R"));
    assert!(AutomationProperties::get_item_status(&search).unwrap().starts_with("Rectangle, 1 of "));
    fixture.key(&search, Key::Down);
    assert!(rows[1].classes().contains("active"));
    fixture.key(&search, Key::Up);
    assert!(rows[0].classes().contains("active"));

    // Normal input updates Text before the queued TextChanged notification runs.
    search.set_text(Some("no matches exist here"));
    fixture.key(&search, Key::Enter);
    assert_eq!(AutomationProperties::get_item_status(&search).as_deref(), Some("No matching actions"));
    assert!(fixture.shell.control::<Border>("ModalBackdrop").is_visible());
    search.set_text(Some("shift+l"));
    fixture.key(&search, Key::Enter);
    assert_eq!(fixture.shell.canvas.tool.get(), Tool::Arrow);
    assert!(fixture.shell.canvas.is_focused());
    assert_eq!(fixture.json(), original);
}

#[test]
fn quick_actions_keep_search_stationary_while_filtering_on_a_short_viewport() {
    let fixture = Fixture::new(false);
    fixture.window.set_width(390.0);
    fixture.window.set_height(370.0);
    let search = open(&fixture);
    let viewport = fixture.shell.control::<ScrollViewer>("ModalViewport");
    fixture.window.capture_rendered_frame().unwrap();
    let origin = || search.translate_point(ferroui_base::Point::new(0.0, 0.0), &fixture.window).unwrap();
    let initial = origin();
    let full_height = viewport.bounds().height;
    assert!(full_height > 0.0);
    assert!(initial.y > 0.0 && initial.y < 370.0);
    let list = fixture.shell.control::<StackPanel>("ModalContent").children().get(0).cast::<StackPanel>().unwrap();
    let original_order = list.children().to_vec();
    search.set_text(Some("Export SVG"));
    Dispatcher::ui_thread().run_jobs(None);
    fixture.window.capture_rendered_frame().unwrap();
    assert_eq!(displayed(&fixture.shell).len(), 1);
    assert_eq!(list.children().to_vec(), original_order, "narrowing to one match need not move any native rows");
    assert_eq!(origin(), initial);
    assert!(viewport.bounds().height < full_height, "the palette shrinks to its displayed results");
    search.set_text(Some("zzzz"));
    Dispatcher::ui_thread().run_jobs(None);
    fixture.window.capture_rendered_frame().unwrap();
    assert_eq!(origin(), initial);
    fixture.key(&search, Key::Escape);
    assert!(!fixture.shell.control::<Border>("ModalBackdrop").is_visible());
}

#[test]
fn quick_actions_cycle_native_tab_focus_and_dismiss_from_close() {
    use ferroui_base::input::PhysicalKey;

    let fixture = Fixture::new(false);
    let search = open(&fixture);
    fixture.window.capture_rendered_frame().unwrap();
    let close = fixture.shell.button("CloseModalButton");
    let press = |key, physical, modifiers| {
        fixture.window.key_press(key, modifiers, physical, None);
        fixture.window.key_release(key, modifiers, physical, None);
    };
    press(Key::Tab, PhysicalKey::Tab, RawInputModifiers::NONE);
    assert!(close.is_focused());
    press(Key::Tab, PhysicalKey::Tab, RawInputModifiers::NONE);
    assert!(search.is_focused());
    press(Key::Tab, PhysicalKey::Tab, RawInputModifiers::SHIFT);
    assert!(close.is_focused());
    press(Key::Escape, PhysicalKey::Escape, RawInputModifiers::NONE);
    assert!(!fixture.shell.control::<Border>("ModalBackdrop").is_visible());
    assert!(fixture.shell.canvas.is_focused());
}
