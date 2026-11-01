//! Keyboard and pointer activation share the editor command dispatcher.
use super::*;
use ferroui_base::{input::{KeyboardNavigation, KeyboardNavigationMode}, interactivity::RoutingStrategies};
use ferroui_controls::{automation::AutomationProperties, ScrollViewer};

pub(super) const QUICK_ACTIONS_TITLE: &str = "Quick actions";

struct ActionRow {
    label: String,
    shortcut: String,
    searchable: String,
    command: &'static str,
    button: Ref<Button>,
}

impl ActionRow {
    fn rank(&self, query: &str, words: &[&str]) -> Option<u8> {
        if query.is_empty() || self.label == query || self.shortcut == query || self.command == query {
            return Some(0);
        }
        if !words.iter().all(|word| self.searchable.contains(word)) {
            return None;
        }
        if self.label.starts_with(query) { return Some(1); }
        if words.iter().all(|word| self.label.split_whitespace().any(|label| label.starts_with(word))) {
            return Some(2);
        }
        Some(3)
    }
}

struct ActionResults {
    rows: Vec<ActionRow>,
    list: Ref<StackPanel>,
    viewport: Ref<ScrollViewer>,
    search: WeakRef<TextBox>,
    matches: RefCell<Vec<usize>>,
    query: RefCell<Option<String>>,
    active: Cell<Option<usize>>,
    empty: Ref<TextBlock>,
    count: Ref<TextBlock>,
}

impl ActionResults {
    fn filter(&self, text: &str) {
        let query = text.trim().to_lowercase().replace('⇧', "shift+").replace('⌘', "ctrl+").replace("cmd+", "ctrl+");
        if self.query.borrow().as_ref() == Some(&query) {
            return;
        }
        let words: Vec<_> = query.split_whitespace().collect();
        let mut ranked = Vec::new();
        for (index, row) in self.rows.iter().enumerate() {
            let rank = row.rank(&query, &words);
            if row.button.is_visible() != rank.is_some() {
                row.button.set_is_visible(rank.is_some());
            }
            if let Some(rank) = rank { ranked.push((rank, index)); }
        }
        ranked.sort_unstable();
        let matches: Vec<_> = ranked.into_iter().map(|(_, index)| index).collect();
        // Keep the same native Button instances, names and click handlers while
        // making visual, keyboard and automation order agree.
        let children = self.list.children();
        let mut position = 0;
        for &index in &matches {
            // Collapsed rows occupy no layout space. Leave them in place to avoid
            // logical and visual tree moves on a filter that only narrows the list.
            while !children.get(position).is_visible() {
                position += 1;
            }
            let control = self.rows[index].button.clone().upcast::<Control>();
            if children.get(position) != control {
                let old = children.index_of(&control).expect("action row belongs to the list");
                children.move_item(old, position);
            }
            position += 1;
        }
        self.empty.set_is_visible(matches.is_empty());
        let noun = if matches.len() == 1 { "action" } else { "actions" };
        self.count.set_text(Some(&format!("{} {noun}", matches.len())));
        let first = matches.first().copied();
        *self.matches.borrow_mut() = matches;
        *self.query.borrow_mut() = Some(query);
        self.viewport.scroll_to_home();
        self.highlight(first);
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
        if let Some(search) = self.search.upgrade() {
            let status = active.map(|i| {
                let matches = self.matches.borrow();
                let position = matches.iter().position(|&index| index == i).unwrap() + 1;
                let name = AutomationProperties::get_name(&self.rows[i].button).unwrap_or_default();
                format!("{name}, {position} of {}", matches.len())
            });
            AutomationProperties::set_item_status(&search, Some(status.as_deref().unwrap_or("No matching actions")));
        }
    }

    fn step(&self, down: bool) {
        let Some(current) = self.active.get() else { return; };
        let matches = self.matches.borrow();
        let Some(position) = matches.iter().position(|&index| index == current) else { return; };
        let next = if down { position.checked_add(1) } else { position.checked_sub(1) };
        if let Some(&index) = next.and_then(|position| matches.get(position)) {
            self.highlight(Some(index));
            self.rows[index].button.bring_into_view();
        }
    }
}

impl EditorShell {
    pub(super) fn quick_actions(self: &Rc<Self>) {
        let mut seen = BTreeSet::new();
        let actions: Vec<_> = ACTIONS.iter().chain(FILE_ACTIONS).chain(LAYER_ACTIONS)
            .copied().filter(|&(_, id)| seen.insert(id)).collect();
        self.action_dialog(QUICK_ACTIONS_TITLE, "Find a tool, file action, or layer command.", &actions);
    }

    pub(super) fn wire_action_dialog(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.button("CloseModalButton").add_handler(InputElement::key_down_event(), move |_, event| {
            if event.key != Key::Escape || !event.key_modifiers.is_empty() { return; }
            if let Some(shell) = weak.upgrade().filter(|s| s.control::<Border>("DialogSearchHost").is_visible()) {
                shell.close_modal();
                event.set_handled(true);
            }
        });
    }

