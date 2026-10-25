use super::*;

#[derive(Clone, Copy)]
struct OutlinerAction {
    id: u64,
    additive: bool,
    range: bool,
    open: bool,
}

struct OutlinerDrag {
    ids: BTreeSet<u64>,
}

struct OutlinerDrop {
    ids: BTreeSet<u64>,
    parent: Option<u64>,
    before: Option<u64>,
}

#[derive(Clone, Copy)]
struct OutlinerDragHover {
    id: u64,
    since: f64,
    frame: u64,
}

struct Outliner<'a> {
    model: &'a Model,
    selected: &'a BTreeSet<u64>,
    entities: &'a renderer::EntityCache,
    action: Option<OutlinerAction>,
    hover: Option<u64>,
    context_menu: Option<(u64, egui::Response)>,
    drop: Option<OutlinerDrop>,
    drop_hover: Option<u64>,
    drag_hover: Option<OutlinerDragHover>,
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
        let object = self.model.objects.get(&id);
        let button = egui::Button::selectable(self.selected.contains(&id), text)
            .truncate()
            .sense(if object.is_some() && !state.locked {
                Sense::click_and_drag()
            } else {
                Sense::click()
            });
        // Keep the drag source ID stable while the hierarchy changes.
        let row_id = ui.make_persistent_id(("outliner_entity_row", id));
        let response = ui
            .scope_builder(egui::UiBuilder::new().scope_id(row_id), |ui| ui.add(button))
            .inner
            .on_hover_text(format!(
                "{label}\nEntity #{id}\n{}",
                if state.hidden {
                    "Click to select · Shift-click to select a range · Ctrl/Cmd-click to toggle selection · Right-click to unhide; check Tags for tag visibility"
                } else {
                    "Click to select · Shift-click to select a range · Ctrl/Cmd-click to toggle selection · Double-click to edit · Drag onto a group to nest, or between rows to reorder"
                }
            ));
        if object.is_some() && response.drag_started_by(egui::PointerButton::Primary) {
            let ids = if self.selected.contains(&id) {
                self.selected
                    .iter()
                    .filter(|id| self.model.objects.contains_key(id))
                    .copied()
                    .collect()
            } else {
                BTreeSet::from([id])
            };
            response.dnd_set_drag_payload(OutlinerDrag { ids });
        }
        if object.is_some() {
            self.object_drop_target(ui, &response, id);
        } else if response.dnd_hover_payload::<OutlinerDrag>().is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
        }
        if response.hovered() {
            self.hover = Some(id);
        }
        if response.secondary_clicked()
            || self.context_menu.is_none() && response.context_menu_opened()
        {
            // Draw the shared menu after releasing the tree's model borrow.
            self.context_menu = Some((id, response.clone()));
        }
        if response.clicked() || response.double_clicked() {
            let (additive, range) =
                ui.input(|i| (i.modifiers.ctrl || i.modifiers.command, i.modifiers.shift));
            self.action = Some(OutlinerAction {
                id,
                additive,
                range,
                open: response.double_clicked() && !additive && !range,
            });
        }
        response
    }

    fn object_drop_target(&mut self, ui: &egui::Ui, response: &egui::Response, id: u64) {
        if response.dnd_hover_payload::<OutlinerDrag>().is_none() {
            return;
        }
        let Some(pointer) = ui.ctx().pointer_interact_pos() else {
            return;
        };
        let margin = response.rect.height() * 0.25;
        let above = pointer.y < response.rect.top() + margin;
        let below = pointer.y > response.rect.bottom() - margin;
        if above || below {
            let parent = self.model.parent(id);
            let siblings = parent
                .map(|id| self.model.objects[&id].entities.clone())
                .unwrap_or_else(|| self.model.root_entities());
            let siblings = self.model.ordered_objects(&siblings);
            let before = if above {
                Some(id)
            } else {
                siblings
                    .iter()
                    .position(|other| *other == id)
                    .and_then(|index| siblings.get(index + 1))
                    .copied()
            };
            let y = if above {
                response.rect.top()
            } else {
                response.rect.bottom()
            };
            self.drop_target(ui, response, parent, before, Some(y));
        } else {
            self.drop_target(ui, response, Some(id), None, None);
        }
    }

    fn drop_target(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        parent: Option<u64>,
        before: Option<u64>,
        insertion_y: Option<f32>,
    ) {
        let Some(payload) = response.dnd_hover_payload::<OutlinerDrag>() else {
            return;
        };
        let valid = self
            .model
            .check_move_objects_before(&payload.ids, parent, before)
            .is_ok();
        let colors = Palette::for_ui(ui);
        let stroke = Stroke::new(2., if valid { colors.green } else { colors.accent });
        if let Some(y) = insertion_y {
            ui.painter().line_segment(
                [
                    Pos2::new(response.rect.left(), y),
                    Pos2::new(ui.max_rect().right(), y),
                ],
                stroke,
            );
        } else {
            ui.painter().rect_stroke(
                response.rect.expand(2.),
                3,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        ui.ctx().set_cursor_icon(if valid {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::NotAllowed
        });
        if valid && insertion_y.is_none() {
            self.drop_hover = parent;
        }
        if let Some(payload) = response.dnd_release_payload::<OutlinerDrag>() {
            self.drop = Some(OutlinerDrop {
                ids: payload.ids.clone(),
                parent,
                before,
            });
        }
    }

    fn root_drop_target(&mut self, ui: &mut egui::Ui) {
        if egui::DragAndDrop::has_payload_of_type::<OutlinerDrag>(ui.ctx()) {
            let response = ui.allocate_response(
                egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                Sense::hover(),
            );
            self.drop_target(ui, &response, None, None, Some(response.rect.top()));
        }
    }

    fn should_expand_drop_target(&mut self, ctx: &egui::Context, id: u64) -> bool {
        if self.drop_hover != Some(id) || !ctx.input(|i| i.pointer.primary_down()) {
            return false;
        }
        let now = ctx.input(|i| i.time);
        let frame = ctx.cumulative_frame_nr();
        let hover = self.drag_hover.get_or_insert(OutlinerDragHover {
            id,
            since: now,
            frame,
        });
        if hover.id != id || hover.frame.saturating_add(1) < frame {
            *hover = OutlinerDragHover {
                id,
                since: now,
                frame,
            };
        }
        hover.frame = frame;
        let remaining = 0.5 - (now - hover.since);
        if remaining <= 0. {
            true
        } else {
            ctx.request_repaint_after(Duration::from_secs_f64(remaining));
            false
        }
    }

    fn level(&mut self, ui: &mut egui::Ui, ids: &BTreeSet<u64>, parent: Option<u64>) {
        let model = self.model;
        let ctx = ui.ctx().clone();
        // Keep objects above raw geometry so large meshes don't bury the hierarchy.
        for id in model.ordered_objects(ids) {
            let object = &model.objects[&id];
            let state_id = ui.make_persistent_id(("outliner_object", id));
            let key = format!("outliner_object/{id}");
            restore_section(ui.ctx(), state_id, &key, false);
            let mut header = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                state_id,
                false,
            )
            .show_header(ui, |ui| self.row(ui, id));
            if self
                .action
                .is_some_and(|action| action.id == id && action.open)
                || self.should_expand_drop_target(&ctx, id)
            {
                header.set_open(true);
            }
            header.body(|ui| {
                if object.entities.is_empty() {
                    ui.weak("Empty object");
                } else {
                    self.level(ui, &object.entities, Some(id));
                }
            });
            remember_section(ui.ctx(), state_id, &key);
        }
        let geometry: Vec<_> = ids
            .iter()
            .filter(|id| !model.objects.contains_key(id))
            .copied()
            .collect();
        if !geometry.is_empty() {
            let key = format!("outliner_geometry/{parent:?}");
            let open = restored_header_open(ui, &key, false);
            let header = egui::CollapsingHeader::new(format!("Geometry ({})", geometry.len()))
                .id_salt(("outliner_geometry", parent))
                .open(open)
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
            remember_section(ui.ctx(), header.header_response.id, &key);
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
            range: false,
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
    fn tree_ranges_include_offscreen_geometry_in_object_first_order() {
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let ctx = egui::Context::default();
        let edges: Vec<_> = (0..100)
            .map(|index| {
                let start = DVec3::Y * index as f64;
                app.doc.model.add_edge(start, start + DVec3::X).unwrap()
            })
            .collect();
        let group = app
            .doc
            .model
            .make_object(&BTreeSet::from([edges[0]]), false, "First row")
            .unwrap();
        let child = *app.doc.model.objects[&group].entities.first().unwrap();
        app.apply_outliner_action(select(group), &ctx);
        app.apply_outliner_action(
            OutlinerAction {
                range: true,
                ..select(*edges.last().unwrap())
            },
            &ctx,
        );
        let expected: BTreeSet<_> = std::iter::once(group)
            .chain(edges.iter().skip(1).copied())
            .collect();
        assert_eq!(app.selected, expected);
        assert!(!app.selected.contains(&child));
        assert_eq!(app.outliner_anchor, Some(group));

        // Deleting the anchor falls back to the clicked row.
        app.doc.model.erase(&BTreeSet::from([group]));
        app.apply_outliner_action(
            OutlinerAction {
                range: true,
                ..select(edges[50])
            },
            &ctx,
        );
        assert_eq!(app.selected, BTreeSet::from([edges[50]]));
        assert_eq!(app.outliner_anchor, Some(edges[50]));
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
        let hover_id = ui.id().with("outliner_drag_hover");
        let mut tree = Outliner {
            model: &self.doc.model,
            selected: &self.selected,
            entities: &self.entity_cache,
            action: None,
            hover: None,
            context_menu: None,
            drop: None,
            drop_hover: None,
            drag_hover: ui
                .ctx()
                .data(|data| data.get_temp::<Option<OutlinerDragHover>>(hover_id))
                .flatten(),
        };
        // Let the inspector handle vertical scrolling as the tree expands.
        egui::ScrollArea::horizontal()
            .id_salt("outliner_tree")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().indent = 16.;
                tree.level(ui, &roots, None);
                tree.root_drop_target(ui);
            });
        let Outliner {
            action,
            hover,
            context_menu,
            drop,
            drop_hover,
            drag_hover,
            ..
        } = tree;
        let drag_hover = drag_hover.filter(|hover| {
            drop_hover == Some(hover.id)
                && ui.input(|i| i.pointer.primary_down())
                && egui::DragAndDrop::has_payload_of_type::<OutlinerDrag>(ui.ctx())
        });
        ui.ctx()
            .data_mut(|data| data.insert_temp(hover_id, drag_hover));
        self.outliner_hover = self.doc.model.expand_entities(&hover.into_iter().collect());
        if let Some(payload) = egui::DragAndDrop::payload::<OutlinerDrag>(ui.ctx())
            && let Some(pointer) = ui.ctx().pointer_interact_pos()
        {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                ui.id().with("outliner_drag_label"),
            ));
            let colors = Palette::for_ui(ui);
            let label = format!(
                "Move {} object{}",
                payload.ids.len(),
                if payload.ids.len() == 1 { "" } else { "s" }
            );
            let text = painter.layout_no_wrap(label, FontId::proportional(12.), colors.ink);
            let rect = Rect::from_min_size(
                pointer + egui::vec2(16., 16.),
                text.size() + egui::vec2(16., 10.),
            );
            painter.rect_filled(rect, 4, colors.paper);
            painter.galley(rect.min + egui::vec2(8., 5.), text, colors.ink);
            let clip = ui.clip_rect();
            if ui.input(|input| input.pointer.primary_down())
                && clip.contains(pointer)
                && ui.min_rect().contains(pointer)
            {
                let edge = 28.;
                let speed = ((clip.top() + edge - pointer.y).max(0.)
                    - (pointer.y - (clip.bottom() - edge)).max(0.))
                    * 12.;
                if speed != 0. {
                    let dt = ui.input(|input| input.stable_dt.min(0.05));
                    ui.scroll_with_delta_animation(
                        egui::vec2(0., speed * dt),
                        egui::style::ScrollAnimation::none(),
                    );
                    ui.ctx().request_repaint();
                }
            }
        }
        if let Some(drop) = drop {
            self.apply_outliner_drop(drop, ui.ctx());
            return;
        }
        if let Some(action) = action {
            self.apply_outliner_action(action, ui.ctx());
        }
        if let Some((id, response)) = context_menu {
            if response.secondary_clicked() {
                if !self.prepare_outliner_selection(id, ui.ctx()) {
                    return;
                }
                self.select_context_target(Some(id));
                self.context_position = None;
            }
            self.context_menu(&response);
        }
    }

    fn apply_outliner_drop(&mut self, drop: OutlinerDrop, ctx: &egui::Context) {
        let before = self.doc.model.clone();
        match self
            .doc
            .model
            .move_objects_before(&drop.ids, drop.parent, drop.before)
        {
            Ok(ids) => {
                let changed = self.doc.model != before;
                self.navigate_context(drop.parent, ctx);
                self.outliner_hover.clear();
                if changed {
                    self.finish_edit("Move objects", before, ctx);
                }
                self.selected = ids;
            }
            Err(message) => self.say(ctx, message),
        }
    }

    fn apply_outliner_action(&mut self, action: OutlinerAction, ctx: &egui::Context) {
        let id = action.id;
        if !self.doc.model.contains_entity(&id) {
            return;
        }
        let anchor = self.outliner_anchor.filter(|anchor| {
            self.doc.model.contains_entity(anchor)
                && self.doc.model.parent(*anchor) == self.doc.model.parent(id)
        });
        if !self.prepare_outliner_selection(id, ctx) {
            return;
        }
        if action.open
            && !action.additive
            && !action.range
            && self.doc.model.objects.contains_key(&id)
        {
            self.enter_object(id, ctx);
        } else if action.range {
            let anchor = anchor.unwrap_or(id);
            let range = self.outliner_range(anchor, id);
            if !action.additive {
                self.selected.clear();
            }
            self.selected.extend(range);
            // Keep the anchor fixed while Shift-click adjusts the end of the range.
            self.outliner_anchor = Some(anchor);
        } else if !action.additive {
            self.selected = BTreeSet::from([id]);
            self.outliner_anchor = Some(id);
        } else {
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
            self.outliner_anchor = Some(id);
        }
        self.hover = None;
        ctx.request_repaint();
    }

    fn outliner_range(&self, anchor: u64, target: u64) -> BTreeSet<u64> {
        let model = &self.doc.model;
        let siblings = model
            .edit_context
            .map(|id| model.objects[&id].entities.clone())
            .unwrap_or_else(|| model.root_entities());
        // Match the tree's object-first order, including virtualized geometry rows.
        let mut order = model.ordered_objects(&siblings);
        order.extend(
            siblings
                .into_iter()
                .filter(|id| !model.objects.contains_key(id)),
        );
        let start = order.iter().position(|id| *id == anchor);
        let end = order.iter().position(|id| *id == target);
        match (start, end) {
            (Some(a), Some(b)) => order[a.min(b)..=a.max(b)].iter().copied().collect(),
            _ => BTreeSet::from([target]),
        }
    }

    fn prepare_outliner_selection(&mut self, id: u64, ctx: &egui::Context) -> bool {
        if !self.doc.model.contains_entity(&id) {
            return false;
        }
        let parent = self.doc.model.parent(id);
        if parent.is_some_and(|parent| self.doc.model.is_locked(parent)) {
            self.say(
                ctx,
                "Unlock the parent object before selecting its contents",
            );
            return false;
        }
        if parent.is_some_and(|parent| self.doc.model.is_hidden(parent)) {
            self.say(
                ctx,
                "Unhide the parent object before selecting its contents",
            );
            return false;
        }
        if self.doc.model.edit_context != parent {
            self.navigate_context(parent, ctx);
        } else {
            self.set_tool(Tool::Select);
        }
        ctx.request_repaint();
        true
    }
}
