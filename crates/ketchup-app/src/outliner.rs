use super::*;

#[derive(Clone, Copy)]
struct OutlinerAction {
    id: u64,
    additive: bool,
    open: bool,
}

struct Outliner<'a> {
    model: &'a Model,
    selected: &'a BTreeSet<u64>,
    entities: &'a renderer::EntityCache,
    action: Option<OutlinerAction>,
}

impl Outliner<'_> {
    fn row(&mut self, ui: &mut egui::Ui, id: u64) -> egui::Response {
        let state = self.entities.get(id);
        let mut label = if let Some(object) = self.model.objects.get(&id) {
            let kind = match object.kind {
                ObjectKind::Group => "Group",
                ObjectKind::Component { .. } => "Component",
            };
            let name = object.name.trim();
            if name.is_empty() {
                format!("{kind} #{id}")
            } else {
                format!("{kind}: {name}")
            }
        } else if self.model.edges.contains_key(&id) {
            format!("Edge #{id}")
        } else if self.model.faces.contains_key(&id) {
            format!("Face #{id}")
        } else {
            format!("Dimension #{id}")
        };
        if state.hidden {
            label.push_str(" · Hidden");
        }
        if state.locked {
            label.push_str(" · Locked");
        }
        if self.model.edit_context == Some(id) {
            label.push_str(" · Editing");
        }
        let text = RichText::new(&label);
        let text = if state.hidden {
            text.color(Palette::for_ui(ui).muted)
        } else {
            text
        };
        let response = ui
            .add(egui::Button::selectable(self.selected.contains(&id), text).truncate())
            .on_hover_text(format!(
                "{label}\nEntity #{id}\n{}",
                if state.hidden {
                    "Hidden entities stay listed. Use Display to show hidden geometry or objects; check Tags for tag visibility."
                } else {
                    "Click to select · Shift/Ctrl/Cmd-click to toggle selection · Double-click an object to edit"
                }
            ));
        if response.clicked() || response.double_clicked() {
            self.action = Some(OutlinerAction {
                id,
                additive: ui
                    .input(|i| i.modifiers.shift || i.modifiers.ctrl || i.modifiers.command),
                open: response.double_clicked(),
            });
        }
        response
    }

    fn level(&mut self, ui: &mut egui::Ui, ids: &BTreeSet<u64>, parent: Option<u64>) {
        let model = self.model;
        // Keep objects above raw geometry so large meshes don't bury the hierarchy.
        for id in ids.iter().filter(|id| model.objects.contains_key(id)) {
            let object = &model.objects[id];
            let state_id = ui.make_persistent_id(("outliner_object", id));
            let mut header = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                state_id,
                false,
            )
            .show_header(ui, |ui| self.row(ui, *id));
            if self
                .action
                .is_some_and(|action| action.id == *id && action.open)
            {
                header.set_open(true);
            }
            header.body(|ui| {
                if object.entities.is_empty() {
                    ui.weak("Empty object");
                } else {
                    self.level(ui, &object.entities, Some(*id));
                }
            });
        }
        let geometry: Vec<_> = ids
            .iter()
            .filter(|id| !model.objects.contains_key(id))
            .copied()
            .collect();
        if !geometry.is_empty() {
            egui::CollapsingHeader::new(format!("Geometry ({})", geometry.len()))
                .id_salt(("outliner_geometry", parent))
                .show(ui, |ui| {
                    // Virtualize geometry rows; imported meshes can contain millions of entities.
                    let height = ui.spacing().interact_size.y;
                    egui::ScrollArea::vertical()
                        .id_salt(("outliner_geometry_rows", parent))
                        .max_height(180.)
                        .auto_shrink([false, true])
                        .show_rows(ui, height, geometry.len(), |ui, range| {
                            for index in range {
                                let id = geometry[index];
                                ui.push_id(id, |ui| {
                                    self.row(ui, id);
                                });
                            }
                        });
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nested_model() -> (KetchupApp, u64, u64, u64) {
        let mut model = Model::default();
        let edge = model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        let component = model
            .make_object(&BTreeSet::from([edge]), true, "Handle")
            .unwrap();
        let group = model
            .make_object(&BTreeSet::from([component]), false, "Cabinet")
            .unwrap();
        // Grouping copies the contents into a new scope and assigns fresh IDs.
        let component = *model.objects[&group].entities.first().unwrap();
        let edge = *model.objects[&component].entities.first().unwrap();
        let doc = Document {
            model,
            ..Default::default()
        };
        (
            KetchupApp::with_document(doc, None, true),
            group,
            component,
            edge,
        )
    }

    fn select(id: u64) -> OutlinerAction {
        OutlinerAction {
            id,
            additive: false,
            open: false,
        }
    }

    #[test]
    fn tree_selection_navigates_to_the_entity_parent_and_opens_objects() {
        let (mut app, group, component, edge) = nested_model();
        let ctx = egui::Context::default();
        let original = app.doc.model.clone();
        app.apply_outliner_action(select(edge), &ctx);
        assert_eq!(app.doc.model.edit_context, Some(component));
        assert_eq!(app.selected, BTreeSet::from([edge]));

        app.apply_outliner_action(select(group), &ctx);
        assert_eq!(app.doc.model.edit_context, None);
        assert_eq!(app.selected, BTreeSet::from([group]));
        app.apply_outliner_action(
            OutlinerAction {
                open: true,
                ..select(component)
            },
            &ctx,
        );
        assert_eq!(app.doc.model.edit_context, Some(component));
        assert!(app.selected.is_empty());
        app.doc.model.edit_context = None;
        assert_eq!(app.doc.model, original);
        assert!(app.history.undo_label().is_none());
    }

    #[test]
    fn tree_selection_respects_hidden_entities_tags_and_locked_ancestors() {
        let (mut app, group, component, edge) = nested_model();
        let ctx = egui::Context::default();
        app.selected.insert(group);
        app.doc.model.hidden.insert(component);
        app.apply_outliner_action(select(edge), &ctx);
        assert_eq!(app.selected, BTreeSet::from([group]));
        assert_eq!(app.doc.model.edit_context, None);

        app.doc.model.hidden.clear();
        let tag = app.doc.model.add_tag("Hidden parts").unwrap();
        app.doc
            .model
            .assign_tag(&BTreeSet::from([group]), Some(tag))
            .unwrap();
        app.doc.model.tags.get_mut(&tag).unwrap().visible = false;
        app.apply_outliner_action(select(edge), &ctx);
        assert_eq!(app.selected, BTreeSet::from([group]));
        assert_eq!(app.doc.model.edit_context, None);

        app.doc.model.tags.get_mut(&tag).unwrap().visible = true;
        app.doc.model.locked.insert(group);
        app.apply_outliner_action(select(edge), &ctx);
        assert_eq!(app.selected, BTreeSet::from([group]));
        assert_eq!(app.doc.model.edit_context, None);
    }

    #[test]
    fn additive_tree_selection_stays_in_one_edit_context() {
        let (mut app, group, component, edge) = nested_model();
        let ctx = egui::Context::default();
        app.selected.insert(group);
        let action = OutlinerAction {
            additive: true,
            ..select(edge)
        };
        app.apply_outliner_action(action, &ctx);
        assert_eq!(app.doc.model.edit_context, Some(component));
        assert_eq!(app.selected, BTreeSet::from([edge]));
        app.apply_outliner_action(action, &ctx);
        assert!(app.selected.is_empty());
    }

    #[test]
    fn tree_renders_nested_components_and_inherited_hidden_geometry() {
        let (mut app, group, _, edge) = nested_model();
        app.doc.model.hidden.insert(group);
        app.doc.model.locked.insert(group);
        let ctx = egui::Context::default();
        ctx.memory_mut(|memory| memory.set_everything_is_visible(true));
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600., 900.))),
                ..Default::default()
            },
            |ui| app.outliner_contents(ui),
        );
        fn texts(shape: &egui::Shape, labels: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => labels.push(text.galley.job.text.clone()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        texts(shape, labels);
                    }
                }
                _ => {}
            }
        }
        let mut labels = Vec::new();
        for shape in &output.shapes {
            texts(&shape.shape, &mut labels);
        }
        assert!(labels.contains(&"Group: Cabinet · Hidden · Locked".into()));
        assert!(labels.contains(&"Component: Handle · Hidden · Locked".into()));
        assert!(labels.contains(&"Geometry (1)".into()));
        assert!(labels.contains(&format!("Edge #{edge} · Hidden · Locked")));
    }
}

