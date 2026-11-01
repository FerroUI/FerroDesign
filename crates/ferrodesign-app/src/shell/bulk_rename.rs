//! Preview batch layer names in Layers-panel order, then commit one scoped edit.

use super::*;
use ferroui_base::interactivity::RoutingStrategies;
use ferroui_controls::automation::AutomationProperties;
use regex::{Captures, Regex};

struct RenameSession {
    version: core::EditorRenderToken,
    page: NodeId,
    layers: Vec<(NodeId, String)>,
}

impl RenameSession {
    fn capture(editor: &Editor) -> Self {
        let selected: BTreeSet<_> = editor.selection.iter().copied().collect();
        let mut pending = editor
            .document
            .page(editor.active_page)
            .map(|page| page.children.clone())
            .unwrap_or_default();
        let mut layers = Vec::new();
        while let Some(id) = pending.pop() {
            let Some(node) = editor.document.nodes.get(&id) else {
                continue;
            };
            if node.locked {
                continue;
            } // Descendants inherit the lock.
            if selected.contains(&id) {
                layers.push((id, node.name.clone()));
            }
            pending.extend(node.children.iter().copied());
        }
        Self {
            version: editor.render_change_token(),
            page: editor.active_page,
            layers,
        }
    }

    fn apply(&self, editor: &mut Editor, changes: Vec<(NodeId, String)>) -> bool {
        if editor.has_active_transaction()
            || self.page != editor.active_page
            || matches!(
                editor.render_changes_since(&self.version),
                core::EditorRenderChanges::Full
            )
            || Self::capture(editor).layers != self.layers
        {
            return false;
        }
        let mut names: BTreeMap<_, _> = changes.into_iter().collect();
        let ids: Vec<_> = names.keys().copied().collect();
        editor.update_nodes(&ids, "Rename layers", |node| {
            if let Some(name) = names.remove(&node.id) {
                node.name = name;
            }
        });
        true
    }

    fn plan(
        &self,
        find: &str,
        replacement: &str,
        start: &str,
    ) -> Result<Vec<(NodeId, String)>, String> {
        let start = start
            .trim()
            .parse::<u32>()
            .map_err(|_| "Enter a nonnegative starting number.".to_owned())?;
        let pattern = if find.is_empty() {
            None
        } else {
            Some(Regex::new(find).map_err(|e| {
                format!(
                    "Invalid match. {}",
                    e.to_string().lines().last().unwrap_or("Check the pattern.")
                )
            })?)
        };
        let mut changes = Vec::new();
        for (index, (id, old)) in self.layers.iter().enumerate() {
            let captures = match &pattern {
                Some(re) => match re.captures(old) {
                    Some(captures) => Some(captures),
                    None => continue,
                },
                None => None,
            };
            let (left, right) = captures
                .as_ref()
                .map(|c| c.get(0).unwrap().range())
                .map(|r| (r.start, r.end))
                .unwrap_or((0, old.len()));
            let mut name = old[..left].to_owned();
            expand(
                &mut name,
                replacement,
                old,
                left,
                right,
                captures.as_ref(),
                u64::from(start) + index as u64,
                u64::from(start) + (self.layers.len() - index - 1) as u64,
            );
            name.push_str(&old[right..]);
            if name.trim().is_empty() {
                return Err(format!("“{old}” would have an empty name."));
            }
            if name != *old {
                changes.push((*id, name));
            }
        }
        Ok(changes)
    }
}

// Parse by Unicode scalar. Match offsets come from regex and are valid UTF-8 boundaries.
fn expand(
    out: &mut String,
    template: &str,
    old: &str,
    left: usize,
    right: usize,
    groups: Option<&Captures<'_>>,
    ascending: u64,
    descending: u64,
) {
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            out.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('$') => {
                chars.next();
                out.push('$');
            }
            Some('&') => {
                chars.next();
                out.push_str(&old[left..right]);
            }
            Some('`') => {
                chars.next();
                out.push_str(&old[..left]);
            }
            Some('\'') => {
                chars.next();
                out.push_str(&old[right..]);
            }
            Some(marker @ ('n' | 'N')) => {
                let mut width = 0;
                while chars.next_if_eq(&marker).is_some() {
                    width += 1;
                }
                let number = if marker == 'n' { ascending } else { descending };
                use std::fmt::Write;
                write!(out, "{number:0width$}").unwrap();
            }
            Some(digit @ '1'..='9')
                if groups.is_some_and(|c| c.len() > digit as usize - '0' as usize) =>
            {
                chars.next();
                let mut group = digit as usize - '0' as usize;
                let captures = groups.unwrap();
                if let Some(next) = chars.peek().and_then(|c| c.to_digit(10)) {
                    let pair = group * 10 + next as usize;
                    if pair < captures.len() {
                        group = pair;
                        chars.next();
                    }
                }
                if let Some(value) = captures.get(group) {
                    out.push_str(value.as_str());
                }
            }
            _ => out.push('$'),
        }
    }
}

