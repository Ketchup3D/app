//! Shared entity menu and commands for the app bar, viewport, and Outliner.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum ContextAction {
    Rename,
    EditText,
    EditObject,
    CloseContext,
    Group,
    Component,
    Ungroup,
    Explode,
    Cut,
    Copy,
    Paste,
    PasteInPlace,
    Delete,
    Hide,
    Unhide,
    Lock,
    Unlock,
}

impl KetchupApp {
    pub(super) fn select_context_target(&mut self, id: Option<u64>) {
        if !id.is_some_and(|id| self.selected.contains(&id)) {
            self.selected = id.into_iter().collect();
        }
    }

    pub(super) fn context_action(&mut self, ctx: &egui::Context, action: ContextAction) {
        use ContextAction::*;
        let locked = self.doc.model.selection_protected(&self.selected);
        if !matches!(action, ContextAction::Copy) {
            self.last_move_copy = None;
            self.length_input.clear();
            self.measurement = None;
            self.drawing = None;
            self.modeling = None;
            self.gesture = None;
            self.line_start = None;
            self.snap = None;
        }
        match action {
            Rename if !locked && self.selected.len() == 1 => {
                let id = *self.selected.first().unwrap();
                if let Some(object) = self.doc.model.objects.get(&id) {
                    self.object_rename = Some((id, object.name.clone()));
                    Self::request_name_input_focus(ctx, egui::Id::unique("rename_object_name"));
                }
            }
            EditText if !locked && self.selected.len() == 1 => {
                self.begin_text_edit(*self.selected.first().unwrap(), ctx);
            }
            EditObject => {
                if self.selected.len() == 1 {
                    self.enter_object(*self.selected.first().unwrap(), ctx);
                }
            }
            CloseContext => self.close_edit_context(ctx),
            Copy => self.copy_selection(ctx),
            Cut if !locked => {
                self.copy_selection(ctx);
                self.erase_selected(ctx);
            }
            Delete if !locked => self.erase_selected(ctx),
            Group => self.begin_object_creation(ctx, false),
            Component => self.begin_object_creation(ctx, true),
            Paste | PasteInPlace => {
                let Some(clipboard) = self.clipboard.clone() else {
                    return;
                };
                self.set_tool(Tool::Select);
                if matches!(action, PasteInPlace) {
                    self.paste_at(ctx, &clipboard, DVec3::ZERO);
                } else {
                    let pos = self
                        .context_position
                        .take()
                        .or_else(|| ctx.input(|i| i.pointer.hover_pos()))
                        .filter(|p| self.viewport.contains(*p))
                        .unwrap_or(self.viewport.center());
                    self.prepare_picking();
                    let point = self
                        .infer_snap(pos, None, None, self.doc.snap_to_axes)
                        .map(|s| s.point)
                        .unwrap_or(clipboard.anchor);
                    self.paste_preview = Some(PastePreview { clipboard, point });
                    self.say(ctx, "Click to place the copy · Escape to cancel");
                }
            }
            Ungroup | Explode if !locked => {
                let before = self.doc.model.clone();
                self.selected = self
                    .doc
                    .model
                    .dissolve_objects(&self.selected, matches!(action, Ungroup));
                self.finish_edit(
                    if matches!(action, Ungroup) {
                        "Ungroup"
                    } else {
                        "Explode"
                    },
                    before,
                    ctx,
                );
            }
            Hide | Lock
                if !locked
                    || (matches!(action, Lock)
                        && !self.doc.model.selection_locked(&self.selected)) =>
            {
                let before = self.doc.model.clone();
                if matches!(action, Hide) {
                    self.doc.model.hidden.extend(&self.selected);
                    self.selected.clear();
                } else {
                    self.doc.model.locked.extend(&self.selected);
                }
                self.finish_context_state_edit(
                    if matches!(action, Hide) {
                        "Hide"
                    } else {
                        "Lock"
                    },
                    before,
                    ctx,
                );
            }
            Unlock => {
                let before = self.doc.model.clone();
                let ids = self.doc.model.expand_entities(&self.selected);
                self.doc.model.locked.retain(|id| !ids.contains(id));
                self.finish_context_state_edit("Unlock", before, ctx);
            }
            Unhide => {
                let before = self.doc.model.clone();
                self.doc
                    .model
                    .hidden
                    .retain(|id| !self.selected.contains(id));
                self.finish_context_state_edit("Unhide", before, ctx);
            }
            _ => {}
        }
    }
    pub(super) fn context_menu(&mut self, response: &egui::Response) {
        self.context_menu_with(response, |_, _| {});
    }

    pub(super) fn context_menu_with(
        &mut self,
        response: &egui::Response,
        extra: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
        response.context_menu(|ui| self.entity_menu(ui, extra));
    }

    pub(super) fn menu_style(style: &mut egui::Style) {
        egui::menu::menu_style(style);
        style.spacing.item_spacing.y = 2.;
        style.spacing.interact_size.y = 24.;
        style.spacing.button_padding = egui::vec2(8., 3.);
    }

    pub(super) fn menu_checkbox(
        ui: &mut egui::Ui,
        checked: &mut bool,
        label: &str,
    ) -> egui::Response {
        // Reserve the background before painting the checkbox and its label.
        let background = ui.painter().add(egui::Shape::Noop);
        let response = ui
            .allocate_ui_with_layout(
                egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                egui::Layout::top_down_justified(egui::Align::Min),
                |ui| ui.checkbox(checked, label),
            )
            .inner;
        if response.hovered() || response.has_focus() || response.is_pointer_button_down_on() {
            let visuals = ui.style().interact(&response);
            ui.painter().set(
                background,
                egui::Shape::rect_filled(
                    response.rect,
                    visuals.corner_radius,
                    visuals.weak_bg_fill,
                ),
            );
        }
        response
    }