impl KetchupApp {
    pub(super) fn outliner_contents(&mut self, ui: &mut egui::Ui) {
        let roots = self.doc.model.root_entities();
        if roots.is_empty() {
            ui.weak("No objects or geometry yet");
            return;
        }
        self.entity_cache.update(&self.doc.model);
        let mut tree = Outliner {
            model: &self.doc.model,
            selected: &self.selected,
            entities: &self.entity_cache,
            action: None,
        };
        ui.label(
            RichText::new("All entities, including hidden geometry")
                .small()
                .color(Palette::for_ui(ui).muted),
        );
        egui::ScrollArea::both()
            .id_salt("outliner_tree")
            .max_height(280.)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().indent = 16.;
                egui::CollapsingHeader::new(&self.doc.name)
                    .id_salt("outliner_model")
                    .default_open(true)
                    .show(ui, |ui| tree.level(ui, &roots, None));
            });
        if let Some(action) = tree.action {
            self.apply_outliner_action(action, ui.ctx());
        }
    }

    fn apply_outliner_action(&mut self, action: OutlinerAction, ctx: &egui::Context) {
        let id = action.id;
        if !self.doc.model.contains_entity(&id) {
            return;
        }
        // Browsing the tree must not bypass visibility or locked edit contexts.
        if self.doc.model.is_hidden(id) {
            self.say(ctx, "Unhide the entity or show its tag before selecting it");
            return;
        }
        let parent = self.doc.model.parent(id);
        if parent.is_some_and(|parent| self.doc.model.is_locked(parent)) {
            self.say(
                ctx,
                "Unlock the parent object before selecting its contents",
            );
            return;
        }
        if self.doc.model.edit_context != parent {
            self.navigate_context(parent, ctx);
        } else {
            self.set_tool(Tool::Select);
        }
        if action.open && self.doc.model.objects.contains_key(&id) {
            self.enter_object(id, ctx);
        } else if !action.additive {
            self.selected = BTreeSet::from([id]);
        } else if !self.selected.remove(&id) {
            self.selected.insert(id);
        }
        self.hover = None;
        ctx.request_repaint();
    }
}
