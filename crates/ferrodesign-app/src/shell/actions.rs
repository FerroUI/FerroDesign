//! Keyboard and pointer activation share the editor command dispatcher.
use super::*;
use ferroui_base::interactivity::RoutingStrategies;
use ferroui_controls::automation::AutomationProperties;

struct ActionRow {
    label: String,
    searchable: String,
    command: &'static str,
    button: Ref<Button>,
}

struct ActionResults {
    rows: Vec<ActionRow>,
    active: Cell<Option<usize>>,
    empty: Ref<TextBlock>,
}

impl ActionResults {
    fn filter(&self, text: &str) {
        let query = text.trim().to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        let mut first = None;
        let mut exact = None;
        for (index, row) in self.rows.iter().enumerate() {
            let matched = words.iter().all(|word| row.searchable.contains(word));
            if row.button.is_visible() != matched {
                row.button.set_is_visible(matched);
            }
            if matched {
                first.get_or_insert(index);
                if row.label == query { exact = Some(index); }
            }
        }
        self.empty.set_is_visible(first.is_none());
        self.highlight(exact.or(first));
    }

    fn highlight(&self, active: Option<usize>) {
        let old = self.active.replace(active);
        if old != active {
            if let Some(index) = old {
                self.rows[index].button.classes().remove("active");
            }
            if let Some(index) = active {
                self.rows[index].button.classes().add("active");
            }
        }
    }

    fn step(&self, down: bool) {
        let Some(current) = self.active.get() else { return; };
        let next = if down {
            (current + 1..self.rows.len()).find(|&i| self.rows[i].button.is_visible())
        } else {
            (0..current).rev().find(|&i| self.rows[i].button.is_visible())
        };
        if let Some(index) = next {
            self.highlight(Some(index));
            self.rows[index].button.bring_into_view();
        }
    }
}

impl EditorShell {
    pub(super) fn reset_action_search(&self) {
        let host = self.control::<Border>("DialogSearchHost");
        if host.is_visible() {
            host.set_is_visible(false);
            host.set_child(None);
        }
    }

    pub(super) fn action_dialog(
        self: &Rc<Self>,
        title: &str,
        description: &str,
        actions: &[(&str, &'static str)],
    ) {
        if !self.flush_inspector_drafts() { return; }
        self.modal(title, description);
        let host = self.control::<StackPanel>("ModalContent");
        let search = TextBox::new();
        search.set_name(Some("ActionSearch".into()));
        search.set_placeholder_text(Some("Search actions…"));
        let magnifier = icons::icon("search", "#666666", 14.0);
        magnifier.set_margin(Thickness::new(8.0, 0.0, 2.0, 0.0));
        search.set_inner_left_content(Some(Control::boxed(magnifier)));
        AutomationProperties::set_name(&search, Some("Search actions"));
        let header = self.control::<Border>("DialogSearchHost");
        header.set_child(Some(search.clone().upcast::<Control>()));
        header.set_is_visible(true);

        let list = StackPanel::new();
        list.set_spacing(2.0);
        host.children().add(list.clone());
        let mut rows = Vec::with_capacity(actions.len());
        for &(caption, command) in actions {
            let (label, shortcut) = caption.split_once("  ").unwrap_or((caption, ""));
            let button = Button::new();
            button.set_name(Some(format!("Action-{command}")));
            button.classes().add("layer");
            AutomationProperties::set_name(&button, Some(caption));
            let content = Grid::new();
            content.set_column_definitions(ColumnDefinitions::parse("*,Auto").unwrap());
            let name = TextBlock::new();
            name.set_text(Some(label));
            name.set_vertical_alignment(VerticalAlignment::Center);
            content.children().add(name);
            if !shortcut.is_empty() {
                let hint = TextBlock::new();
                hint.set_text(Some(shortcut));
                hint.classes().add("label");
                hint.set_vertical_alignment(VerticalAlignment::Center);
                Grid::set_column(&hint, 1);
                content.children().add(hint);
            }
            button.set_content(Some(Control::boxed(content)));
            let weak = Rc::downgrade(self);
            button.click(move |_, _| {
                if let Some(shell) = weak.upgrade() {
                    shell.close_modal();
                    shell.command(command);
                }
            });
            list.children().add(button.clone());
            rows.push(ActionRow {
                label: label.to_lowercase(),
                searchable: format!("{} {command}", caption.to_lowercase()).replace('-', " "),
                command,
                button,
            });
        }
        let empty = TextBlock::new();
        empty.set_text(Some("No matching actions"));
        empty.classes().add("label");
        host.children().add(empty.clone());
        let results = Rc::new(ActionResults { rows, active: Cell::new(None), empty });
        results.filter("");
        let search_ref = search.downgrade();
        let filtered = results.clone();
        search.text_changed(move |_, _| {
            if let Some(field) = search_ref.upgrade() {
                filtered.filter(&field.text().unwrap_or_default());
            }
        });
        let weak = Rc::downgrade(self);
        let field = search.downgrade();
        search.add_handler_with(InputElement::key_down_event(), move |_, event| {
            let Some(shell) = weak.upgrade() else { return; };
            if field.upgrade().is_some_and(|field| field.is_composing_text())
                || !event.key_modifiers.is_empty() { return; }
            match event.key {
                Key::Down | Key::Up => results.step(event.key == Key::Down),
                Key::Escape => shell.close_modal(),
                Key::Enter => {
                    if let Some(index) = results.active.get() {
                        shell.close_modal();
                        shell.command(results.rows[index].command);
                    }
                }
                _ => return,
            }
            event.set_handled(true);
        }, RoutingStrategies::TUNNEL, false);
        search.focus();
    }
}
