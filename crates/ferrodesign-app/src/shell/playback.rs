//! Keyboard interaction between the prototype surface and the native player bar.
use super::*;
use ferroui_base::input::{KeyModifiers, NavigationMethod};
use ferroui_base::interactivity::RoutingStrategies;

const CONTROLS: [&str; 4] = [
    "PreviewBack",
    "PreviewNext",
    "PreviewRestart",
    "PreviewExit",
];

impl EditorShell {
    pub(super) fn present_preview(self: &Rc<Self>) -> bool {
        let root = {
            let editor = self.editor.borrow();
            let document = &editor.document;
            let page = editor.active_page;
            let target = editor
                .selection
                .iter()
                .copied()
                .find(|id| document.page_of(*id) == Some(page) && document.effective_visible(*id))
                .or_else(|| core::PrototypePlayer::page_start(document, page))
                // Existing documents may consist solely of loose artwork.
                .or_else(|| {
                    document
                        .page(page)?
                        .children
                        .iter()
                        .copied()
                        .find(|id| document.effective_visible(*id))
                });
            target.map(|mut id| {
                while let Some(parent) = document.nodes.get(&id).and_then(|node| node.parent) {
                    if parent == page {
                        break;
                    }
                    id = parent;
                }
                id
            })
        };
        if root.is_none() {
            self.toast("Add a visible frame or layer to preview this page.");
            return false;
        }
        self.set_presentation(root)
    }

    pub(super) fn preview_canvas_key(self: &Rc<Self>, key: &str) {
        if self.prototype.key(key) {
            return;
        }
        match key {
            "Left" => {
                self.prototype.step(false);
            }
            "Right" | "N" | "Space" => {
                self.prototype.step(true);
            }
            "R" => {
                self.prototype.restart();
            }
            "Escape" if self.prototype.has_overlay() => {
                self.prototype.step(false);
            }
            "Escape" => {
                self.prototype.stop();
            }
            "Tab" | "F6" => self.focus_playback_control(None, false),
            "Shift+Tab" | "Shift+F6" => self.focus_playback_control(None, true),
            _ => {}
        }
    }

    fn focus_playback_control(&self, from: Option<usize>, reverse: bool) {
        let indices = 0..CONTROLS.len();
        let target = if reverse {
            indices
                .take(from.unwrap_or(CONTROLS.len()))
                .rev()
                .find(|i| self.button(CONTROLS[*i]).is_enabled())
        } else {
            indices
                .skip(from.map_or(0, |index| index + 1))
                .find(|i| self.button(CONTROLS[*i]).is_enabled())
        };
        if let Some(index) = target {
            self.button(CONTROLS[index])
                .focus_with(NavigationMethod::Tab, KeyModifiers::NONE);
        } else {
            self.canvas
                .focus_with(NavigationMethod::Tab, KeyModifiers::NONE);
        }
    }

    pub(super) fn wire_playback_focus(self: &Rc<Self>) {
        for (index, name) in CONTROLS.iter().enumerate() {
            let weak = Rc::downgrade(self);
            self.button(name).add_handler_with(
                InputElement::key_down_event(),
                move |_, event| {
                    let Some(shell) = weak.upgrade() else { return };
                    let modifiers = event.key_modifiers;
                    match event.key {
                        Key::Tab if (modifiers - KeyModifiers::SHIFT).is_empty() => shell
                            .focus_playback_control(
                                Some(index),
                                modifiers.contains(KeyModifiers::SHIFT),
                            ),
                        Key::F6 if (modifiers - KeyModifiers::SHIFT).is_empty() => {
                            shell.canvas.focus();
                        }
                        Key::Escape if modifiers.is_empty() => {
                            if shell.prototype.has_overlay() {
                                shell.command("prototype-back");
                            } else {
                                shell.command("prototype-exit");
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
    }
}
