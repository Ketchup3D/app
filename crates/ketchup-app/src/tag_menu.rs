//! Compact tag management for the viewport toolbar.
use super::*;
use crate::inspector_table::{Column, InspectorTable};

impl KetchupApp {
    fn set_selection_tag_membership(
        &mut self,
        ctx: &egui::Context,
        tag: u64,
        assigned: bool,
    ) -> Result<(), &'static str> {
        let targets = if assigned {
            self.selected.clone()
        } else {
            self.selected
                .iter()
                .copied()
                .filter(|id| self.doc.model.entity_tags.get(id) == Some(&tag))
                .collect()
        };
        let before = self.doc.model.clone();
        self.doc
            .model
            .assign_tag(&targets, if assigned { Some(tag) } else { None })?;
        self.finish_context_state_edit(
            if assigned {
                "Assign tag"
            } else {
                "Unassign tag"
            },
            before,
            ctx,
        );
        Ok(())
    }

    pub(super) fn toolbar_tags_contents(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let mut before = None;
        let mut action = "Edit tags";
        ui.text_strong("Tags");
        ui.add_space(4.);
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 52. - ui.spacing().item_spacing.x).max(40.);
            let input = ui.add_sized(
                [width, 28.],
                egui::TextEdit::singleline(&mut self.new_tag_name)
                    .hint_text("New tag name")
                    .align(Align2::LEFT_CENTER)
                    .char_limit(128),
            );
            let enter = input.has_focus()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
            if ui
                .add_enabled(
                    !self.new_tag_name.trim().is_empty(),
                    egui::Button::new("Add")
                        .min_size(egui::vec2(52., 28.))
                        .stroke(Stroke::new(1., colors.accent)),
                )
                .clicked()
                || enter
            {
                if !self.new_tag_name.trim().is_empty() {
                    before = Some(self.doc.model.clone());
                    match self.doc.model.add_tag(&self.new_tag_name) {
                        Ok(_) => {
                            self.new_tag_name.clear();
                            action = "Add tag";
                        }
                        Err(error) => self.say(ui.ctx(), error),
                    }
                }
            }
        });
        ui.add_space(6.);
        let mut rename = self.tag_rename.take();
        let mut start_rename = None;
        let mut save = false;
        let mut cancel = false;
        let mut delete = None;
        let tags: Vec<_> = self.doc.model.tags.values().cloned().collect();
        let color_editor_id = ui.make_persistent_id("tag_color_editor");
        let mut editing_color = ui.data_mut(|data| data.get_temp::<u64>(color_editor_id));
        if editing_color.is_some_and(|id| !self.doc.model.tags.contains_key(&id)) {
            editing_color = None;
        }
        let can_assign =
            !self.selected.is_empty() && !self.doc.model.selection_protected(&self.selected);
        let scroll = sidebar::restored_scroll_area(
            ui,
            "toolbar_tags/results",
            egui::ScrollArea::vertical()
                .id_salt("toolbar_tags_results")
                .max_height(if editing_color.is_some() {
                    120.
                } else {
                    (ui.ctx().content_rect().height() * 0.45).clamp(100., 320.)
                }),
        );
        let output = scroll.show(ui, |ui| {
            let mut table = InspectorTable::with_column_spacing(
                ui.available_width(),
                [
                    Column::fixed(26.),
                    Column::fixed(28.),
                    Column::flex(1.),
                    Column::fixed(28.),
                    Column::fixed(28.),
                ],
                6.,
            )
            .striped();
            table.header(ui, ["", "", "Name", "", ""]);
            table.row(ui, "untagged", |column, cell| {
                if column == 2 {
                    cell.text_label(RichText::new("Untagged").color(colors.muted));
                }
            });
            for tag in &tags {
                let editing = rename.as_ref().is_some_and(|(id, _)| *id == tag.id);
                table.row(ui, tag.id, |column, cell| match column {
                    0 => {
                        let count = self
                            .selected
                            .iter()
                            .filter(|id| self.doc.model.entity_tags.get(id) == Some(&tag.id))
                            .count();
                        let mut assigned =
                            !self.selected.is_empty() && count == self.selected.len();
                        let mixed = count > 0 && count < self.selected.len();
                        if cell
                            .add_enabled(
                                can_assign,
                                egui::Checkbox::new(&mut assigned, "").indeterminate(mixed),
                            )
                            .accessible_name(format!("Assign: {}", tag.name))
                            .on_hover_text("Assign this tag to the selection; untick to remove it")
                            .changed()
                        {
                            if let Err(error) =
                                self.set_selection_tag_membership(cell.ctx(), tag.id, assigned)
                            {
                                self.say(cell.ctx(), error);
                            }
                        }
                    }
                    1 => {
                        let active = editing_color == Some(tag.id);
                        let response = cell
                            .add(
                                egui::Button::new("")
                                    .min_size(egui::vec2(24., 24.))
                                    .fill(Color32::from_rgb(
                                        tag.color[0],
                                        tag.color[1],
                                        tag.color[2],
                                    ))
                                    .stroke(Stroke::new(
                                        if active { 2. } else { 1. },
                                        if active { colors.accent } else { colors.muted },
                                    )),
                            )
                            .accessible_name(format!("Color: {}", tag.name))
                            .on_hover_text("Click to edit the tag color");
                        if response.clicked() {
                            editing_color = if active { None } else { Some(tag.id) };
                        }
                    }
                    2 => {
                        if editing {
                            let name = &mut rename.as_mut().unwrap().1;
                            let input =
                                Self::name_input(cell, name, egui::Id::unique("rename_tag_name"));
                            save |= input.has_focus()
                                && cell.input_mut(|i| {
                                    i.consume_key(egui::Modifiers::NONE, Key::Enter)
                                });
                            cancel |= input.has_focus()
                                && cell.input_mut(|i| {
                                    i.consume_key(egui::Modifiers::NONE, Key::Escape)
                                });
                        } else if cell
                            .add(crate::natural_text::Label::new(&tag.name).truncate())
                            .on_hover_text(&tag.name)
                            .double_clicked()
                        {
                            start_rename = Some((tag.id, tag.name.clone()));
                        }
                    }
                    3 => {
                        if editing {
                            save |= cell.button("✓").on_hover_text("Save name").clicked();
                        } else {
                            let response = cell
                                .add(egui::Button::new("").min_size(egui::vec2(26., 26.)))
                                .accessible_name(format!("Rename: {}", tag.name))
                                .on_hover_text("Rename tag");
                            icons::ui_tool(cell.painter(), response.rect.shrink(4.), Tool::Line);
                            if response.clicked() {
                                start_rename = Some((tag.id, tag.name.clone()));
                            }
                        }
                    }
                    _ => {
                        if editing {
                            cancel |= cell.button("×").on_hover_text("Cancel renaming").clicked();
                        } else if cell
                            .button("×")
                            .accessible_name(format!("Delete: {}", tag.name))
                            .on_hover_text("Delete tag; its entities become Untagged")
                            .clicked()
                        {
                            delete = Some(tag.id);
                        }
                    }
                });
            }
        });
        sidebar::remember_scroll(
            ui.ctx(),
            "toolbar_tags/results",
            output.state.clamped_offset(),
        );
        if let Some(id) = editing_color {
            let tag = self.doc.model.tags.get(&id).cloned().unwrap();
            ui.separator();
            ui.horizontal(|ui| {
                ui.text_strong(format!("Color · {}", tag.name));
                if ui
                    .small_button("×")
                    .on_hover_text("Close color editor")
                    .clicked()
                {
                    editing_color = None;
                }
            });
            let mut color = Color32::from_rgb(tag.color[0], tag.color[1], tag.color[2]);
            ui.scope(|ui| {
                ui.spacing_mut().slider_width = (ui.available_width() - 24.).clamp(80., 288.);
                if egui::color_picker::color_picker_color32(
                    ui,
                    &mut color,
                    egui::color_picker::Alpha::Opaque,
                ) {
                    before.get_or_insert_with(|| self.doc.model.clone());
                    self.doc.model.tags.get_mut(&id).unwrap().color =
                        [color.r(), color.g(), color.b()];
                    action = "Tag color";
                }
            });
        }
        ui.data_mut(|data| {
            if let Some(id) = editing_color {
                data.insert_temp(color_editor_id, id);
            } else {
                data.remove::<u64>(color_editor_id);
            }
        });
        if let Some((id, name)) = start_rename {
            rename = Some((id, name));
            Self::request_name_input_focus(ui.ctx(), egui::Id::unique("rename_tag_name"));
        }
        if save && let Some((id, name)) = &rename {
            before.get_or_insert_with(|| self.doc.model.clone());
            match self.doc.model.rename_tag(*id, name) {
                Ok(()) => {
                    rename = None;
                    action = "Rename tag";
                }
                Err(error) => self.say(ui.ctx(), error),
            }
        }
        if cancel {
            rename = None;
        }
        if let Some(id) = delete {
            before.get_or_insert_with(|| self.doc.model.clone());
            self.doc.model.remove_tag(id);
            action = "Delete tag";
        }
        self.tag_rename = rename;
        ui.separator();
        if self.selected.is_empty() {
            ui.text_weak("Select objects or geometry to assign a tag.");
        } else {
            ui.text_small("Apply to selection");
            let first = self
                .selected
                .first()
                .and_then(|id| self.doc.model.entity_tags.get(id))
                .copied();
            let mixed = self
                .selected
                .iter()
                .any(|id| self.doc.model.entity_tags.get(id).copied() != first);
            let current = if mixed {
                "Mixed tags"
            } else {
                first
                    .and_then(|id| self.doc.model.tags.get(&id))
                    .map_or("Untagged", |tag| tag.name.as_str())
            };
            let mut assign = None;
            ui.add_enabled_ui(!self.doc.model.selection_protected(&self.selected), |ui| {
                egui::ComboBox::from_id_salt("toolbar_selection_tag")
                    .width(ui.available_width())
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(!mixed && first.is_none(), "Untagged")
                            .clicked()
                        {
                            assign = Some(None);
                        }
                        for tag in &tags {
                            if ui
                                .selectable_label(!mixed && first == Some(tag.id), &tag.name)
                                .clicked()
                            {
                                assign = Some(Some(tag.id));
                            }
                        }
                    });
            });
            if let Some(tag) = assign {
                before.get_or_insert_with(|| self.doc.model.clone());
                if let Err(error) = self.doc.model.assign_tag(&self.selected, tag) {
                    self.say(ui.ctx(), error);
                }
                action = "Assign tag";
            }
        }
        if let Some(before) = before {
            self.finish_context_state_edit(action, before, ui.ctx());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_ui::ContextExt as _;

    #[test]
    fn tag_checkboxes_assign_and_remove_without_hiding_or_touching_other_tags() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let a = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        let b = app
            .doc
            .model
            .add_edge(DVec3::Y, DVec3::Y + DVec3::X)
            .unwrap();
        let first = app.doc.model.add_tag("First").unwrap();
        let other = app.doc.model.add_tag("Other").unwrap();
        app.doc
            .model
            .assign_tag(&BTreeSet::from([a]), Some(first))
            .unwrap();
        app.doc
            .model
            .assign_tag(&BTreeSet::from([b]), Some(other))
            .unwrap();
        app.selected = BTreeSet::from([a, b]);
        let before = app.doc.model.clone();
        app.set_selection_tag_membership(&ctx, first, false)
            .unwrap();
        assert!(!app.doc.model.entity_tags.contains_key(&a));
        assert_eq!(app.doc.model.entity_tags.get(&b), Some(&other));
        assert!(app.doc.model.tags[&first].visible);
        assert!(app.doc.model.hidden.is_empty());
        app.undo(&ctx, false);
        assert_eq!(app.doc.model, before);
        app.set_selection_tag_membership(&ctx, first, true).unwrap();
        assert_eq!(app.doc.model.entity_tags.get(&a), Some(&first));
        assert_eq!(app.doc.model.entity_tags.get(&b), Some(&first));
    }

    #[test]
    fn clicking_a_tag_swatch_opens_an_inline_color_editor() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.doc.model.add_tag("Sample").unwrap();
        let mut swatch = None;
        let mut labels = Vec::new();
        for frame in 0..4 {
            let output = ctx.run_test_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600., 800.))),
                    events: swatch.map_or_else(Vec::new, |pos| {
                        vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed: frame == 2,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ]
                    }),
                    ..Default::default()
                },
                |ui| {
                    ui.set_width(340.);
                    app.toolbar_tags_contents(ui);
                },
            );
            for shape in &output.shapes {
                if let egui::Shape::Rect(rect) = &shape.shape {
                    let c = app.doc.model.tags.values().next().unwrap().color;
                    if rect.fill == Color32::from_rgb(c[0], c[1], c[2]) && rect.rect.width() < 40. {
                        swatch = Some(rect.rect.center());
                    }
                }
                if let egui::Shape::Text(text) = &shape.shape {
                    labels.push(text.galley.job.text.clone());
                }
            }
        }
        assert!(labels.iter().any(|text| text == "Color · Sample"));
    }
}
