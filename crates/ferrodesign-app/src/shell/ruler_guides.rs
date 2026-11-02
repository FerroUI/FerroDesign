//! Exact, keyboard-accessible counterparts of the canvas ruler gestures.
use super::styles::style_label;
use super::*;
use ferrodesign_core::{Guide, RulerGuideId};
use ferroui_base::interactivity::RoutingStrategies;
use ferroui_controls::automation::AutomationProperties;

const PER_PAGE: usize = 8;

impl EditorShell {
    pub(super) fn ruler_guides_dialog(self: &Rc<Self>, start: usize) {
        if !self.flush_inspector_drafts() {
            return;
        }
        self.canvas.cancel_active_edit();
        self.canvas.set_rulers_visible(true);
        let selected = self.canvas.selected_ruler_guide();
        let (page, frame) = {
            let editor = self.editor.borrow();
            (
                editor.active_page,
                editor.selection.first().copied().filter(|id| {
                    editor
                        .document
                        .ruler_owner_transform(editor.active_page, *id)
                        .is_some()
                        && !editor.document.effective_locked(*id)
                }),
            )
        };
        let owner = selected.map(|id| id.owner).or(frame).unwrap_or(page);
        let offset = selected.map_or(start, |id| id.index / PER_PAGE * PER_PAGE);
        self.show_ruler_guides(owner, offset);
    }

