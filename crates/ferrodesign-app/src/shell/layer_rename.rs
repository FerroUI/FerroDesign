//! A rename field exists only while its virtualized Layers row is being edited.
use super::*;
use ferroui_controls::automation::AutomationProperties;

impl EditorShell {
    pub(super) fn reveal_selected_layer(self: &Rc<Self>, edit_name: bool) -> bool {
        if !self.flush_inspector_drafts() { return false; }
        self.canvas.cancel_active_edit();
        let (id, ancestors) = {
            let editor = self.editor.borrow();
            let &[id] = editor.selection.as_slice() else {
                self.toast("Select one layer first.");
                return false;
            };
            if editor.document.page_of(id) != Some(editor.active_page)
                || editor.has_active_transaction() {
                return false;
            }
            if edit_name && editor.document.effective_locked(id) {
                self.toast("Unlock the layer to rename it.");
                return false;
            }
            let mut ancestors = Vec::new();
            let mut current = id;
            while let Some(parent) = editor.document.nodes.get(&current).and_then(|n| n.parent) {
                if parent == editor.active_page { break; }
                ancestors.push(parent);
                current = parent;
            }
            (id, ancestors)
        };

        if self.compact.get() && self.compact_panel.get() != workspace::CompactPanel::Layers {
            self.compact_panel.set(workspace::CompactPanel::Layers);
            self.layout_workspace();
        }
        self.assets.set(false);
        self.expanded.borrow_mut().extend(ancestors);
        self.refresh_layers();
        if self.layer_projection.borrow().index_of(id).is_none() {
            self.field("LayerSearch").set_text(Some(""));
            self.refresh_layers();
        }
        let Some(index) = self.layer_projection.borrow().index_of(id) else { return false; };
        let host = self.control::<ItemsControl>("LayersHost");
        host.update_layout();
        host.scroll_into_view(index as i32);
        let Some(button) = self.layer_buttons.borrow().get(&id).and_then(WeakRef::upgrade) else {
            return false;
        };
        if !edit_name { return true; }
        let Some(row) = button.parent().and_then(|parent| parent.cast::<Grid>()) else { return false; };
        self.open_layer_name_field(id, &row)
    }

    fn open_layer_name_field(self: &Rc<Self>, id: NodeId, row: &Ref<Grid>) -> bool {
        let (original, version) = {
            let editor = self.editor.borrow();
            let Some(node) = editor.document.nodes.get(&id) else { return false; };
            (node.name.clone(), editor.render_change_token())
        };
        let field = TextBox::new();
        field.set_name(Some("LayerRenameEditor".into()));
        field.classes().add("layer-rename");
        AutomationProperties::set_name(&field, Some("Layer name"));
        field.set_text(Some(&original));
        Grid::set_column(&field, 1);
        Grid::set_column_span(&field, 3);
        row.children().add(field.clone());

        let active = Cell::new(true);
        let weak = Rc::downgrade(self);
        let weak_field = field.downgrade();
        let weak_row = row.downgrade();
        let committed = Rc::new(RefCell::new(original.clone()));
        let finish = Rc::new(move |accept: bool| {
            let (Some(shell), Some(field)) = (weak.upgrade(), weak_field.upgrade()) else { return; };
            if !active.replace(false) { return; }
            let value = field.text().unwrap_or_default();
            let changed = if accept && !value.trim().is_empty() && value != original {
                let mut editor = shell.editor.borrow_mut();
                // A row may be recycled after import, deletion, undo or page
                // changes. A stale draft must never rename a replacement ID.
                !matches!(editor.render_changes_since(&version), core::EditorRenderChanges::Full)
                    && editor.selection.as_slice() == [id]
                    && editor.document.nodes.get(&id).is_some_and(|node| node.name == original)
                    && !editor.has_active_transaction() && !editor.document.effective_locked(id)
                    && editor.update_node(id, "Rename layer", |n| n.name = value).is_some()
            } else { false };
            if let Some(row) = weak_row.upgrade() { row.children().remove(field); }
            if changed { shell.changed(); }
            shell.sync_document_draft_dirty();
        });
        let submit = finish.clone();
        field.add_handler(InputElement::lost_focus_event(), move |_, _| submit(true));
        let submit = finish.clone();
        let weak = Rc::downgrade(self);
        let weak_field = field.downgrade();
        field.add_handler(InputElement::key_down_event(), move |_, event| {
            if event.key != Key::Enter || weak_field.upgrade().is_some_and(|f| f.is_composing_text()) { return; }
            event.set_handled(true);
            submit(true);
            if let Some(shell) = weak.upgrade() { shell.canvas.focus(); }
        });
        let submit = finish.clone();
        let weak = Rc::downgrade(self);
        self.bind_inspector_draft_with_discard(&field, committed, Rc::new(move || submit(true)), None,
            Some(Rc::new(move || {
                finish(false);
                if let Some(shell) = weak.upgrade() { shell.canvas.focus(); }
            })));
        field.focus();
        field.select_all();
        true
    }
}