    pub(super) fn reset_action_search(&self) {
        let host = self.control::<Border>("DialogSearchHost");
        if host.is_visible() {
            host.set_is_visible(false);
            host.set_child(None);
            self.control::<Border>("ModalPanel").classes().remove("action-dialog");
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
        self.control::<Border>("ModalPanel").classes().add("action-dialog");
        let host = self.control::<StackPanel>("ModalContent");
        let search = TextBox::new();
        search.set_name(Some("ActionSearch".into()));
        search.set_placeholder_text(Some("Search by name or shortcut…"));
        search.classes().add("clearButton");
        AutomationProperties::set_help_text(&search, Some("Use Up and Down to choose an action, Enter to run it, and Escape to close."));
        let magnifier = icons::icon("search", "#666666", 14.0);
        magnifier.set_margin(Thickness::new(8.0, 0.0, 2.0, 0.0));
        search.set_inner_left_content(Some(Control::boxed(magnifier)));
        AutomationProperties::set_name(&search, Some("Search actions"));
        let header = self.control::<Border>("DialogSearchHost");
        header.set_child(Some(search.clone().upcast::<Control>()));
        header.set_is_visible(true);

        let list = StackPanel::new();
        KeyboardNavigation::set_tab_navigation(&list, KeyboardNavigationMode::None);
        list.set_spacing(2.0);
        host.children().add(list.clone());
        let mut rows = Vec::with_capacity(actions.len());
        for &(caption, command) in actions {
            let (label, shortcut) = caption.split_once("  ").unwrap_or((caption, ""));
            let button = Button::new();
            button.set_name(Some(format!("Action-{command}")));
            button.classes().add("layer");
            button.classes().add("action-result");
            AutomationProperties::set_name(&button, Some(label));
            if !shortcut.is_empty() { AutomationProperties::set_accelerator_key(&button, shortcut); }
            let content = Grid::new();
            content.set_column_definitions(ColumnDefinitions::parse("8,*,Auto").unwrap());
            let marker = Border::new();
            marker.classes().add("action-marker");
            content.children().add(marker);
            let name = TextBlock::new();
            name.classes().add("action-caption");
            name.set_text_trimming(<dyn ferroui_base::media::TextTrimming>::character_ellipsis());
            Grid::set_column(&name, 1);
            name.set_text(Some(label));
            name.set_vertical_alignment(VerticalAlignment::Center);
            content.children().add(name);
            if !shortcut.is_empty() {
                let hint = TextBlock::new();
                hint.set_text(Some(shortcut));
                hint.classes().add("label");
                hint.set_vertical_alignment(VerticalAlignment::Center);
                Grid::set_column(&hint, 2);
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
                label: label.trim_end_matches('…').to_lowercase(),
                shortcut: shortcut.to_lowercase().replace('⇧', "shift+"),
                searchable: format!("{} {command}", caption.to_lowercase()).replace('-', " "),
                command,
                button,
            });
        }
        let empty = TextBlock::new();
        empty.set_name(Some("ActionEmpty".into()));
        empty.set_text(Some("No matching actions. Try a different name or shortcut."));
        empty.set_text_wrapping(TextWrapping::Wrap);
        empty.classes().add("label");
        host.children().add(empty.clone());
        let count = TextBlock::new();
        count.classes().add("label");
        let footer = self.control::<StackPanel>("ModalActions");
        footer.children().add(count.clone());
        let keys = TextBlock::new();
        keys.set_text(Some("↑ ↓  Choose     Enter  Run"));
        keys.classes().add("label");
        footer.children().add(keys);
        let results = Rc::new(ActionResults {
            rows, list, viewport: self.control("ModalViewport"), search: search.downgrade(),
            matches: RefCell::default(), query: RefCell::default(), active: Cell::new(None), empty, count,
        });
        results.filter("");
        let search_ref = search.downgrade();
        let filtered = results.clone();
        search.text_changed(move |_, _| {
            if let Some(field) = search_ref.upgrade().filter(|field| field.is_attached_to_visual_tree()) {
                filtered.filter(&field.text().unwrap_or_default());
            }
        });
        let weak = Rc::downgrade(self);
        let field = search.downgrade();
        search.add_handler_with(InputElement::key_down_event(), move |_, event| {
            let Some(shell) = weak.upgrade() else { return; };
            let Some(search) = field.upgrade() else { return; };
            if search.is_composing_text() || !event.key_modifiers.is_empty() { return; }
            if matches!(event.key, Key::Down | Key::Up | Key::Enter) {
                // TextChanged is deferred. A fast Enter must use the actual field value.
                results.filter(&search.text().unwrap_or_default());
            }
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