    pub(super) fn configure_entity_menu(ui: &mut egui::Ui) {
        ui.set_min_width(230.);
        Self::menu_style(ui.style_mut());
    }

    pub(super) fn entity_menu(
        &mut self,
        ui: &mut egui::Ui,
        extra: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
        let mut action = None;
        Self::configure_entity_menu(ui);
        let selected = !self.selected.is_empty();
        let editable = selected && !self.doc.model.selection_protected(&self.selected);
        let objects = self
            .selected
            .iter()
            .any(|id| self.doc.model.objects.contains_key(id));
        let groups = self.selected.iter().any(|id| {
            self.doc
                .model
                .objects
                .get(id)
                .is_some_and(|o| o.kind == ObjectKind::Group)
        });
        let mut item = |ui: &mut egui::Ui, label: &str, enabled, command| {
            let shortcut = match command {
                ContextAction::Cut => Some((egui::Modifiers::COMMAND, Key::X)),
                ContextAction::Copy => Some((egui::Modifiers::COMMAND, Key::C)),
                ContextAction::Paste => Some((egui::Modifiers::COMMAND, Key::V)),
                ContextAction::PasteInPlace => {
                    Some((egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::V))
                }
                ContextAction::Delete => Some((egui::Modifiers::NONE, Key::Delete)),
                _ => None,
            };
            let mut button = egui::Button::new(label);
            if let Some((modifiers, key)) = shortcut {
                button = button.shortcut_text(
                    ui.ctx()
                        .format_shortcut(&egui::KeyboardShortcut::new(modifiers, key)),
                );
            }
            if ui.add_enabled(enabled, button).clicked() {
                action = Some(command);
                ui.close();
            }
        };
        item(
            ui,
            "Rename…",
            editable && self.selected.len() == 1 && objects,
            ContextAction::Rename,
        );
        if self.selected.len() == 1
            && self
                .selected
                .first()
                .is_some_and(|id| self.doc.model.texts.contains_key(id))
        {
            item(ui, "Edit Text…", editable, ContextAction::EditText);
        }
        if self.selected.len() == 1 && objects {
            let object = &self.doc.model.objects[self.selected.first().unwrap()];
            let label = if object.kind == ObjectKind::Group {
                "Edit Group"
            } else {
                "Edit Component"
            };
            item(
                ui,
                label,
                !self.doc.model.is_locked(object.id) && !self.doc.model.is_hidden(object.id),
                ContextAction::EditObject,
            );
            ui.separator();
        }
        if self.doc.model.edit_context.is_some() {
            item(
                ui,
                "Close Editing Context",
                true,
                ContextAction::CloseContext,
            );
            ui.separator();
        }
        item(ui, "Make Group…", editable, ContextAction::Group);
        item(ui, "Create Component…", editable, ContextAction::Component);
        item(ui, "Ungroup", editable && groups, ContextAction::Ungroup);
        item(ui, "Explode", editable && objects, ContextAction::Explode);
        ui.separator();
        item(ui, "Cut", editable, ContextAction::Cut);
        item(ui, "Copy", selected, ContextAction::Copy);
        item(ui, "Paste", self.clipboard.is_some(), ContextAction::Paste);
        item(
            ui,
            "Paste in Place",
            self.clipboard.is_some(),
            ContextAction::PasteInPlace,
        );
        item(ui, "Delete", editable, ContextAction::Delete);
        ui.separator();
        if self
            .selected
            .iter()
            .any(|id| self.doc.model.hidden.contains(id))
        {
            item(ui, "Unhide", selected, ContextAction::Unhide);
        } else {
            item(ui, "Hide", editable, ContextAction::Hide);
        }
        if self.doc.model.selection_locked(&self.selected) {
            item(ui, "Unlock", selected, ContextAction::Unlock);
        } else {
            item(ui, "Lock", selected, ContextAction::Lock);
        }
        extra(self, ui);
        if let Some(action) = action {
            self.context_action(ui.ctx(), action);
            self.context_position = None;
        }
    }

    pub(super) fn rename_object_window(&mut self, ctx: &egui::Context) {
        let Some((id, mut name)) = self.object_rename.take() else {
            return;
        };
        if !self.doc.model.objects.contains_key(&id) {
            return;
        }
        let mut save = false;
        let mut cancel = false;
        let response = Modal::new("rename_object", 360.).show(ctx, |modal| {
            modal.header(|ui| {
                ui.heading("Rename");
                ui.weak("Choose a name for this object.");
            });
            modal.body(|ui| {
                ui.label("Name");
                let input = Self::name_input(ui, &mut name, egui::Id::unique("rename_object_name"));
                if name.len() > 1024 {
                    name.truncate(name.floor_char_boundary(1024));
                }
                save = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            });
            modal.footer(|ui| {
                cancel = ui.button("Cancel").clicked();
                let valid = !name.trim().is_empty();
                save = (primary_button(ui, "Save", valid).clicked() || save) && valid;
            });
        });
        if save {
            self.rename_object(id, &name, ctx);
        } else if !cancel && !response.should_close() {
            self.object_rename = Some((id, name));
        }
    }

    fn rename_object(&mut self, id: u64, name: &str, ctx: &egui::Context) {
        let name = name.trim();
        if name.is_empty()
            || name.len() > 1024
            || self.doc.model.selection_protected(&BTreeSet::from([id]))
            || !self.doc.model.objects.contains_key(&id)
        {
            return;
        }
        let before = self.doc.model.clone();
        self.doc.model.objects.get_mut(&id).unwrap().name = name.to_owned();
        self.finish_context_state_edit("Rename", before, ctx);
    }
}
