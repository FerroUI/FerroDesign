//! Keep page controls stable while names, order and active-page state change.
use super::*;
use super::layer_rename::NameRow;
use ferroui_base::interactivity::RoutingStrategies;
use ferroui_controls::{MenuItem, ToolTip, automation::AutomationProperties, primitives::ToggleButton};

#[derive(Default)]
pub(super) struct PageList {
    rows: Vec<PageRow>,
    active: Option<NodeId>,
    title: String,
}

struct PageRow {
    id: NodeId,
    name: String,
    host: Ref<Grid>,
    button: Ref<ToggleButton>,
    label: Ref<TextBlock>,
}

impl EditorShell {
    pub(super) fn wire_page_menu(self: &Rc<Self>) {
        for (name, offset) in [("PageMoveUp", -1), ("PageMoveDown", 1)] {
            let weak = Rc::downgrade(self);
            self.control::<MenuItem>(name).click(move |_, _| {
                if let Some(shell) = weak.upgrade() { shell.move_active_page(offset); }
            });
        }
        let weak = Rc::downgrade(self);
        self.control::<MenuItem>("PageRename").click(move |_, _| {
            let Some(shell) = weak.upgrade() else { return; };
            shell.rename_active_page();
        });
    }

    pub(super) fn refresh_pages(self: &Rc<Self>) {
        let editor = self.editor.borrow();
        let pages = &editor.document.pages;
        let active = editor.active_page;
        let mut list = self.page_list.take();
        let names_changed = !list.rows.iter().map(|row| (row.id, &row.name))
            .eq(pages.iter().map(|page| (page.id, &page.name)));
        let active_changed = list.active.replace(active) != Some(active);
        let title_changed = list.title != editor.document.name;
        if !active_changed && !names_changed && !title_changed {
            self.page_list.replace(list);
            return;
        }
        let entries = names_changed.then(|| pages.iter().map(|page| (page.id, page.name.clone())).collect::<Vec<_>>());
        let title = &editor.document.name;
        if title_changed {
            list.title.clone_from(title);
            self.text("FileTitle", title);
            self.text("CompactDocumentTitle", title);
        }
        let page = editor.document.page(active).map(|p| p.name.as_str()).unwrap_or("");
        self.text("CanvasCaption", &format!("{title} / {page}"));
        self.text("CompactPageTitle", page);
        drop(editor);

        if let Some(entries) = entries {
            let children = self.control::<StackPanel>("PagesHost").children();
            let mut old: BTreeMap<_, _> = list.rows.drain(..).map(|row| (row.id, row)).collect();
            for (index, (id, name)) in entries.into_iter().enumerate() {
                let mut row = old.remove(&id).unwrap_or_else(|| self.new_page_row(id));
                if row.name != name || (name.is_empty() && row.label.text().is_none()) {
                    row.label.set_text(Some(&name));
                    AutomationProperties::set_name(&row.button, Some(&name));
                    ToolTip::set_tip(&row.button, Some(Rc::new(name.clone())));
                    row.name = name;
                }
                let host = row.host.clone().upcast::<Control>();
                if index == children.count() {
                    children.add(host);
                } else if children.get(index) != host {
                    if let Some(previous) = children.index_of(&host) { children.move_item(previous, index); }
                    else { children.insert(index, host); }
                }
                list.rows.push(row);
            }
            for row in old.into_values() { children.remove(row.host); }
        }
        let mut selected = None;
        for (index, row) in list.rows.iter().enumerate() {
            let current = row.id == active;
            if row.button.is_checked() != Some(current) { row.button.set_is_checked(Some(current)); }
            if current { selected = Some(index); }
        }
        self.control::<MenuItem>("PageMoveUp").set_is_enabled(selected.is_some_and(|i| i > 0));
        self.control::<MenuItem>("PageMoveDown").set_is_enabled(selected.is_some_and(|i| i + 1 < list.rows.len()));
        if let Some(index) = selected.filter(|_| active_changed || names_changed) { list.rows[index].host.bring_into_view(); }
        self.page_list.replace(list);
    }

