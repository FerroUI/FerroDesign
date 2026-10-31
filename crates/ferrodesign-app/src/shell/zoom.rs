//! Camera controls share the retained viewport with wheel and hand input.
use super::*;
use ferroui_base::interactivity::RoutingStrategies;

impl EditorShell {
    pub(super) fn sync_zoom(&self) {
        let percent = (self.canvas.viewport.get().zoom * 100.0).round() as i32;
        // A hand drag can report hundreds of camera positions without changing
        // this label. Avoid content allocation and measure on those events.
        if self.displayed_zoom.replace(Some(percent)) != Some(percent) {
            self.button("ZoomValueButton")
                .set_content(Some(Rc::new(format!("{percent}%"))));
        }
    }

    pub(super) fn wire_zoom(self: &Rc<Self>) {
        let owner = self.button("ZoomValueButton");
        let flyout = owner.flyout().expect("ZoomValueButton flyout");
        let weak = Rc::downgrade(self);
        owner.click(move |_, _| {
            if let Some(shell) = weak.upgrade() {
                let field = shell.field("ZoomPercentField");
                let percent = number(shell.canvas.viewport.get().zoom * 100.0);
                field.set_text(Some(&percent));
                field.focus();
                field.select_all();
            }
        });
        for (name, scale) in [("Zoom50", 0.5), ("Zoom100", 1.0), ("Zoom200", 2.0)] {
            let weak = Rc::downgrade(self);
            let flyout = flyout.downgrade();
            self.button(name).click(move |_, _| {
                if let (Some(shell), Some(flyout)) = (weak.upgrade(), flyout.upgrade()) {
                    flyout.hide();
                    shell.canvas.set_zoom(scale);
                    shell.canvas.focus();
                }
            });
        }
        for (name, command) in [("ZoomFitPage", "fit"), ("ZoomFitSelection", "fit-selection")] {
            let weak = Rc::downgrade(self);
            let flyout = flyout.downgrade();
            self.button(name).click(move |_, _| {
                if let (Some(shell), Some(flyout)) = (weak.upgrade(), flyout.upgrade()) {
                    flyout.hide();
                    shell.command(command);
                }
            });
        }
        let weak = Rc::downgrade(self);
        let flyout = flyout.downgrade();
        self.field("ZoomPercentField").add_handler_with(
            InputElement::key_down_event(),
            move |_, event| {
                if !matches!(event.key, Key::Enter | Key::Escape) { return; }
                let (Some(shell), Some(flyout)) = (weak.upgrade(), flyout.upgrade()) else { return; };
                event.set_handled(true);
                if event.key == Key::Enter {
                    let field = shell.field("ZoomPercentField");
                    let Some(scale) = parse_zoom(&field.text().unwrap_or_default()) else {
                        field.select_all();
                        return;
                    };
                    shell.canvas.set_zoom(scale);
                }
                flyout.hide();
                shell.canvas.focus();
            },
            RoutingStrategies::TUNNEL,
            false,
        );
    }
}

fn parse_zoom(input: &str) -> Option<f64> {
    let percent = input.trim().trim_end_matches('%').trim_end().parse::<f64>().ok()?;
    let scale = percent / 100.0;
    (core::Viewport::MIN_ZOOM..=core::Viewport::MAX_ZOOM).contains(&scale).then_some(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_input_accepts_fractional_percentages_with_the_viewport_bounds() {
        assert_eq!(parse_zoom(" 37.5 % "), Some(0.375));
        assert_eq!(parse_zoom("2"), Some(core::Viewport::MIN_ZOOM));
        assert_eq!(parse_zoom("6400"), Some(core::Viewport::MAX_ZOOM));
        for invalid in ["", "0", "1.99%", "6401", "NaN", "inf", "12px"] {
            assert_eq!(parse_zoom(invalid), None);
        }
    }
}
