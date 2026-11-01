//! Shared searchable dropdown with a fixed search row and persisted results scroll.
use super::sidebar;
use eframe::egui;

pub(super) struct Choice<T> {
    pub value: T,
    pub label: String,
    pub search_text: String,
    pub tooltip: Option<String>,
    /// Keep clearing/unassigning available even when the query has no matches.
    pub always_visible: bool,
}

impl<T> Choice<T> {
    pub fn new(value: T, label: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            value,
            search_text: label.clone(),
            label,
            tooltip: None,
            always_visible: false,
        }
    }
}

pub(super) fn matches_query(candidate: &str, query: &str) -> bool {
    let candidate = candidate.to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| candidate.contains(word))
}

pub(super) struct SearchableSelector<'a> {
    pub id_salt: egui::IdSalt,
    pub scroll_key: &'a str,
    pub selected_label: &'a str,
    pub width: f32,
    pub popup_width: f32,
    pub hint: &'a str,
    pub empty_text: &'a str,
}

impl SearchableSelector<'_> {
    /// `inner` reports an explicit selection, including selecting the current value.
    pub fn show<T: Clone + PartialEq>(
        self,
        ui: &mut egui::Ui,
        selected: &mut T,
        filter: &mut String,
        choices: Vec<Choice<T>>,
    ) -> egui::InnerResponse<bool> {
        let was_open = egui::ComboBox::is_open(ui.ctx(), ui.make_persistent_id(self.id_salt));
        let mut picked = false;
        let mut response = egui::ComboBox::from_id_salt(self.id_salt)
            .width(self.width)
            .selected_text(self.selected_label)
            .truncate()
            .height(f32::INFINITY)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                // A fixed width avoids a feedback loop as popup content is measured.
                let width = self.popup_width;
                let height = ui.spacing().interact_size.y.max(26.);
                let mut query_changed = false;
                let search = ui
                    .allocate_ui_with_layout(
                        egui::vec2(width, height),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let search_width = width - 48. - ui.spacing().item_spacing.x;
                            let search = ui.add_sized(
                                [search_width, height],
                                egui::TextEdit::singleline(filter)
                                    .id_salt((self.id_salt, "search"))
                                    .return_key(None)
                                    .hint_text(self.hint)
                                    .desired_width(search_width),
                            );
                            if ui
                                .add_enabled(
                                    !filter.is_empty(),
                                    egui::Button::new("Clear").min_size(egui::vec2(48., height)),
                                )
                                .clicked()
                            {
                                filter.clear();
                                query_changed = true;
                                search.request_focus();
                            }
                            search
                        },
                    )
                    .inner;
                if !was_open {
                    search.request_focus();
                }
                query_changed |= search.changed();
                ui.separator();
                let choices: Vec<_> = choices
                    .iter()
                    .filter(|choice| {
                        choice.always_visible || matches_query(&choice.search_text, filter)
                    })
                    .collect();
                let matches: Vec<_> = choices
                    .iter()
                    .filter(|choice| !choice.always_visible)
                    .collect();
                if search.has_focus()
                    && matches.len() == 1
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    })
                {
                    *selected = matches[0].value.clone();
                    picked = true;
                    ui.close();
                }
                let mut results = sidebar::restored_scroll_area(
                    ui,
                    self.scroll_key,
                    egui::ScrollArea::vertical()
                        .id_salt(self.scroll_key)
                        .max_height(ui.spacing().combo_height),
                );
                if query_changed {
                    results = results.vertical_scroll_offset(0.);
                }
                let output = results.show(ui, |ui| {
                    for (index, choice) in choices.iter().enumerate() {
                        ui.push_id(index, |ui| {
                            let mut response =
                                ui.selectable_value(selected, choice.value.clone(), &choice.label);
                            if let Some(tooltip) = &choice.tooltip {
                                response = response.on_hover_text(tooltip);
                            }
                            if response.clicked() {
                                picked = true;
                                ui.close();
                            }
                        });
                    }
                    if matches.is_empty() {
                        ui.weak(self.empty_text);
                    }
                });
                sidebar::remember_scroll(ui.ctx(), self.scroll_key, output.state.clamped_offset());
            })
            .response;
        if picked {
            response.mark_changed();
        }
        egui::InnerResponse {
            inner: picked,
            response,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_all_words_in_any_order_ignoring_case() {
        assert!(matches_query("CAD — Canadian dollar", "DOLLAR cad"));
        assert!(matches_query("Solid wood", "  wood   solid "));
        assert!(matches_query("Plywood", ""));
        assert!(!matches_query("Plywood", "wood solid"));
    }
}