    fn new_page_row(self: &Rc<Self>, id: NodeId) -> PageRow {
        let host = Grid::new();
        let button = ToggleButton::new();
        button.classes().add("page-row");
        let content = Grid::new();
        content.set_column_definitions(ColumnDefinitions::parse("19,*").unwrap());
        let icon = icons::icon("file", "#666666", 13.0);
        icon.set_horizontal_alignment(ferroui_base::layout::HorizontalAlignment::Left);
        content.children().add(icon);
        let label = TextBlock::new();
        label.set_text_trimming(<dyn ferroui_base::media::TextTrimming>::character_ellipsis());
        label.set_vertical_alignment(VerticalAlignment::Center);
        Grid::set_column(&label, 1);
        content.children().add(label.clone());
        button.set_content(Some(Control::boxed(content)));
        host.children().add(button.clone());
        let twice = Rc::new(Cell::new(false));
        let pressed = twice.clone();
        button.add_handler_with(InputElement::pointer_pressed_event(), move |_, e| {
            pressed.set(e.properties().is_left_button_pressed && e.click_count() == 2);
        }, RoutingStrategies::TUNNEL, true);
        let weak = Rc::downgrade(self);
        button.click(move |_, _| {
            let Some(shell) = weak.upgrade() else { return; };
            let rename = twice.replace(false);
            if shell.go_to_page(id) {
                if rename { shell.rename_active_page(); } else { shell.canvas.focus(); }
            }
            // Clicking the active ToggleButton or rejecting an invalid inspector edit
            // must not leave the accessible checked state out of sync with the document.
            let active = shell.editor.borrow().active_page;
            for row in &shell.page_list.borrow().rows {
                if row.id == id { row.button.set_is_checked(Some(id == active)); break; }
            }
        });
        let weak = Rc::downgrade(self);
        button.add_handler(InputElement::key_down_event(), move |_, e| {
            if e.key != Key::F2 || !e.key_modifiers.is_empty() { return; }
            if let Some(shell) = weak.upgrade() {
                if shell.go_to_page(id) { shell.rename_active_page(); }
                e.set_handled(true);
            }
        });
        let weak = Rc::downgrade(self);
        let target = button.downgrade();
        button.add_handler(InputElement::context_requested_event(), move |_, e| {
            let (Some(shell), Some(target)) = (weak.upgrade(), target.upgrade()) else { return; };
            if shell.go_to_page(id) {
                if let Some(menu) = shell.button("PageActionsButton").flyout() { menu.show_at(&target); }
            }
            e.set_handled(true);
        });
        PageRow { id, name: String::new(), host, button, label }
    }

    fn go_to_page(self: &Rc<Self>, id: NodeId) -> bool {
        if self.editor.borrow().active_page == id { return true; }
        if !self.flush_inspector_drafts() { return false; }
        self.finish_inline_text(true);
        self.canvas.cancel_active_edit();
        let mut editor = self.editor.borrow_mut();
        if editor.document.page(id).is_none() { return false; }
        editor.active_page = id;
        editor.selection.clear();
        editor.refresh();
        drop(editor);
        self.canvas.reset_fit();
        self.refresh();
        true
    }

    fn rename_active_page(self: &Rc<Self>) {
        if !self.flush_inspector_drafts() { return; }
        self.canvas.cancel_active_edit();
        let id = self.editor.borrow().active_page;
        let row = self.page_list.borrow().rows.iter().find(|r| r.id == id).map(|r| r.host.clone());
        if let Some(row) = row {
            row.bring_into_view();
            self.open_row_name(NameRow::Page(id), &row);
        }
    }

    fn move_active_page(self: &Rc<Self>, offset: isize) {
        if !self.flush_inspector_drafts() { return; }
        self.canvas.cancel_active_edit();
        let mut editor = self.editor.borrow_mut();
        let Some(index) = editor.document.pages.iter().position(|p| p.id == editor.active_page) else { return; };
        let Some(next) = index.checked_add_signed(offset).filter(|&i| i < editor.document.pages.len()) else { return; };
        editor.transact("Reorder pages", |doc| doc.pages.swap(index, next));
        drop(editor);
        self.changed();
        self.canvas.focus();
    }
}
