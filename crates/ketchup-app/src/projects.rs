use super::*;
use crate::project_files::{self, Outcome};
use std::{path::PathBuf, sync::mpsc::TryRecvError};

#[derive(Clone, Copy)]
pub(super) enum Action {
    New,
    Open,
    #[cfg(not(target_arch = "wasm32"))]
    RestartForUpdate,
}

#[derive(Clone, Copy, Default)]
pub(super) enum Operation {
    #[default]
    Open,
    Save,
    Import,
    Export,
}

#[derive(Default)]
pub(super) struct Project {
    pub path: Option<PathBuf>,
    pub modified: bool,
    pub pending: Option<project_files::Pending>,
    pub operation: Operation,
    pub snapshot: Option<String>,
    pub confirm: Option<Action>,
    pub after_save: Option<Action>,
    pub error: Option<String>,
}
impl Project {
    pub fn blocked(&self) -> bool {
        self.pending.is_some() || self.confirm.is_some() || self.error.is_some()
    }
}

impl KetchupApp {
    pub(super) fn replace_project(
        &mut self,
        mut doc: Document,
        path: Option<PathBuf>,
        ctx: &egui::Context,
    ) {
        if let Some(path) = &path {
            project_files::set_document_name(&mut doc, path);
        }
        self.set_tool(Tool::Select);
        egui::DragAndDrop::clear_payload(ctx);
        self.doc = doc;
        self.history = History::default();
        self.selected.clear();
        self.outliner_anchor = None;
        self.group_emphasis = true;
        self.hover = None;
        self.context_position = None;
        self.object_creation = None;
        self.object_rename = None;
        self.new_tag_name.clear();
        self.tag_rename = None;
        self.help = false;
        self.about = false;
        self.entity_cache = renderer::EntityCache::default();
        self.face_cache = renderer::FaceCache::default();
        self.picking_cache = crate::picking::PickingCache::default();
        self.paste_face_cache = renderer::FaceCache::default();
        self.changed(ctx);
        self.project.path = path;
        self.project.modified = false;
        self.project.confirm = None;
        self.project.after_save = None;
        self.project.error = None;
        self.save_error = None;
        self.storage_recovery = false;
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }

    pub(super) fn request_project(&mut self, action: Action, ctx: &egui::Context) {
        if self.project.blocked() {
            return;
        }
        if self.project.modified {
            self.project.confirm = Some(action);
        } else {
            self.perform_project(action, ctx);
        }
    }

