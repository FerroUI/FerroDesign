//! Keyboard navigation follows the displayed tree. The editor selection and
//! keyboard focus can move independently, even across virtualized rows.
use super::layer_projection::TreeAction;
use super::*;
use ferroui_base::input::{KeyEventArgs, KeyModifiers, NavigationMethod};
use ferroui_base::interactivity::RoutingStrategies;
use ferroui_controls::automation::AutomationProperties;

impl EditorShell {
    pub(super) fn sync_tree_disclosure(&self) {
        let action = self
            .layer_projection
            .borrow()
            .tree_action(&self.editor.borrow());
        self.visible("CollapseLayersButton", action.is_some());
        let Some(action) = action else { return };
        let button = self.button("CollapseLayersButton");
        button.set_is_enabled(action != TreeAction::SelectionOnly);
        self.visible("ExpandTreeIcon", action == TreeAction::Expand);
        self.visible("CollapseTreeIcon", action != TreeAction::Expand);
        let name = match action {
            TreeAction::Expand => "Expand all layers",
            TreeAction::Collapse => "Collapse all layers",
            TreeAction::CollapseOthers => "Collapse other layers",
            TreeAction::SelectionOnly => "Selected layers remain visible",
        };
        if AutomationProperties::get_name(&button).as_deref() != Some(name) {
            AutomationProperties::set_name(&button, Some(name));
            ferroui_controls::ToolTip::set_tip(&button, Some(Rc::new(name.to_owned())));
        }
    }

    pub(super) fn realize_layer_button(&self, id: NodeId) -> Option<Ref<Button>> {
        let index = self.layer_projection.borrow().index_of(id)?;
        let host = self.control::<ItemsControl>("LayersHost");
        host.update_layout();
        host.scroll_into_view(index as i32);
        host.update_layout();
        self.layer_buttons
            .borrow()
            .get(&id)?
            .upgrade()
            .filter(|row| row.is_attached_to_visual_tree())
    }

    pub(super) fn focus_layer_row(&self, id: NodeId, method: NavigationMethod) -> bool {
        self.realize_layer_button(id)
            .is_some_and(|button| button.focus_with(method, KeyModifiers::NONE))
    }

    pub(super) fn disclose_layer(self: &Rc<Self>, id: NodeId, open: bool) {
        if open {
            self.expanded.borrow_mut().insert(id);
        } else {
            self.expanded.borrow_mut().remove(&id);
        }
        self.refresh_layers();
        self.focus_layer_row(id, NavigationMethod::Directional);
    }

    pub(super) fn enter_layer_rows(self: &Rc<Self>) -> bool {
        if self.assets.get() {
            return false;
        }
        let query = self
            .field("LayerSearch")
            .text()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let target = {
            let projection = self.layer_projection.borrow();
            if query.is_empty() {
                self.editor
                    .borrow()
                    .selection
                    .last()
                    .copied()
                    .filter(|id| projection.index_of(*id).is_some())
                    .or_else(|| projection.rows().first().map(|row| row.id))
            } else {
                projection
                    .rows()
                    .iter()
                    .find(|row| row.name.to_lowercase().contains(&query))
                    .map(|row| row.id)
            }
        };
        target.is_some_and(|id| self.navigate_to_layer(id, KeyModifiers::NONE))
    }

    fn navigate_to_layer(self: &Rc<Self>, id: NodeId, modifiers: KeyModifiers) -> bool {
        if !self.flush_inspector_drafts() {
            return false;
        }
        self.canvas.cancel_active_edit();
        if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::META) {
            self.layer_projection
                .borrow()
                .select_row(&mut self.editor.borrow_mut(), id, modifiers);
            self.refresh_selection();
            self.canvas.invalidate_visual();
        }
        self.focus_layer_row(id, NavigationMethod::Directional)
    }

    pub(super) fn wire_layer_navigation(self: &Rc<Self>, button: &Ref<Button>, id: NodeId) {
        // Button normally invokes Click on Space release. Consume both phases of
        // the toggle chord, even if Ctrl/Cmd is released before Space.
        let toggling = Rc::new(Cell::new(false));
        let pressed = toggling.clone();
        let weak = Rc::downgrade(self);
        button.add_handler_with(
            InputElement::key_down_event(),
            move |_, event| {
                let Some(shell) = weak.upgrade() else { return };
                if shell.has_layer_drag() {
                    return;
                }
                if event.key == Key::Space
                    && (event.key_modifiers == KeyModifiers::CONTROL
                        || event.key_modifiers == KeyModifiers::META)
                {
                    if !pressed.replace(true) {
                        shell.layer_projection.borrow().select_row(
                            &mut shell.editor.borrow_mut(),
                            id,
                            event.key_modifiers,
                        );
                        shell.refresh_selection();
                        shell.canvas.invalidate_visual();
                    }
                    event.set_handled(true);
                } else if shell.layer_row_key(id, event) {
                    event.set_handled(true);
                }
            },
            RoutingStrategies::TUNNEL,
            false,
        );
        button.add_handler_with(
            InputElement::key_up_event(),
            move |_, event| {
                if event.key == Key::Space && toggling.replace(false) {
                    event.set_handled(true);
                }
            },
            RoutingStrategies::TUNNEL,
            false,
        );
    }

    fn layer_row_key(self: &Rc<Self>, id: NodeId, event: &KeyEventArgs) -> bool {
        let mods = event.key_modifiers;
        if event.key == Key::Escape && mods.is_empty() {
            if self.control::<Border>("DrawerBackdrop").is_visible() {
                return false;
            }
            return self.canvas.focus();
        }
        if !mods.contains(KeyModifiers::ALT)
            && matches!(
                event.key,
                Key::Up | Key::Down | Key::Home | Key::End | Key::Left | Key::Right
            )
        {
            let projection = self.layer_projection.borrow();
            let rows = projection.rows();
            let Some(index) = projection.index_of(id) else {
                return false;
            };
            let row = &rows[index];
            let target = match event.key {
                Key::Up => rows.get(index.saturating_sub(1)),
                Key::Down => rows.get((index + 1).min(rows.len() - 1)),
                Key::Home => rows.first(),
                Key::End => rows.last(),
                Key::Left | Key::Right => {
                    let open = event.key == Key::Right;
                    if row.expanded == Some(!open) {
                        drop(projection);
                        self.disclose_layer(id, open);
                        return true;
                    }
                    if open {
                        rows.get(index + 1).filter(|next| next.depth > row.depth)
                    } else {
                        rows[..index]
                            .iter()
                            .rev()
                            .find(|parent| parent.depth < row.depth)
                    }
                }
                _ => unreachable!(),
            }
            .map(|row| row.id);
            drop(projection);
            if let Some(target) = target {
                self.navigate_to_layer(target, mods);
            }
            return true;
        }
        if let Some(action) = self.canvas.editor_shortcut(event) {
            if action == "rename-layer" && !self.editor.borrow().selection.contains(&id) {
                if !self.navigate_to_layer(id, KeyModifiers::NONE) {
                    return true;
                }
            }
            self.command(action);
            return true;
        }
        false
    }
}
