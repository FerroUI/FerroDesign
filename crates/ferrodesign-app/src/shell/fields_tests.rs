//! Exercise the compiled native inspector's routed Enter/Escape and real
//! focus-loss handlers. Core setters alone cannot expose these two regressions.
use super::*;
use ferroui_base::input::{KeyEventArgs, MouseButton, RawInputModifiers};
use ferroui_base::threading::{Dispatcher, UnitTestDispatcherScope};
use ferroui_base::FerroLocator;
use ferroui_controls::{AppBuilder, Application, Window};
use ferroui_fonts_inter::AppBuilderExtension;
use ferroui_harfbuzz::HarfBuzzApplicationExtensions;
use ferroui_headless::{FerroHeadlessPlatformExtensions, FerroHeadlessPlatformOptions, HeadlessWindowExtensions};
use ferroui_themes_fluent::FluentTheme;
use ferroui_vello::{VelloApplicationExtensions, VelloOptions, VelloRenderingMode};

struct Fixture {
    shell: Rc<EditorShell>,
    window: Ref<Window>,
    id: NodeId,
    locator: Rc<dyn IDisposable>,
    _dispatcher: UnitTestDispatcherScope,
}

impl Fixture {
    fn new(bound: bool) -> Self {
        Self::with_kind(bound, NodeKind::Polygon)
    }

    fn with_kind(bound: bool, kind: NodeKind) -> Self {
        let dispatcher = Dispatcher::unit_test_scope();
        AppBuilder::reset_setup_for_unit_tests();
        let locator = FerroLocator::enter_scope();
        crate::register_types();
        let builder = AppBuilder::configure::<Application>()
            .with_inter_font()
            .use_harfbuzz()
            .with(Rc::new(VelloOptions::with_rendering_mode(
                VelloRenderingMode::Cpu,
            )))
            .use_vello()
            .use_headless(FerroHeadlessPlatformOptions {
                use_headless_drawing: false,
                ..Default::default()
            })
            .setup_without_starting();
        builder
            .instance()
            .unwrap()
            .styles()
            .add(FluentTheme::new().as_style());
        let mut document = core::Document::new("Native inspector field regression");
        let mut node = Node::new(kind, "Radius", 20.0, 20.0, 120.0, 120.0).with_radius(12.5);
        if bound {
            document.variables.insert(
                900,
                core::Variable {
                    id: 900,
                    name: "Radius".into(),
                    collection: "Shape".into(),
                    values: BTreeMap::from([("Default".into(), core::VariableValue::Number(12.5))]),
                },
            );
            document.next_id = 901;
            node.variable_bindings.insert("corner_radius".into(), 900);
        }
        let id = document.insert_node(1, node).unwrap();
        let mut editor = Editor::new(document);
        editor.selection = vec![id];
        let shell = EditorShell::new();
        *shell.editor.borrow_mut() = editor;
        shell.refresh();
        let window = Window::new();
        window.set_width(1500.0);
        window.set_height(960.0);
        window.set_content(Some(Control::boxed(shell.root.clone())));
        window.show();
        Dispatcher::ui_thread().run_jobs(None);
        let result = Self {
            shell,
            window,
            id,
            locator,
            _dispatcher: dispatcher,
        };
        result.stop_save_timer();
        assert!(result.shell.field("RadiusField").is_effectively_visible());
        assert!(result.shell.field("RadiusField").is_enabled());
        result
    }

    fn stop_save_timer(&self) {
        // This is an input/history test, not a persistence test. Synchronous
        // routed input lets us cancel the deferred autosave before pumping
        // any dispatcher jobs, so it never writes a user autosave file.
        if let Some(timer) = self.shell.pending_save_timer.borrow_mut().take() {
            timer.dispose();
        }
        self.shell.pending_save.set(false);
        platform::set_document_serialization_pending(false);
    }

    fn click_button(&self, button: &Button, modifiers: RawInputModifiers) {
        let bounds = button.bounds();
        let center = ferroui_base::Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let point = button.translate_point(center, &self.window).unwrap();
        self.window.mouse_move(point, RawInputModifiers::NONE);
        self.window.mouse_down(point, MouseButton::Left, modifiers);
        self.window.mouse_up(point, MouseButton::Left, modifiers);
    }

    fn key(&self, field: &Ref<TextBox>, key: Key) {
        assert!(
            field.is_focused(),
            "Route the key to the actual focused native field"
        );
        let mut event = KeyEventArgs::new();
        event.set_routed_event(Some(InputElement::key_down_event()));
        event.key = key;
        field.raise_event(&event);
        self.stop_save_timer();
        assert!(event.handled());
    }

