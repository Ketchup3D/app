use super::*;
use crate::project_files::{self, Outcome};
use std::{path::PathBuf, sync::mpsc::TryRecvError};

#[derive(Clone, Copy)]
pub(super) enum Action {
    New,
    Open,
    Recent(usize),
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
    CuttingPlan,
    Screenshot,
}

pub(super) struct Project {
    pub path: Option<PathBuf>,
    pub recent: Vec<RecentProject>,
    pub import_format: String,
    pub export_stl: bool,
    pub modified: bool,
    pub pending: Option<project_files::Pending>,
    pub operation: Operation,
    pub snapshot: Option<String>,
    pub confirm: Option<Action>,
    pub after_save: Option<Action>,
    pub error: Option<String>,
    pub cutting_plan_selection: Option<bool>,
    pub cutting_plan_selection_preference: Option<bool>,
    pub cutting_plan_layout: crate::cutting_layout::Settings,
}
impl Default for Project {
    fn default() -> Self {
        Self {
            path: None,
            recent: Vec::new(),
            import_format: "skp".into(),
            export_stl: false,
            modified: false,
            pending: None,
            operation: Operation::default(),
            snapshot: None,
            confirm: None,
            after_save: None,
            error: None,
            cutting_plan_selection: None,
            cutting_plan_selection_preference: None,
            cutting_plan_layout: crate::cutting_layout::Settings::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct RecentProject {
    name: String,
    path: Option<PathBuf>,
    server: Option<(String, String)>,
    browser_file: Option<String>,
    #[serde(default)]
    browser_identity: Option<String>,
}

impl RecentProject {
    fn same_identity(&self, other: &Self) -> bool {
        if let Some(path) = &self.path { return other.path.as_ref() == Some(path); }
        if let Some(file) = &self.browser_file {
            return other.path.is_none() && other.browser_file.as_ref() == Some(file) && self.browser_identity == other.browser_identity && self.server == other.server;
        }
        other.path.is_none() && other.browser_file.is_none() && self.server == other.server
    }
    fn description(&self) -> String {
        let location = if let Some(path) = &self.path { path.display().to_string() }
        else if let Some(file) = &self.browser_file { format!("Browser file: {file} (browser hides the full path; reselect to open)") }
        else { String::new() };
        if let Some((server, id)) = &self.server {
            let url = format!("{}/share/{id}", server.trim_end_matches('/'));
            if location.is_empty() { url } else { format!("{location}\n{url}") }
        } else { location }
    }
}

impl Project {
    pub fn blocked(&self) -> bool {
        self.pending.is_some()
            || self.confirm.is_some()
            || self.error.is_some()
            || self.cutting_plan_selection.is_some()
    }
}

impl KetchupApp {
    pub(super) fn remember_recent_project(&mut self, browser_file: Option<(String, String)>) {
        let path = self.project.path.clone().map(|path| {
            #[cfg(not(target_arch = "wasm32"))]
            { std::fs::canonicalize(&path).unwrap_or(path) }
            #[cfg(target_arch = "wasm32")]
            { path }
        });
        let server = self.doc.shared_project.as_ref().map(|link|
            (link.server.trim_end_matches('/').to_owned(), link.id.clone()));
        if path.is_none() && server.is_none() && browser_file.is_none() { return; }
        let (browser_file, browser_identity) = browser_file.map_or((None, None), |(name, identity)| (Some(name), Some(identity)));
        let entry = RecentProject { name: self.doc.name.clone(), path, server, browser_file, browser_identity };
        self.project.recent.retain(|old| !entry.same_identity(old));
        self.project.recent.insert(0, entry);
        self.project.recent.truncate(10);
    }

    pub(super) fn recent_projects_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        Self::menu_submenu(ui, "Recent Projects", None, |ui| {
            if self.project.recent.is_empty() { ui.label("No recent projects"); }
            let mut chosen = None;
            let mut remove = None;
            for (index, entry) in self.project.recent.iter().enumerate() {
                let detail = entry.description();
                let mut label = egui::text::LayoutJob::default();
                label.append(&entry.name, 0., egui::TextFormat {
                    font_id: egui::TextStyle::Button.resolve(ui.style()),
                    color: ui.visuals().text_color(),
                    ..Default::default()
                });
                label.append(&format!("\n{detail}"), 0., egui::TextFormat {
                    font_id: egui::TextStyle::Small.resolve(ui.style()),
                    color: ui.visuals().weak_text_color(),
                    ..Default::default()
                });
                ui.horizontal(|ui| {
                    if ui.add(Self::menu_item_button(ui, label, None).wrap()).on_hover_text(detail).clicked() {
                        chosen = Some(index);
                    }
                    if ui.small_button("Remove")
                        .on_hover_text("Remove from recent projects")
                        .clicked()
                    {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                self.project.recent.remove(index);
                ctx.request_repaint();
            } else if let Some(index) = chosen {
                self.request_project(Action::Recent(index), ctx);
                ui.close();
            }
        });
    }

    pub(super) fn replace_project(
        &mut self,
        mut doc: Document,
        path: Option<PathBuf>,
        ctx: &egui::Context,
    ) {
        self.fasteners.open=false;
        self.fastener_dialog=Default::default();
        self.release_shared_selection(ctx);
        self.sync_stock_materials(ctx);
        if doc.shared_project.is_none() {
            if let Some(path) = &path {
                project_files::set_document_name(&mut doc, path);
            }
        }
        doc.display.group_emphasis = self.doc.display.group_emphasis;
        // Snapping targets are app preferences, including when an older project
        // carries the former geometry master switch in its document.
        doc.snap_to_geometry =
            self.snap_to_edge || self.snap_to_face || self.snap_to_face_center;
        self.set_tool(Tool::Select);
        egui::DragAndDrop::clear_payload(ctx);
        self.doc = doc;
        self.viewports
            .reframe_project(&self.doc.camera, &self.doc.viewport_cameras);
        self.selected = Self::restore_document_selection(&mut self.doc);
        self.outliner_anchor = None;
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
        self.sync_stock_materials(ctx);
        self.project.modified = false;
        self.project.confirm = None;
        self.project.after_save = None;
        self.project.error = None;
        self.project.cutting_plan_selection = None;
        let screenshot_settings = std::mem::take(&mut self.screenshot.settings);
        self.screenshot = screenshot_dialog::ScreenshotDialog::default();
        self.screenshot.settings = screenshot_settings;
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
            Action::Recent(index) => {
                let Some(entry) = self.project.recent.get(index).cloned() else { return; };
                if let Some(path) = entry.path {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.project.operation = Operation::Open;
                        self.project.pending = Some(project_files::open_path(ctx, path));
                        return;
                    }
                    #[cfg(target_arch = "wasm32")]
                    let _ = path;
                }
                if entry.browser_file.is_some() {
                    self.say(ctx, &format!("Reselect {} to reopen this browser file", entry.name));
                    self.perform_project(Action::Open, ctx);
                } else if let Some((server, id)) = entry.server {
                    if server.trim_end_matches('/') != self.account.server.trim_end_matches('/') {
                        self.project.error = Some(format!("This project belongs to {server}. Connect to that server in Account before opening it."));
                    } else {
                        self.sharing.preferences.invitation = id;
                        self.sharing.preferences.open = true;
                    }
                }
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
        self.project.pending = Some(project_files::import(
            ctx,
            self.doc.model.clone(),
            self.project.import_format.clone(),
        ));
    }

    pub(super) fn export_model(&mut self, ctx: &egui::Context) {
        if self.project.blocked() {
            return;
        }
        let geometry=self.doc.model.evaluate_joinery();
        if !geometry.errors.is_empty() {
            self.project.error=Some(geometry.errors.into_iter().map(|(id,error)|format!("Wood joint #{id}: {error}")).collect::<Vec<_>>().join(" · "));
            return;
        }
        match if self.project.export_stl {
            crate::mesh_exchange::export_stl(&geometry.model)
        } else {
            crate::interchange::export_obj(&geometry.model).map(String::into_bytes)
        } {
            Ok(contents) => {
                self.project.operation = Operation::Export;
                self.project.pending = Some(project_files::export(
                    ctx,
                    self.doc.name.clone(),
                    contents,
                    self.project.export_stl,
                ));
            }
            Err(error) => self.project.error = Some(error),
        }
    }

    pub(super) fn prepare_cutting_plan(&mut self) {
        if !self.project.blocked() {
            self.project.cutting_plan_selection = Some(
                !self.selected.is_empty()
                    && self
                        .project
                        .cutting_plan_selection_preference
                        .unwrap_or(true),
            );
        }
    }

    fn export_cutting_plan(&mut self, ctx: &egui::Context, selection: bool) {
        let selected = if selection {
            self.selected.clone()
        } else {
            BTreeSet::new()
        };
        if selection && selected.is_empty() {
            self.project.error =
                Some("Select at least one part, or choose the whole project.".into());
            return;
        }
        self.project.operation = Operation::CuttingPlan;
        self.project.pending = Some(project_files::export_cutting_plan(
            ctx,
            self.doc.name.clone(),
            self.doc.model.clone(),
            selected,
            self.project.cutting_plan_layout,
        ));
    }

    pub(super) fn save_project(&mut self, ctx: &egui::Context, save_as: bool) {
        if self.shared_save_check(ctx, save_as) {
            return;
        }
        self.save_project_checked(ctx, save_as);
    }

    pub(super) fn save_project_checked(&mut self, ctx: &egui::Context, save_as: bool) {
        if self.project.blocked() {
            return;
        }
        self.sync_stock_materials(ctx);
        self.sync_viewport_cameras();
        self.sync_selection();
        if let Some(link) = &mut self.doc.shared_project {
            link.file_updated_at = sharing::now_timestamp();
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
        let operation = self.project.operation;
        self.project.operation = Operation::Open;
        let snapshot = self.project.snapshot.take();
        let (result, browser_file) = match result {
            #[cfg(target_arch = "wasm32")]
            Ok(Outcome::BrowserOpened(doc, name, identity)) => (Ok(Outcome::Opened(doc, None)), Some((name, identity))),
            other => (other, None),
        };
        match result {
            Ok(Outcome::Opened(doc, path)) => {
                self.replace_project(*doc, path, ctx);
                self.remember_recent_project(browser_file);
                self.say(ctx, "Project opened");
            }
            Ok(Outcome::Saved(path)) => {
                let downloaded = cfg!(target_arch = "wasm32") && path.is_none();
                let modified = serde_json::to_string(&self.doc).ok() != snapshot;
                if let Some(path) = &path {
                    if self.doc.shared_project.is_none() {
                        project_files::set_document_name(&mut self.doc, path);
                    }
                    self.changed(ctx);
                }
                self.project.path = path;
                self.project.modified = modified;
                self.say(
                    ctx,
                    if downloaded {
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
                self.finish_edit(
                    if self.project.import_format == "skp" {
                        "Import SketchUp"
                    } else {
                        "Import geometry"
                    },
                    before,
                    ctx,
                );
                self.fit(ctx);
                self.say(ctx, "Geometry imported as editable groups");
            }
            Ok(Outcome::Exported) => self.say(
                ctx,
                match (operation, cfg!(target_arch = "wasm32")) {
                    (Operation::CuttingPlan, true) => "Cutting plan PDF opened in a new tab",
                    (Operation::CuttingPlan, false) => "Cutting plan PDF saved and opened",
                    (Operation::Screenshot, true) => "Screenshot download started",
                    (Operation::Screenshot, false) => "Screenshot exported",
                    (_, true) => "Geometry download started",
                    (_, false) => "Geometry exported",
                },
            ),
            #[cfg(target_arch = "wasm32")]
            Ok(Outcome::BrowserOpened(..)) => unreachable!(),
            Ok(Outcome::Cancelled) => self.project.after_save = None,
            Err(error) => {
                self.project.after_save = None;
                self.project.error = Some(format!("Could not complete file operation: {error}"));
            }
        }
    }

    pub(super) fn project_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut selection) = self.project.cutting_plan_selection {
            let selection_before = selection;
            let response = Modal::new("woodworking_cutting_plan", 480.).show(ctx, |modal| {
                modal.header(|ui| {
                    ui.text_heading("Woodworking cutting plan");
                    ui.text_weak("Create a PDF with quantities and finished cut sizes.");
                });
                modal.body(|ui| {
                    ui.add_enabled_ui(!self.selected.is_empty(), |ui| {
                        ui.radio_value(&mut selection, true, "Selected parts / assemblies (visible only)");
                    });
                    ui.radio_value(&mut selection, false, "Whole project (visible only)");
                    ui.add_space(10.);
                    ui.horizontal(|ui| {
                        ui.text_label("Saw kerf (cut width)");
                        ui.add(egui::DragValue::new(&mut self.project.cutting_plan_layout.kerf_mm)
                            .speed(0.1).range(0. ..=100.).suffix(" mm"));
                    });
                    ui.checkbox(&mut self.project.cutting_plan_layout.allow_rotation, "Allow rotating pieces 90 degrees")
                        .on_hover_text("Leave off when the length must follow the stock grain.");
                    ui.text_weak("Only visible parts with assigned stock materials are included. Assign materials in Entity Info using the stock pricing table. Nested parts inherit their assembly’s material unless assigned separately.");
                    ui.text_weak("Layouts use assigned stock dimensions and saw kerf. Confirm grain, joinery and irregular shapes against the model; this is a practical layout, not a guaranteed minimum.");
                });
                modal.footer(|ui| {
                    if ui.button("Cancel").clicked() { Some(false) }
                    else if primary_button(ui, "Generate PDF…", true).clicked() { Some(true) }
                    else { None }
                })
            });
            self.project.cutting_plan_selection = Some(selection);
            if selection != selection_before {
                self.project.cutting_plan_selection_preference = Some(selection);
            }
            if let Some(generate) = response.inner {
                self.project.cutting_plan_selection = None;
                if generate {
                    self.export_cutting_plan(ctx, selection);
                }
            } else if response.should_close() {
                self.project.cutting_plan_selection = None;
            }
        }
        if self.project.pending.is_some() {
            Modal::new("project_file_pending", 360.)
                .dismissible(false)
                .show(ctx, |modal| {
                    modal.header(|ui| {
                        ui.text_heading("Project file");
                    });
                    modal.body(|ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.text_label(match self.project.operation {
                                Operation::Open => "Opening project…",
                                Operation::Save => "Saving project…",
                                Operation::Import => "Importing geometry…",
                                Operation::Export => "Exporting geometry…",
                                Operation::CuttingPlan => "Preparing cutting plan PDF…",
                                Operation::Screenshot => "Exporting screenshot…",
                            });
                        });
                    });
                    modal.footer(|ui| {
                        ui.text_weak("Please wait…");
                    });
                });
        }
        if let Some(action) = self.project.confirm {
            let response = Modal::new("project_unsaved", 480.).show(ctx, |modal| {
                modal.header(|ui| {
                    ui.text_heading("Save project changes?");
                });
                modal.body(|ui| {
                    ui.text_label(format!(
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
                    ui.text_heading("Project file");
                });
                modal.body(|ui| {
                    ui.text_label(error);
                });
                modal.footer(|ui| primary_button(ui, "OK", true).clicked())
            });
            if response.inner || response.should_close() {
                self.project.error = None;
            }
        }
    }
}

#[cfg(test)]
mod recent_tests {
    use super::*;

    #[test]
    fn recent_projects_deduplicate_move_to_front_and_cap_at_ten() {
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        for index in 0..12 {
            app.project.path = Some(format!("project-{index}.ketchup").into());
            app.remember_recent_project(None);
        }
        assert_eq!(app.project.recent.len(), 10);
        app.project.path = Some("project-5.ketchup".into());
        app.remember_recent_project(None);
        assert_eq!(app.project.recent.len(), 10);
        assert!(app.project.recent[0].path.as_ref().unwrap().ends_with("project-5.ketchup"));
        assert_ne!(app.project.recent[0].description(), app.project.recent[1].description());
    }

    #[test]
    fn browser_and_server_sources_are_distinct() {
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.remember_recent_project(Some(("model.ketchup".into(), "model.ketchup:100:1".into())));
        app.remember_recent_project(Some(("model.ketchup".into(), "model.ketchup:100:1".into())));
        assert_eq!(app.project.recent.len(), 1);
        app.doc.shared_project = Some(ketchup_core::SharedProject {
            server: "https://example.test".into(), id: "project".into(),
            revision: 1, server_updated_at: 0, file_updated_at: 0,
        });
        app.remember_recent_project(None);
        assert_eq!(app.project.recent.len(), 2);
        assert!(app.project.recent[1].description().contains("reselect"));
    }
}