    fn perform_project(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::New => self.new_model(ctx),
            Action::Open => {
                self.project.operation = Operation::Open;
                self.project.pending = Some(project_files::open(ctx));
            }
            #[cfg(not(target_arch = "wasm32"))]
            Action::RestartForUpdate => self.restart_for_update(ctx),
        }
    }

    pub(super) fn import_model(&mut self, ctx: &egui::Context) {
        if self.project.blocked() {
            return;
        }
        self.project.operation = Operation::Import;
        self.project.pending = Some(project_files::import(ctx, self.doc.model.clone()));
    }

    pub(super) fn export_model(&mut self, ctx: &egui::Context) {
        if self.project.blocked() {
            return;
        }
        match crate::interchange::export_obj(&self.doc.model) {
            Ok(contents) => {
                self.project.operation = Operation::Export;
                self.project.pending =
                    Some(project_files::export(ctx, self.doc.name.clone(), contents));
            }
            Err(error) => self.project.error = Some(error),
        }
    }

    pub(super) fn save_project(&mut self, ctx: &egui::Context, save_as: bool) {
        if self.project.blocked() {
            return;
        }
        let contents = match serde_json::to_string(&self.doc) {
            Ok(contents) => contents,
            Err(error) => {
                self.project.error = Some(format!("Could not save project: {error}"));
                self.project.after_save = None;
                return;
            }
        };
        let path = if save_as {
            None
        } else {
            self.project.path.clone()
        };
        self.project.snapshot = Some(contents.clone());
        self.project.operation = Operation::Save;
        self.project.pending = Some(project_files::save(
            ctx,
            path,
            self.doc.name.clone(),
            contents,
        ));
    }

    pub(super) fn poll_project(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.project.pending else {
            return;
        };
        let result = match pending.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("The file operation did not complete".into()),
        };
        self.project.pending = None;
        self.project.operation = Operation::Open;
        let snapshot = self.project.snapshot.take();
        match result {
            Ok(Outcome::Opened(doc, path)) => {
                self.replace_project(*doc, path, ctx);
                self.say(ctx, "Project opened");
            }
            Ok(Outcome::Saved(path)) => {
                let modified = serde_json::to_string(&self.doc).ok() != snapshot;
                if let Some(path) = &path {
                    project_files::set_document_name(&mut self.doc, path);
                    self.changed(ctx);
                }
                self.project.path = path;
                self.project.modified = modified;
                self.say(
                    ctx,
                    if cfg!(target_arch = "wasm32") {
                        "Project download started"
                    } else {
                        "Project saved"
                    },
                );
                if let Some(action) = self.project.after_save.take() {
                    if !self.project.modified {
                        self.perform_project(action, ctx);
                    }
                }
            }
            Ok(Outcome::Imported(model, selected)) => {
                let before = std::mem::replace(&mut self.doc.model, *model);
                self.set_tool(Tool::Select);
                self.selected = selected;
                self.finish_edit("Import SketchUp", before, ctx);
                self.fit(ctx);
                self.say(ctx, "SketchUp geometry imported as editable groups");
            }
            Ok(Outcome::Exported) => self.say(
                ctx,
                if cfg!(target_arch = "wasm32") {
                    "OBJ download started"
                } else {
                    "OBJ exported"
                },
            ),
            Ok(Outcome::Cancelled) => self.project.after_save = None,
            Err(error) => {
                self.project.after_save = None;
                self.project.error = Some(format!("Could not complete file operation: {error}"));
            }
        }
    }

    pub(super) fn project_dialogs(&mut self, ctx: &egui::Context) {
        if self.project.pending.is_some() {
            Modal::new("project_file_pending", 360.)
                .dismissible(false)
                .show(ctx, |modal| {
                    modal.header(|ui| {
                        ui.heading("Project file");
                    });
                    modal.body(|ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(match self.project.operation {
                                Operation::Open => "Opening project…",
                                Operation::Save => "Saving project…",
                                Operation::Import => "Importing SketchUp model…",
                                Operation::Export => "Exporting OBJ…",
                            });
                        });
                    });
                    modal.footer(|ui| {
                        ui.weak("Please wait…");
                    });
                });
        }
        if let Some(action) = self.project.confirm {
            let response = Modal::new("project_unsaved", 480.).show(ctx, |modal| {
                modal.header(|ui| {
                    ui.heading("Save project changes?");
                });
                modal.body(|ui| {
                    ui.label(format!(
                        "Save changes to “{}” as a .ketchup file before continuing?",
                        self.doc.name
                    ));
                });
                modal.footer(|ui| {
                    if ui.button("Cancel").clicked() {
                        Some(2)
                    } else if primary_button(ui, "Save", true).clicked() {
                        Some(0)
                    } else if ui
                        .button(match action {
                            #[cfg(not(target_arch = "wasm32"))]
                            Action::RestartForUpdate => "Restart with local recovery",
                            _ => "Discard changes",
                        })
                        .clicked()
                    {
                        Some(1)
                    } else {
                        None
                    }
                })
            });
            if let Some(choice) = response.inner {
                self.project.confirm = None;
                match choice {
                    0 => {
                        self.project.after_save = Some(action);
                        self.save_project(ctx, false);
                    }
                    1 => self.perform_project(action, ctx),
                    _ => {}
                }
            } else if response.should_close() {
                self.project.confirm = None;
            }
        }
        if let Some(error) = &self.project.error {
            let response = Modal::new("project_file_error", 480.).show(ctx, |modal| {
                modal.header(|ui| {
                    ui.heading("Project file");
                });
                modal.body(|ui| {
                    ui.label(error);
                });
                modal.footer(|ui| primary_button(ui, "OK", true).clicked())
            });
            if response.inner || response.should_close() {
                self.project.error = None;
            }
        }
    }
}