impl EditorShell {
    pub(super) fn bulk_rename_dialog(self: &Rc<Self>) -> bool {
        if !self.flush_inspector_drafts() {
            return false;
        }
        self.canvas.cancel_active_edit();
        let session = Rc::new(RenameSession::capture(&self.editor.borrow()));
        if session.layers.is_empty() {
            self.toast("Unlock a selected layer to rename it.");
            return false;
        }
        self.modal(
            "Rename layers",
            "Preview names in layer order. Locked layers keep their names.",
        );
        self.control::<Border>("ModalPanel")
            .classes()
            .add("keyboard-dialog");
        let host = self.control::<StackPanel>("ModalContent");
        let label = |caption: &str| {
            let label = TextBlock::new();
            label.classes().add("rename-label");
            label.set_text(Some(caption));
            host.children().add(label);
        };
        let field = |caption: &str, name: &str, placeholder: &str, value: &str| {
            label(caption);
            let field = TextBox::new();
            field.classes().add("rename-field");
            field.set_name(Some(name.to_owned()));
            AutomationProperties::set_name(&field, Some(caption));
            field.set_placeholder_text(Some(placeholder));
            field.set_text(Some(value));
            host.children().add(field.clone());
            field
        };
        let find = field(
            "Match",
            "RenameMatch",
            "Entire name, or a regular expression",
            "",
        );
        let replace = field("Rename to", "RenameReplacement", "Layer name", "$&");
        let tokens = Grid::new();
        tokens.set_column_definitions(ColumnDefinitions::parse("*,*,*").unwrap());
        tokens.set_column_spacing(4.0);
        for (i, (caption, token, name, accessible)) in [
            ("Match", "$&", "RenameCurrent", "Insert matched text or layer name"),
            ("Number ↑", "$n", "RenameAscending", "Insert ascending number"),
            ("Number ↓", "$N", "RenameDescending", "Insert descending number"),
        ]
        .into_iter()
        .enumerate()
        {
            let weak = replace.downgrade();
            let button = self.style_button(caption, name, true, move |_| {
                if let Some(field) = weak.upgrade() {
                    field.set_selected_text(Some(token));
                    field.focus();
                }
            });
            button.classes().add("rename-token");
            AutomationProperties::set_name(&button, Some(accessible));
            ferroui_controls::ToolTip::set_tip(&button, Some(Rc::new(accessible.to_string())));
            Grid::set_column(&button, i as i32);
            tokens.children().add(button);
        }
        host.children().add(tokens);
        let start = field(
            "Number range starts at",
            "RenameStart",
            "1",
            "1",
        );
        label("Preview");
        let surface = Border::new();
        surface.classes().add("rename-preview");
        let preview = TextBlock::new();
        preview.classes().add("rename-preview");
        preview.set_name(Some("RenamePreview".into()));
        surface.set_child(preview.clone());
        host.children().add(surface);
        let actions = self.control::<StackPanel>("ModalActions");
        actions
            .children()
            .add(self.style_button("Cancel", "RenameCancel", true, |s| s.close_modal()));
        let apply = Button::new();
        apply.set_name(Some("RenameApply".into()));
        apply.classes().add("primary");
        apply.set_content(Some(Rc::new("Rename".to_owned())));
        actions.children().add(apply.clone());

        let inputs = [find.downgrade(), replace.downgrade(), start.downgrade()];
        let plan = {
            let session = session.clone();
            Rc::new(move || {
                let [find, replace, start] = inputs
                    .each_ref()
                    .map(|weak| weak.upgrade().and_then(|f| f.text()).unwrap_or_default());
                session.plan(&find, &replace, &start)
            })
        };
        let refresh = {
            let session = session.clone();
            let preview = preview.downgrade();
            let apply = apply.downgrade();
            let plan = plan.clone();
            Rc::new(move || {
                let (Some(preview), Some(apply)) = (preview.upgrade(), apply.upgrade()) else {
                    return;
                };
                let result = plan();
                apply.set_is_enabled(matches!(&result, Ok(names) if !names.is_empty()));
                let text = match result {
                    Err(message) => message,
                    Ok(names) if names.is_empty() => "No names would change.".into(),
                    Ok(names) => {
                        let mut text = format!(
                            "{} of {} names will change",
                            names.len(),
                            session.layers.len()
                        );
                        let mut originals = session.layers.iter();
                        for (id, new) in names.iter().take(6) {
                            if let Some((_, old)) = originals.find(|(key, _)| key == id) {
                                text.push_str(&format!("\n{old}  →  {new}"));
                            }
                        }
                        if names.len() > 6 {
                            text.push_str(&format!("\n…and {} more", names.len() - 6));
                        }
                        text
                    }
                };
                preview.set_text(Some(&text));
            })
        };
        let weak = Rc::downgrade(self);
        let submit = Rc::new(move || {
            let Some(shell) = weak.upgrade() else { return };
            let Ok(names) = plan() else { return };
            if names.is_empty() {
                return;
            }
            let changed = session.apply(&mut shell.editor.borrow_mut(), names);
            shell.close_modal();
            if changed {
                shell.changed();
            } else {
                shell.toast("Selection changed. Open Rename layers again.");
            }
        });
        let click = submit.clone();
        apply.click(move |_, _| click());
        for field in [&find, &replace, &start] {
            let update = refresh.clone();
            field.text_changed(move |_, _| update());
            let submit = submit.clone();
            let weak = Rc::downgrade(self);
            let source = field.downgrade();
            field.add_handler_with(
                InputElement::key_down_event(),
                move |_, event| {
                    if !event.key_modifiers.is_empty()
                        || source.upgrade().is_some_and(|f| f.is_composing_text())
                    {
                        return;
                    }
                    match event.key {
                        Key::Enter => submit(),
                        Key::Escape => {
                            if let Some(shell) = weak.upgrade() {
                                shell.close_modal();
                            }
                        }
                        _ => return,
                    }
                    event.set_handled(true);
                },
                RoutingStrategies::TUNNEL,
                false,
            );
        }
        refresh();
        // Realize the ScrollViewer before handing keyboard input over from the canvas.
        self.control::<Border>("ModalPanel").update_layout();
        replace.focus();
        replace.select_all();
        true
    }
}