    fn json(&self) -> String {
        self.shell.editor.borrow().document.to_json().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop_save_timer();
        self.window.close();
        self.stop_save_timer();
        self.locator.dispose();
        AppBuilder::reset_setup_for_unit_tests();
    }
}

#[test]
fn native_rejected_radius_enter_keeps_focus_and_escape_restores_without_canvas_selection_loss() {
    let fixture = Fixture::new(false);
    let field = fixture.shell.field("RadiusField");
    let before = fixture.json();
    for invalid in ["-1", "NaN", "10000001"] {
        assert!(field.focus());
        field.set_text(Some(invalid));
        fixture.key(&field, Key::Enter);
        assert!(
            field.is_focused(),
            "Rejected Enter must retain native field ownership"
        );
        assert_eq!(field.text().as_deref(), Some(invalid));
        assert_eq!(fixture.json(), before);
        assert_eq!(fixture.shell.editor.borrow().history_len(), 0);
        fixture.key(&field, Key::Escape);
        assert!(field.is_focused());
        assert_eq!(field.text().as_deref(), Some("12.50"));
        assert_eq!(fixture.shell.editor.borrow().selection, vec![fixture.id]);
        assert_eq!(fixture.json(), before);
        assert_eq!(fixture.shell.editor.borrow().history_len(), 0);
    }

    field.set_text(Some("18.5"));
    fixture.key(&field, Key::Enter);
    assert!(fixture.shell.canvas.is_focused());
    assert!(!field.is_focused());
    assert_eq!(
        fixture.shell.editor.borrow().document.nodes[&fixture.id].corner_radius,
        18.5
    );
    assert_eq!(
        fixture.shell.editor.borrow().history_len(),
        1,
        "The real LostFocus must not repeat Enter"
    );
    let after = fixture.json();
    assert!(fixture.shell.editor.borrow_mut().undo());
    assert_eq!(fixture.json(), before);
    assert!(fixture.shell.editor.borrow_mut().redo());
    assert_eq!(fixture.json(), after);
}

#[test]
fn native_smoothing_rejected_enter_then_escape_restores_last_committed_percent_and_selection() {
    // Match the browser's actual failure sequence on a supported box. Polygon
    // radius uses the same submission handler but does not expose smoothing.
    let fixture = Fixture::with_kind(false, NodeKind::Rectangle);
    let field = fixture.shell.field("CornerSmoothingField");
    assert!(field.is_effectively_visible());
    assert!(field.is_enabled());
    let before = fixture.json();
    assert!(field.focus());
    field.set_text(Some("60"));
    fixture.key(&field, Key::Enter);
    assert!(fixture.shell.canvas.is_focused());
    assert_eq!(
        fixture.shell.editor.borrow().document.nodes[&fixture.id].corner_smoothing,
        0.6
    );
    assert_eq!(fixture.shell.editor.borrow().history_len(), 1);
    let after = fixture.json();

    assert!(field.focus());
    field.set_text(Some("101"));
    fixture.key(&field, Key::Enter);
    assert!(field.is_focused());
    assert_eq!(field.text().as_deref(), Some("101"));
    assert_eq!(fixture.json(), after);
    assert_eq!(fixture.shell.editor.borrow().history_len(), 1);

    fixture.key(&field, Key::Escape);
    assert!(field.is_focused());
    assert_eq!(field.text().as_deref(), Some("60"));
    assert_eq!(fixture.shell.editor.borrow().selection, vec![fixture.id]);
    assert_eq!(fixture.json(), after);
    assert_eq!(fixture.shell.editor.borrow().history_len(), 1);
    assert!(fixture.shell.editor.borrow_mut().undo());
    assert_eq!(fixture.json(), before);
    assert!(fixture.shell.editor.borrow_mut().redo());
    assert_eq!(fixture.json(), after);
}

#[test]
fn native_explicit_equal_radius_enter_detaches_binding_once_while_blur_and_flush_remain_passive() {
    let fixture = Fixture::new(true);
    let field = fixture.shell.field("RadiusField");
    let before = fixture.json();
    assert!(field.focus());
    assert!(fixture.shell.canvas.focus());
    assert!(fixture.shell.flush_inspector_drafts());
    assert_eq!(
        fixture.json(),
        before,
        "Unchanged focus loss and save do not detach bindings"
    );
    assert_eq!(fixture.shell.editor.borrow().history_len(), 0);

    assert!(field.focus());
    // Same bytes as the displayed confirmed value: only explicit submission
    // can express the direct literal override here.
    assert_eq!(field.text().as_deref(), Some("12.50"));
    field.set_text(Some("12.50"));
    fixture.key(&field, Key::Enter);
    assert!(fixture.shell.canvas.is_focused());
    assert!(!fixture.shell.editor.borrow().document.nodes[&fixture.id]
        .variable_bindings
        .contains_key("corner_radius"));
    assert_eq!(
        fixture.shell.editor.borrow().document.nodes[&fixture.id].corner_radius,
        12.5
    );
    assert_eq!(fixture.shell.editor.borrow().history_len(), 1);
    let after = fixture.json();
    assert!(fixture.shell.editor.borrow_mut().undo());
    assert_eq!(fixture.json(), before);
    assert!(fixture.shell.editor.borrow_mut().redo());
    assert_eq!(fixture.json(), after);

    assert!(field.focus());
    fixture.key(&field, Key::Enter);
    assert!(fixture.shell.canvas.is_focused());
    assert_eq!(fixture.json(), after);
    assert_eq!(
        fixture.shell.editor.borrow().history_len(),
        1,
        "An unbound equal literal remains a no-op"
    );
}

