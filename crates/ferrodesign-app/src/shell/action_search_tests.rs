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
    assert!(viewport.bounds().height > 0.0);
    assert!(initial.y > 0.0 && initial.y < 370.0);
    search.set_text(Some("Export SVG"));
    Dispatcher::ui_thread().run_jobs(None);
    fixture.window.capture_rendered_frame().unwrap();
    assert_eq!(displayed(&fixture.shell).len(), 1);
    assert_eq!(origin(), initial);
    search.set_text(Some("zzzz"));
    Dispatcher::ui_thread().run_jobs(None);
    fixture.window.capture_rendered_frame().unwrap();
    assert_eq!(origin(), initial);
    fixture.key(&search, Key::Escape);
    assert!(!viewport.classes().contains("action-palette"));
}