    fn show_ruler_guides(self: &Rc<Self>, owner: NodeId, offset: usize) {
        let (page, title, total, first, entries, version, selected_frame, frame_size) = {
            let e = self.editor.borrow();
            let d = &e.document;
            if d.ruler_owner_transform(e.active_page, owner).is_none() || d.effective_locked(owner)
            {
                return;
            }
            let list = d.guides.get(&owner).map(Vec::as_slice).unwrap_or_default();
            let first = offset.min(list.len().saturating_sub(1)) / PER_PAGE * PER_PAGE;
            let title = d
                .nodes
                .get(&owner)
                .map(|n| format!("Frame · {}", n.name))
                .or_else(|| d.page(owner).map(|p| format!("Page · {}", p.name)))
                .unwrap_or_default();
            let frame = e.selection.first().copied().filter(|id| {
                d.ruler_owner_transform(e.active_page, *id).is_some() && !d.effective_locked(*id)
            });
            (
                e.active_page,
                title,
                list.len(),
                first,
                list.iter()
                    .skip(first)
                    .take(PER_PAGE)
                    .copied()
                    .collect::<Vec<_>>(),
                e.render_change_token(),
                frame,
                d.nodes.get(&owner).map(|node| (node.width, node.height)),
            )
        };
        self.modal("Ruler guides", &title);
        let panel = self.control::<Border>("ModalPanel");
        panel.classes().add("keyboard-dialog");
        let host = self.control::<StackPanel>("ModalContent");
        let add = Grid::new();
        add.set_column_definitions(ColumnDefinitions::parse("*,*").unwrap());
        add.set_column_spacing(4.);
        for (i, (label, horizontal, name)) in [
            ("Add vertical", false, "RulerAddVertical"),
            ("Add horizontal", true, "RulerAddHorizontal"),
        ]
        .into_iter()
        .enumerate()
        {
            let token = version.clone();
            let button = self.style_button(label, name, true, move |s| {
                if token != s.editor.borrow().render_change_token() {
                    s.ruler_guides_dialog(0);
                    return;
                }
                let v = s.canvas.viewport.get();
                let p = v.screen_to_world(Point::new(v.width / 2., v.height / 2.));
                let position = s.editor.borrow().document.nodes.get(&owner).map_or_else(
                    || if horizontal { p.y.round() } else { p.x.round() },
                    |n| {
                        if horizontal {
                            n.height / 2.
                        } else {
                            n.width / 2.
                        }
                    },
                );
                if s.canvas.commit_ruler(
                    None,
                    Some((
                        owner,
                        Guide {
                            horizontal,
                            position,
                        },
                    )),
                    false,
                ) {
                    s.show_ruler_guides(owner, total / PER_PAGE * PER_PAGE);
                }
            });
            button.classes().add("ruler-add");
            Grid::set_column(&button, i as i32);
            add.children().add(button);
        }
        let first_add = add
            .children()
            .get(0)
            .cast::<Button>()
            .expect("vertical guide action");
        host.children().add(add);
        if owner != page {
            host.children().add(self.style_button(
                "Show page guides",
                "RulerPage",
                true,
                move |s| s.show_ruler_guides(page, 0),
            ));
        } else if let Some(frame) = selected_frame {
            host.children().add(self.style_button(
                "Show selected frame guides",
                "RulerFrame",
                true,
                move |s| s.show_ruler_guides(frame, 0),
            ));
        }
        let preferred = self
            .canvas
            .selected_ruler_guide()
            .filter(|guide| {
                guide.owner == owner && (first..first + entries.len()).contains(&guide.index)
            })
            .map_or_else(
                || offset.clamp(first, total.saturating_sub(1)),
                |guide| guide.index,
            );
        let mut focus = None;
        if total == 0 {
            host.children().add(style_label(
                "Drag a guide from either ruler, or add one here.",
            ));
        }
        for (n, guide) in entries.into_iter().enumerate() {
            let index = first + n;
            let id = RulerGuideId { owner, index };
            let row = Grid::new();
            row.set_column_definitions(ColumnDefinitions::parse("Auto,*,Auto,Auto").unwrap());
            row.set_column_spacing(4.);
            let axis = if guide.horizontal { "Y" } else { "X" };
            let label = style_label(&format!("{} · {axis}", index + 1));
            label.set_vertical_alignment(VerticalAlignment::Center);
            row.children().add(label);
            let field = TextBox::new();
            field.set_name(Some(format!("RulerPosition{index}")));
            field.set_text(Some(&guide.position.to_string()));
            field.classes().add("ruler-position");
            AutomationProperties::set_name(
                &field,
                Some(&format!("Guide {} {axis} position", index + 1)),
            );
            Grid::set_column(&field, 1);
            row.children().add(field.clone());
            if index == preferred {
                focus = Some(field.clone());
            }
            let error = TextBlock::new();
            error.set_name(Some(format!("RulerError{index}")));
            error.classes().add("ruler-error");
            let limit =
                frame_size.map(|(width, height)| if guide.horizontal { height } else { width });
            let message = limit.map_or_else(
                || "Enter a finite number, for example -12.75.".to_owned(),
                |end| format!("Enter a position between 0 and {end}."),
            );
            error.set_text(Some(&message));
            error.set_is_visible(false);
            let validation = error.downgrade();
            let input = field.downgrade();
            field.text_changing(move |_, _| {
                if let (Some(field), Some(error)) = (input.upgrade(), validation.upgrade()) {
                    if error.is_visible() {
                        error.set_is_visible(false);
                        AutomationProperties::set_item_status(&field, None);
                    }
                }
            });
            let validation = error.downgrade();
            let input = field.downgrade();
            let token = version.clone();
            let apply = Rc::new(move |s: &Rc<EditorShell>| {
                let Some(field) = input.upgrade() else { return };
                if s.editor.borrow().render_change_token() != token {
                    s.ruler_guides_dialog(0);
                    return;
                }
                let Some(position) = field
                    .text()
                    .unwrap_or_default()
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite() && limit.is_none_or(|end| (0.0..=end).contains(n)))
                else {
                    field.focus();
                    if let Some(error) = validation.upgrade() {
                        error.set_is_visible(true);
                        AutomationProperties::set_item_status(&field, error.text().as_deref());
                        error.bring_into_view();
                    }
                    return;
                };
                if s.canvas.commit_ruler(
                    Some(id),
                    Some((owner, Guide { position, ..guide })),
                    false,
                ) {
                    s.show_ruler_guides(owner, first);
                } else {
                    field.focus();
                }
            });
            let invoke = apply.clone();
            let weak = Rc::downgrade(self);
            let input = field.downgrade();
            field.add_handler_with(
                InputElement::key_down_event(),
                move |_, e| {
                    if e.key == Key::Enter
                        && e.key_modifiers.is_empty()
                        && !input.upgrade().is_some_and(|f| f.is_composing_text())
                    {
                        if let Some(s) = weak.upgrade() {
                            invoke(&s);
                        }
                        e.set_handled(true);
                    }
                },
                RoutingStrategies::TUNNEL,
                false,
            );
            let set = self.style_button("Set", &format!("RulerApply{index}"), true, move |s| {
                apply(s)
            });
            AutomationProperties::set_name(
                &set,
                Some(&format!("Set guide {} {axis} position", index + 1)),
            );
            Grid::set_column(&set, 2);
            row.children().add(set);
            let token = version.clone();
            let remove =
                self.style_button("Remove", &format!("RulerRemove{index}"), true, move |s| {
                    if s.editor.borrow().render_change_token() != token {
                        s.ruler_guides_dialog(0);
                        return;
                    }
                    if s.canvas.commit_ruler(Some(id), None, false) {
                        s.show_ruler_guides(owner, index);
                    }
                });
            AutomationProperties::set_name(&remove, Some(&format!("Remove guide {}", index + 1)));
            Grid::set_column(&remove, 3);
            row.children().add(remove);
            host.children().add(row);
            host.children().add(error);
        }
        if total > PER_PAGE {
            host.children().add(style_label(&format!(
                "Guides {}–{} of {total}",
                first + 1,
                (first + PER_PAGE).min(total)
            )));
            let nav = StackPanel::new();
            nav.set_orientation(Orientation::Horizontal);
            nav.children().add(self.style_button(
                "Previous",
                "RulerPrevious",
                first > 0,
                move |s| s.show_ruler_guides(owner, first.saturating_sub(PER_PAGE)),
            ));
            nav.children().add(self.style_button(
                "Next",
                "RulerNext",
                first + PER_PAGE < total,
                move |s| s.show_ruler_guides(owner, first + PER_PAGE),
            ));
            host.children().add(nav);
        }
        let done = self.style_button("Done", "RulerDone", true, |s| s.close_modal());
        done.classes().add("primary");
        self.control::<StackPanel>("ModalActions")
            .children()
            .add(done);
        panel.update_layout();
        if let Some(field) = focus {
            field.focus();
            field.select_all();
            field.bring_into_view();
        } else {
            first_add.focus();
            first_add.bring_into_view();
        }
    }
}