#[test]
fn native_layer_rows_route_click_range_and_toggle_modifiers() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let (back, middle, front) = {
        let mut editor = shell.editor.borrow_mut();
        let page = editor.active_page;
        let mut add = |name| {
            editor
                .document
                .insert_node(
                    page,
                    Node::new(NodeKind::Rectangle, name, 0.0, 0.0, 16.0, 16.0),
                )
                .unwrap()
        };
        let middle = add("Middle");
        let front = add("Front");
        editor.refresh();
        (fixture.id, middle, front)
    };
    shell.refresh();
    Dispatcher::ui_thread().run_jobs(None);
    let before = fixture.json();
    let click = |id, modifiers| {
        let button = shell.layer_buttons.borrow()[&id]
            .upgrade()
            .expect("realized layer button");
        fixture.click_button(&button, modifiers);
    };

    click(front, RawInputModifiers::NONE);
    assert_eq!(shell.editor.borrow().selection, [front]);
    click(back, RawInputModifiers::SHIFT);
    assert_eq!(shell.editor.borrow().selection, [front, middle, back]);
    click(middle, RawInputModifiers::CONTROL);
    assert_eq!(shell.editor.borrow().selection, [front, back]);
    click(middle, RawInputModifiers::META);
    assert_eq!(shell.editor.borrow().selection, [front, back, middle]);
    click(back, RawInputModifiers::NONE);
    assert_eq!(shell.editor.borrow().selection, [back]);
    assert_eq!(fixture.json(), before);
    assert_eq!(shell.editor.borrow().history_len(), 0);
}

#[test]
fn native_compact_workspace_overlays_drawers_without_moving_the_artboard() {
    let fixture = Fixture::new(false);
    let shell = &fixture.shell;
    let canvas = shell.control::<Grid>("CanvasHost");
    let present = |name| shell.control::<Control>(name).is_effectively_visible();
    let render = || {
        fixture.window.capture_rendered_frame().expect("rendered native workspace");
    };
    render();
    let desktop = canvas.bounds();
    let before = fixture.json();
    let history = shell.editor.borrow().history_len();
    let selection = shell.editor.borrow().selection.clone();

    fixture.window.set_width(390.0);
    fixture.window.set_height(844.0);
    render();
    assert_eq!(shell.root.bounds().width, 390.0);
    assert!(present("CompactHeader") && present("Toolbar"));
    assert!(!present("LeftPanel") && !present("RightPanel"));
    let compact = canvas.bounds();
    assert_eq!((compact.x, compact.width), (0.0, 390.0));
    let camera = shell.canvas.viewport.get();

    for (button, layers, properties) in [
        ("CompactLayers", true, false),
        ("CompactLayers", false, false),
        ("CompactProperties", false, true),
        ("CompactLayers", true, false),
        ("CompactProperties", false, true),
        ("CompactProperties", false, false),
    ] {
        fixture.click_button(&shell.button(button), RawInputModifiers::NONE);
        render();
        assert_eq!(present("LeftPanel"), layers);
        assert_eq!(present("NavigationRail"), layers);
        assert_eq!(present("RightPanel"), properties);
        assert_eq!(present("Toolbar"), !layers && !properties);
        assert_eq!(canvas.bounds(), compact);
        assert_eq!(shell.canvas.viewport.get(), camera);
        assert_eq!(shell.editor.borrow().selection, selection);
        assert_eq!(shell.editor.borrow().history_len(), history);
    }
    fixture.window.set_width(1500.0);
    fixture.window.set_height(960.0);
    render();
    assert!(!present("CompactHeader"));
    assert!(present("LeftPanel") && present("RightPanel") && present("Toolbar"));
    assert_eq!(canvas.bounds(), desktop);
    assert_eq!(shell.editor.borrow().selection, selection);
    assert_eq!(fixture.json(), before);
}
