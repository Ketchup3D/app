use eframe::egui::{self, Key};

pub(super) fn valid_tool_key(name: &str) -> bool {
    name == "Space" || (name.len() == 1 && name.as_bytes()[0].is_ascii_uppercase())
}

/// A focused key recorder. Listening is transient; the caller persists only a
/// completed assignment. Key events are consumed before the button or modal
/// can interpret them as activation or dismissal.
pub(super) fn shortcut_input(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
    width: f32,
) -> Option<String> {
    ui.push_id(id, |ui| {
        let state_id = ui.make_persistent_id("listening");
        let listening_button = ui
            .ctx()
            .data_mut(|data| data.get_temp::<egui::Id>(state_id));
        let mut listening =
            listening_button.is_some_and(|id| ui.memory(|memory| memory.has_focus(id)));
        let mut assigned = None;
        let mut finished = false;
        if listening {
            ui.input_mut(|input| {
                input.events.retain(|event| {
                    if let egui::Event::Key {
                        key,
                        pressed,
                        repeat,
                        modifiers,
                        ..
                    } = event
                    {
                        if *pressed && !*repeat && !finished {
                            if *key == Key::Escape {
                                finished = true;
                            } else if modifiers.is_none() && valid_tool_key(key.name()) {
                                assigned = Some(key.name().to_owned());
                                finished = true;
                            }
                        }
                        false
                    } else {
                        true
                    }
                });
            });
        }
        let label = if listening && !finished {
            "Press a key…"
        } else {
            current
        };
        let response = ui
            .add_sized(
                [width, 32.],
                egui::Button::new(egui::RichText::new(label).monospace())
                    .selected(listening && !finished),
            )
            .on_hover_text("Click, then press a letter or Space. Escape cancels.");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Button,
                ui.is_enabled(),
                format!(
                    "Shortcut {current}. {}",
                    if listening {
                        "Press a letter or Space; Escape cancels"
                    } else {
                        "Click to change"
                    }
                ),
            )
        });
        if finished {
            listening = false;
            response.surrender_focus();
        } else if response.clicked() {
            listening = true;
            response.request_focus();
        } else if !response.has_focus() {
            listening = false;
        }
        ui.ctx().data_mut(|data| {
            if listening {
                data.insert_temp(state_id, response.id);
            } else {
                data.remove::<egui::Id>(state_id);
            }
        });
        assigned
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> (Option<String>, egui::Pos2, bool) {
        let mut assigned = None;
        let mut center = egui::Pos2::ZERO;
        let mut escape_left = false;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400., 240.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                center = ui.next_widget_position() + egui::vec2(60., 16.);
                assigned = shortcut_input(ui, "test_key", "L", 144.);
                escape_left = ui.input(|input| input.key_pressed(Key::Escape));
            },
        );
        (assigned, center, escape_left)
    }

    fn click(ctx: &egui::Context, pos: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn records_letters_and_space_and_consumes_escape_without_assignment() {
        for (pressed_key, expected) in [
            (Key::R, Some("R")),
            (Key::Space, Some("Space")),
            (Key::Escape, None),
        ] {
            let ctx = egui::Context::default();
            let (_, center, _) = frame(&ctx, vec![]);
            click(&ctx, center);
            let (assigned, _, escape_left) = frame(&ctx, vec![key(pressed_key)]);
            assert_eq!(assigned.as_deref(), expected);
            assert!(
                !escape_left,
                "Escape cancels capture before the modal sees it"
            );
        }
    }

    #[test]
    fn unfocused_controls_do_not_capture_keys() {
        let ctx = egui::Context::default();
        let (assigned, _, _) = frame(&ctx, vec![key(Key::R)]);
        assert!(assigned.is_none());
    }
}
