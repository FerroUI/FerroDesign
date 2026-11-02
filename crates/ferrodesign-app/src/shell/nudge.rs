//! Local workspace preference. Dismiss with Done, Close or the backdrop to
//! apply both fields together; Escape discards the in-progress values.
use super::{styles::style_label, *};
use ferroui_base::interactivity::RoutingStrategies;
use ferroui_controls::automation::AutomationProperties;

pub(super) struct NudgeForm {
    small: Ref<TextBox>,
    big: Ref<TextBox>,
    error: Ref<TextBlock>,
}

impl EditorShell {
    pub(super) fn wire_nudge_backdrop(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.control::<Border>("ModalBackdrop").add_handler_with(
            InputElement::pointer_pressed_event(),
            move |_, e| {
                let Some(s) = weak.upgrade() else { return };
                if s.nudge_form.borrow().is_none() || !e.properties().is_left_button_pressed {
                    return;
                }
                let panel = s.control::<Border>("ModalPanel");
                let p = e.get_position(Some(&panel));
                let bounds = panel.bounds();
                if p.x < 0.0 || p.y < 0.0 || p.x >= bounds.width || p.y >= bounds.height {
                    e.set_handled(true);
                    s.apply_nudge_settings();
                }
            },
            RoutingStrategies::TUNNEL,
            false,
        );
    }

    pub(super) fn nudge_dialog(self: &Rc<Self>) {
        self.modal("Nudge amount", "Choose how far layers move with an arrow key or Shift + arrow. Values are in document units.");
        self.control::<Border>("ModalPanel")
            .classes()
            .add("keyboard-dialog");
        let host = self.control::<StackPanel>("ModalContent");
        let error = TextBlock::new();
        error.set_name(Some("NudgeError".into()));
        error.classes().add("ruler-error");
        error.set_text(Some(
            "Enter an amount greater than 0 and at most 10,000,000.",
        ));
        error.set_is_visible(false);
        let field = |name: &str, label: &str, value: f64| {
            host.children().add(style_label(label));
            let input = TextBox::new();
            input.set_name(Some(name.into()));
            AutomationProperties::set_name(&input, Some(label));
            input.classes().add("ruler-position");
            input.set_text(Some(&value.to_string()));
            host.children().add(input.clone());
            let weak = Rc::downgrade(self);
            let weak_input = input.downgrade();
            input.add_handler_with(
                InputElement::key_down_event(),
                move |_, e| {
                    if e.key == Key::Enter
                        && e.key_modifiers.is_empty()
                        && weak_input.upgrade().is_some_and(|f| !f.is_composing_text())
                    {
                        if let Some(s) = weak.upgrade() {
                            s.apply_nudge_settings();
                        }
                        e.set_handled(true);
                    }
                },
                RoutingStrategies::TUNNEL,
                false,
            );
            let validation = error.downgrade();
            let entry = input.downgrade();
            input.text_changing(move |_, _| {
                if let (Some(validation), Some(entry)) = (validation.upgrade(), entry.upgrade()) {
                    validation.set_is_visible(false);
                    AutomationProperties::set_item_status(&entry, None);
                }
            });
            input
        };
        let current = self.canvas.nudge.get();
        let small = field("NudgeSmall", "Small nudge", current.small);
        let big = field("NudgeBig", "Big nudge", current.big);
        host.children().add(error.clone());
        *self.nudge_form.borrow_mut() = Some(NudgeForm {
            small: small.clone(),
            big,
            error,
        });
        let footer = self.control::<StackPanel>("ModalActions");
        footer
            .children()
            .add(self.style_button("Reset", "NudgeReset", true, |s| {
                if let Some(form) = s.nudge_form.borrow().as_ref() {
                    let defaults = platform::NudgeAmounts::default();
                    form.small.set_text(Some(&defaults.small.to_string()));
                    form.big.set_text(Some(&defaults.big.to_string()));
                    form.small.focus();
                    form.small.select_all();
                }
            }));
        let done = self.style_button("Done", "NudgeDone", true, |s| {
            s.apply_nudge_settings();
        });
        done.classes().add("primary");
        footer.children().add(done);
        self.control::<Border>("ModalPanel").update_layout();
        small.focus();
        small.select_all();
    }

    pub(super) fn apply_nudge_settings(&self) -> bool {
        let draft = self.nudge_form.borrow();
        let Some(form) = draft.as_ref() else {
            return false;
        };
        let parse = |field: &TextBox| {
            let value = field
                .text()
                .unwrap_or_default()
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|&value| platform::NudgeAmounts::valid(value));
            if value.is_none() {
                form.error.set_is_visible(true);
                AutomationProperties::set_item_status(field, form.error.text().as_deref());
                field.focus();
                field.select_all();
                field.bring_into_view();
            }
            value
        };
        let Some(small) = parse(&form.small) else {
            return false;
        };
        let Some(big) = parse(&form.big) else {
            return false;
        };
        let amounts = platform::NudgeAmounts { small, big };
        drop(draft);
        self.canvas.nudge.set(amounts);
        let saved = amounts.save();
        self.close_modal();
        if !saved {
            self.toast("Nudge amounts applied for this session. Preferences could not be saved.");
        }
        true
    }
}
