//! Shared entity context menu and commands for the viewport and Outliner.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum ContextAction {
    Rename,
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
        let mut action = None;
        response.context_menu(|ui| {
            ui.set_min_width(230.);
            ui.spacing_mut().item_spacing.y = 2.;
            ui.spacing_mut().interact_size.y = 24.;
            ui.spacing_mut().button_padding = egui::vec2(8., 3.);
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
        });
        if let Some(action) = action {
            self.context_action(&response.ctx, action);
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
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Rename")
            .id(egui::Id::new("rename_object"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Name");
                let input = Self::name_input(ui, &mut name, egui::Id::unique("rename_object_name"));
                if name.len() > 1024 {
                    name.truncate(name.floor_char_boundary(1024));
                }
                let valid = !name.trim().is_empty();
                save = valid && input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.add_space(10.);
                ui.horizontal(|ui| {
                    save |= ui.add_enabled(valid, egui::Button::new("Save")).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if save {
            self.rename_object(id, &name, ctx);
        } else if open && !cancel {
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
