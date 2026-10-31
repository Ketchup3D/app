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
    paint: &'a std::collections::BTreeMap<u64, OutlinerPaint>,
    materials: &'a std::collections::BTreeMap<u64, OutlinerMaterial>,
    filtered_objects: Option<&'a BTreeSet<u64>>,
    selected: &'a BTreeSet<u64>,
    entities: &'a renderer::EntityCache,
    action: Option<OutlinerAction>,
    toggle_visibility: Option<u64>,
    hover: Option<u64>,
    context_menu: Option<(u64, egui::Response)>,
    drop: Option<OutlinerDrop>,
    drop_hover: Option<u64>,
    drag_hover: Option<OutlinerDragHover>,
}

// Hierarchy summaries depend on the document, not selection, camera, scroll
// position or disclosure state. Reuse them while interacting with the scene.
#[derive(Default)]
pub(in crate::app) struct OutlinerCache {
    revision: Option<u64>,
    roots: BTreeSet<u64>,
    paint: std::collections::BTreeMap<u64, OutlinerPaint>,
    materials: std::collections::BTreeMap<u64, OutlinerMaterial>,
    query: String,
    filtered_objects: Option<BTreeSet<u64>>,
    #[cfg(test)]
    summary_builds: usize,
}

impl OutlinerCache {
    fn update(&mut self, model: &Model, revision: u64, query: &str) {
        let changed = self.revision != Some(revision);
        if changed {
            self.roots = model.root_entities();
            self.materials = outliner_materials(model, &self.roots);
            self.paint = if model.face_colors.is_empty() {
                Default::default()
            } else {
                outliner_paint(model, &self.roots)
            };
            self.revision = Some(revision);
            #[cfg(test)]
            {
                self.summary_builds += 1;
            }
        }
        if changed || self.query != query {
            self.query = query.to_owned();
            self.filtered_objects = matching_outliner_objects(model, query);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutlinerMaterial {
    None,
    Partial,
    Full,
}

impl OutlinerMaterial {
    fn icon(self) -> &'static str {
        match self {
            Self::None => "material_none",
            Self::Partial => "material_partial",
            Self::Full => "material_full",
        }
    }

    fn color(self, ui: &egui::Ui) -> Color32 {
        match self {
            Self::None => Palette::for_ui(ui).muted,
            Self::Partial => ui.visuals().warn_fg_color,
            Self::Full => Palette::for_ui(ui).green,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::None => "No material assigned",
            Self::Partial => "Partial material assignment",
            Self::Full => "Full material assignment",
        }
    }

    fn merge(self, other: Self) -> Self {
        if self == other { self } else { Self::Partial }
    }
}

// Stock assignments inherit from the nearest assigned assembly, just as in the
// cutting plan. Include hidden and collapsed parts, and aggregate once per frame.
fn outliner_materials(
    model: &Model,
    roots: &BTreeSet<u64>,
) -> std::collections::BTreeMap<u64, OutlinerMaterial> {
    fn visit(
        model: &Model,
        id: u64,
        inherited: bool,
        states: &mut std::collections::BTreeMap<u64, OutlinerMaterial>,
    ) -> OutlinerMaterial {
        let assigned = if inherited {
            OutlinerMaterial::Full
        } else {
            OutlinerMaterial::None
        };
        let state = if let Some(object) = model.objects.get(&id) {
            let inherited = inherited || model.object_stock_material(object).is_some();
            object
                .entities
                .iter()
                .filter(|id| {
                    model.objects.contains_key(id)
                        || model.edges.contains_key(id)
                        || model.faces.contains_key(id)
                })
                .map(|id| visit(model, *id, inherited, states))
                .reduce(OutlinerMaterial::merge)
                .unwrap_or(if inherited {
                    OutlinerMaterial::Full
                } else {
                    OutlinerMaterial::None
                })
        } else {
            assigned
        };
        states.insert(id, state);
        state
    }
    let mut states = std::collections::BTreeMap::new();
    for id in roots {
        visit(model, *id, false, &mut states);
    }
    states
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutlinerPaint {
    Empty,
    Unpainted,
    Color([u8; 3]),
    Mixed,
}

impl OutlinerPaint {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Empty, paint) | (paint, Self::Empty) => paint,
            (a, b) if a == b => a,
            _ => Self::Mixed,
        }
    }
}

// Aggregate the hierarchy once, including collapsed and hidden descendants.
// Derive colors from document materials so face edits and undo cannot leave a
// stale group color, and avoid rescanning every descendant for each visible row.
fn outliner_paint(
    model: &Model,
    roots: &BTreeSet<u64>,
) -> std::collections::BTreeMap<u64, OutlinerPaint> {
    fn visit(
        model: &Model,
        id: u64,
        colors: &mut std::collections::BTreeMap<u64, OutlinerPaint>,
    ) -> OutlinerPaint {
        if let Some(object) = model.objects.get(&id) {
            let paint = object
                .entities
                .iter()
                .fold(OutlinerPaint::Empty, |paint, id| {
                    paint.merge(visit(model, *id, colors))
                });
            colors.insert(id, paint);
            paint
        } else if model.faces.contains_key(&id) {
            model
                .face_colors
                .get(&id)
                .copied()
                .map(OutlinerPaint::Color)
                .unwrap_or(OutlinerPaint::Unpainted)
        } else {
            OutlinerPaint::Empty
        }
    }
    let mut colors = std::collections::BTreeMap::new();
    for id in model.objects.keys().filter(|id| roots.contains(id)) {
        visit(model, *id, &mut colors);
    }
    colors
}

// Include ancestors so a match remains reachable even inside collapsed objects.
// Filtering never changes the remembered disclosure states.
fn matching_outliner_objects(model: &Model, query: &str) -> Option<BTreeSet<u64>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    let mut visible = BTreeSet::new();
    for object in model.objects.values() {
        if !object.name.to_lowercase().contains(&query)
            && !format!("#{}", object.id).contains(&query)
        {
            continue;
        }
        let mut current = Some(object.id);
        while let Some(id) = current {
            if !visible.insert(id) {
                break;
            }
            current = model.parent(id);
        }
    }
    Some(visible)
}

fn empty_outliner_label(ui: &mut egui::Ui, icon: &str, label: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(18., 18.), Sense::hover());
        icons::line_icon(ui.painter(), rect.center(), icon, Palette::for_ui(ui).muted);
        ui.weak(label);
    });
}

impl Outliner<'_> {
    fn row(&mut self, ui: &mut egui::Ui, id: u64) -> egui::Response {
        let state = self.entities.get(id);
        let object = self.model.objects.get(&id);
        let paint = self
            .paint
            .get(&id)
            .copied()
            .or_else(|| {
                self.model
                    .face_colors
                    .get(&id)
                    .copied()
                    .map(OutlinerPaint::Color)
            })
            .unwrap_or(OutlinerPaint::Empty);
        let (kind, icon) = if let Some(object) = object {
            match object.kind {
                ObjectKind::Group => ("Group", "group"),
                ObjectKind::Component { .. } => ("Component", "component"),
            }
        } else if self.model.edges.contains_key(&id) {
            ("Edge", "edge")
        } else if self.model.faces.contains_key(&id) {
            ("Face", "face")
        } else if self.model.texts.contains_key(&id) {
            ("Text", "text")
        } else {
            ("Dimension", "dimension")
        };
        let mut label = object
            .map(|object| object.name.trim())
            .or_else(|| {
                self.model
                    .texts
                    .get(&id)
                    .map(|text| text.text.lines().next().unwrap_or("Text"))
            })
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("#{id}"));
        if state.locked {
            label.push_str(" · Locked");
        }
        if self.model.edit_context == Some(id) {
            label.push_str(" · Editing");
        }
        let hidden_color = Palette::for_ui(ui).muted.gamma_multiply(0.5);
        let text = RichText::new(&label);
        let text = if state.hidden {
            text.color(hidden_color)
        } else {
            text
        };
        let paint_color = match paint {
            OutlinerPaint::Color([r, g, b]) => Some(Color32::from_rgb(r, g, b)),
            _ => None,
        };
        let icon = if icon == "group" && paint_color.is_some() {
            "group_filled"
        } else {
            icon
        };
        let icon = egui::Atom::paint(egui::vec2(18., 18.), move |ui, args| {
            let outline = if state.hidden {
                hidden_color
            } else {
                args.fallback_text_color
            };
            let color = paint_color
                .map(|color| {
                    if state.hidden {
                        color.gamma_multiply(0.5)
                    } else {
                        color
                    }
                })
                .unwrap_or(outline);
            if icon == "face" && paint_color.is_some() {
                icons::face_icon(ui.painter(), args.rect.center(), color, outline);
            } else {
                icons::line_icon(ui.painter(), args.rect.center(), icon, color);
                if icon == "group_filled" {
                    icons::line_icon(ui.painter(), args.rect.center(), "group", outline);
                }
            }
        });
        let button = egui::Button::selectable(
            self.selected.contains(&id),
            (icon, text, egui::Atom::grow()),
        )
        .truncate()
        .sense(if object.is_some() && !state.locked {
            Sense::click_and_drag()
        } else {
            Sense::click()
        });
        // Keep the drag source ID stable while the hierarchy changes.
        let row_id = ui.make_persistent_id(("outliner_entity_row", id));
        let response = ui
            .scope_builder(egui::UiBuilder::new().scope_id(row_id), |ui| {
                ui.horizontal(|ui| {
                    let height = ui.spacing().interact_size.y;
                    let width = (ui.available_width() - 2. * (height + ui.spacing().item_spacing.x))
                        .max(0.);
                    let response = ui.add_sized(egui::vec2(width, height), button);
                    let material = self.materials.get(&id).copied().unwrap_or(OutlinerMaterial::None);
                    let (rect, indicator) = ui.allocate_exact_size(egui::vec2(height, height), Sense::hover());
                    let color = material.color(ui);
                    icons::line_icon(ui.painter(), rect.center(), material.icon(), color);
                    let material_label = format!("{} — {kind}: {label}", material.label());
                    indicator.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), &material_label));
                    indicator.on_hover_text(material_label);
                    let hidden = self.model.hidden.contains(&id);
                    let eye_hidden = state.hidden;
                    let eye = egui::Atom::paint(egui::vec2(18., 18.), move |ui, args| {
                        icons::line_icon(
                            ui.painter(),
                            args.rect.center(),
                            if eye_hidden { "eye_closed" } else { "eye" },
                            if eye_hidden {
                                hidden_color
                            } else {
                                args.fallback_text_color
                            },
                        );
                    });
                    let name = format!("{} {kind}: {label}", if hidden { "Show" } else { "Hide" });
                    let tooltip = if state.hidden && !hidden {
                        format!("{name}\nVisibility is also controlled by the parent object and tags")
                    } else {
                        name.clone()
                    };
                    if ui
                        .add_sized(egui::vec2(height, height), egui::Button::new(eye).frame(false))
                        .accessible_name(name)
                        .on_hover_text(tooltip)
                        .clicked()
                    {
                        self.toggle_visibility = Some(id);
                    }
                    response
                })
                .inner
            })
            .inner
            .accessible_name(format!("{kind}: {label}"))
            .on_hover_text(format!(
                "{kind}: {label}\nEntity #{id}{}\n{}",
                match paint {
                    OutlinerPaint::Color([r, g, b]) => format!("\nPaint: #{r:02X}{g:02X}{b:02X}"),
                    OutlinerPaint::Mixed => "\nPaint: Mixed face colors".to_owned(),
                    _ => String::new(),
                },
                if state.hidden {
                    "Click to select · Shift-click to select a range · Ctrl/Cmd-click to toggle selection · Use the eye button to toggle visibility; check parent objects and Tags for inherited visibility"
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
            if let Some(visible) = self.filtered_objects {
                if visible.contains(&id) {
                    self.row(ui, id);
                    ui.indent(("outliner_search_children", id), |ui| {
                        self.level(ui, &object.entities, Some(id));
                    });
                }
                continue;
            }
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
                    let icon = match object.kind {
                        ObjectKind::Group => "group",
                        ObjectKind::Component { .. } => "component",
                    };
                    empty_outliner_label(ui, icon, "Empty object");
                } else {
                    self.level(ui, &object.entities, Some(id));
                }
            });
            remember_section(ui.ctx(), state_id, &key);
        }
        if self.filtered_objects.is_some() {
            return;
        }
        let geometry: Vec<_> = ids
            .iter()
            .filter(|id| !model.objects.contains_key(id))
            .copied()
            .collect();
        if !geometry.is_empty() {
            let key = format!("outliner_geometry/{parent:?}");
            let open = restored_header_open(ui, &key, false);
            let mut label = egui::text::LayoutJob::simple(
                format!("Geometry ({})", geometry.len()),
                egui::TextStyle::Button.resolve(ui.style()),
                Color32::PLACEHOLDER,
                f32::INFINITY,
            );
            // Reserve space after the disclosure arrow for the geometry icon.
            label.sections[0].leading_space = 22.;
            let header = egui::CollapsingHeader::new(label)
                .id_salt(("outliner_geometry", parent))
                .open(open)
                .show(ui, |ui| {
                    // Virtualize geometry rows; imported meshes can contain millions of entities.
                    let height = ui.spacing().interact_size.y;
                    let scroll_key = format!("outliner_geometry_rows/{parent:?}");
                    let scroll = restored_scroll_area(
                        ui,
                        &scroll_key,
                        egui::ScrollArea::vertical()
                            .id_salt(("outliner_geometry_rows", parent))
                            .max_height(180.)
                            .auto_shrink([false, true]),
                    )
                    .show_rows(ui, height, geometry.len(), |ui, range| {
                        for index in range {
                            let id = geometry[index];
                            ui.push_id(id, |ui| {
                                self.row(ui, id);
                            });
                        }
                    });
                    remember_scroll(ui.ctx(), &scroll_key, scroll.state.clamped_offset());
                });
            icons::line_icon(
                ui.painter(),
                Pos2::new(
                    header.header_response.rect.left() + ui.spacing().indent + 9.,
                    header.header_response.rect.center().y,
                ),
                "geometry",
                ui.style().interact(&header.header_response).text_color(),
            );
            remember_section(ui.ctx(), header.header_response.id, &key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_summaries_survive_navigation_and_refresh_after_document_edits() {
        let (mut model, _, _) = paintable_model();
        let face = *model.faces.keys().next().unwrap();
        model.face_colors.insert(face, [10, 20, 30]);
        let mut snapshot = renderer::ModelSnapshot::default();
        let mut cache = OutlinerCache::default();
        let original = model.clone();
        for query in ["", "Painted", "missing", ""] {
            cache.update(&model, snapshot.update(&model), query);
            assert_eq!(cache.roots, model.root_entities());
            assert_eq!(cache.materials, outliner_materials(&model, &cache.roots));
            assert_eq!(cache.paint, outliner_paint(&model, &cache.roots));
            assert_eq!(
                cache.filtered_objects,
                matching_outliner_objects(&model, query)
            );
        }
        assert_eq!(cache.summary_builds, 1);
        let face = *model.faces.keys().next().unwrap();
        model.face_colors.insert(face, [150, 70, 30]);
        cache.update(&model, snapshot.update(&model), "");
        assert_eq!(cache.paint, outliner_paint(&model, &cache.roots));
        assert_eq!(cache.summary_builds, 2);
        model = original;
        cache.update(&model, snapshot.update(&model), "");
        assert_eq!(cache.paint, outliner_paint(&model, &cache.roots));
        assert_eq!(cache.summary_builds, 3);
    }

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

    fn paintable_model() -> (Model, u64, u64) {
        let mut model = Model::default();
        let points = [
            DVec3::ZERO,
            DVec3::X * 2.,
            DVec3::new(2., 2., 0.),
            DVec3::Y * 2.,
        ];
        for i in 0..4 {
            model.add_edge(points[i], points[(i + 1) % 4]).unwrap();
        }
        let face = *model.faces.keys().next().unwrap();
        model.push_pull(face, 2.).unwrap();
        model
            .make_object(&model.selectable_entities(), true, "Painted component")
            .unwrap();
        let group = model
            .make_object(&model.selectable_entities(), false, "Painted group")
            .unwrap();
        let component = *model.objects[&group].entities.first().unwrap();
        (model, group, component)
    }

    #[test]
    fn material_summary_tracks_partial_full_inherited_and_restored_assignments() {
        let (app, group, component, edge) = nested_model();
        let mut model = app.doc.model;
        let roots = model.root_entities();
        let loose_edge = model.add_edge(DVec3::Y, DVec3::Y + DVec3::X).unwrap();
        model
            .objects
            .get_mut(&group)
            .unwrap()
            .entities
            .insert(loose_edge);
        let stock = ketchup_core::StockMaterial {
            name: "Pine".into(),
            dimensions: [190, 890, 24000],
        };
        let material_id = ketchup_core::MaterialId::new_v4();
        model.stock_materials.insert(material_id, stock.clone());
        let before = model.clone();
        assert_eq!(
            outliner_materials(&model, &roots)[&group],
            OutlinerMaterial::None
        );
        model.objects.get_mut(&component).unwrap().stock_material_id = Some(material_id);
        model.hidden.insert(component);
        let partial = outliner_materials(&model, &roots);
        assert_eq!(partial[&group], OutlinerMaterial::Partial);
        assert_eq!(partial[&component], OutlinerMaterial::Full);
        assert_eq!(partial[&edge], OutlinerMaterial::Full);
        assert_eq!(partial[&loose_edge], OutlinerMaterial::None);

        // An assembly assignment covers its own geometry and unassigned children.
        model.objects.get_mut(&group).unwrap().stock_material = Some(stock);
        model.objects.get_mut(&component).unwrap().stock_material_id = None;
        let full = outliner_materials(&model, &roots);
        for id in [group, component, edge, loose_edge] {
            assert_eq!(full[&id], OutlinerMaterial::Full);
        }
        let restored = Document::from_json(
            &serde_json::to_string(&Document {
                model: model.clone(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(outliner_materials(&restored.model, &roots), full);
        let mut history = History::default();
        history.record("Assign material", before, &model);
        assert!(history.undo(&mut model));
        assert_eq!(
            outliner_materials(&model, &roots)[&group],
            OutlinerMaterial::None
        );
        assert!(history.redo(&mut model));
        assert_eq!(outliner_materials(&model, &roots), full);
    }

    #[test]
    fn material_icons_show_distinct_status_colors_and_filled_boards_in_both_themes() {
        let ctx = egui::Context::default();
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            ctx.set_theme(theme);
            for (status, filled_boards) in [
                (OutlinerMaterial::None, 0),
                (OutlinerMaterial::Partial, 1),
                (OutlinerMaterial::Full, 2),
            ] {
                let mut color = Color32::TRANSPARENT;
                let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    color = status.color(ui);
                    let colors = [
                        OutlinerMaterial::None.color(ui),
                        OutlinerMaterial::Partial.color(ui),
                        OutlinerMaterial::Full.color(ui),
                    ];
                    assert_ne!(colors[0], colors[1]);
                    assert_ne!(colors[1], colors[2]);
                    assert_ne!(colors[0], colors[2]);
                    icons::line_icon(ui.painter(), Pos2::new(30., 30.), status.icon(), color);
                });
                assert_eq!(output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == color)).count(), filled_boards);
            }
        }
    }

    #[test]
    fn group_paint_tracks_nested_faces_mixed_colors_and_undo() {
        let (mut model, group, component) = paintable_model();
        let roots = model.root_entities();
        let before = model.clone();
        let unpainted = outliner_paint(&model, &roots);
        for id in [group, component] {
            assert_eq!(unpainted[&id], OutlinerPaint::Unpainted);
        }
        let rgb = [31, 82, 149];
        model.paint_object(group, rgb).unwrap();
        let mut history = History::default();
        history.record("Paint group", before, &model);
        // Hidden descendants still belong to the object's material summary.
        model.hidden.insert(component);
        let painted = outliner_paint(&model, &roots);
        for id in [group, component] {
            assert_eq!(painted[&id], OutlinerPaint::Color(rgb));
        }
        assert!(history.undo(&mut model));
        assert_eq!(outliner_paint(&model, &roots), unpainted);
        assert!(history.redo(&mut model));
        assert_eq!(outliner_paint(&model, &roots), painted);
        let face = *model.faces.keys().next().unwrap();
        model.paint_face(face, [201, 91, 67]).unwrap();
        for id in [group, component] {
            assert_eq!(outliner_paint(&model, &roots)[&id], OutlinerPaint::Mixed);
        }
        model.face_colors.remove(&face);
        assert_eq!(outliner_paint(&model, &roots)[&group], OutlinerPaint::Mixed);
        let (app, edge_group, edge_component, _) = nested_model();
        let colors = outliner_paint(&app.doc.model, &app.doc.model.root_entities());
        assert_eq!(colors[&edge_group], OutlinerPaint::Empty);
        assert_eq!(colors[&edge_component], OutlinerPaint::Empty);
    }

    #[test]
    fn painted_outliner_icons_fill_folders_keep_selection_color_and_dim_when_hidden() {
        fn has_icon_color(shape: &egui::Shape, color: Color32, filled: bool) -> bool {
            match shape {
                egui::Shape::Rect(rect) => filled && rect.fill == color,
                egui::Shape::Path(path) => {
                    !filled && path.stroke.color == egui::epaint::ColorMode::Solid(color)
                }
                egui::Shape::Vec(shapes) => shapes
                    .iter()
                    .any(|shape| has_icon_color(shape, color, filled)),
                _ => false,
            }
        }
        let (mut model, group, component) = paintable_model();
        let rgb = [31, 82, 149];
        model.paint_object(group, rgb).unwrap();
        let mut app = KetchupApp::with_document(
            Document {
                model,
                ..Default::default()
            },
            None,
            true,
        );
        let ctx = egui::Context::default();
        ctx.memory_mut(|memory| memory.set_everything_is_visible(true));
        for id in [group, component] {
            app.outliner_filter = app.doc.model.objects[&id].name.clone();
            app.selected = BTreeSet::from([id]);
            for hidden in [false, true] {
                if hidden {
                    app.doc.model.hidden.insert(group);
                } else {
                    app.doc.model.hidden.remove(&group);
                }
                let mut output = None;
                for _ in 0..2 {
                    output = Some(ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                Pos2::ZERO,
                                egui::vec2(600., 900.),
                            )),
                            ..Default::default()
                        },
                        |ui| app.outliner_contents(ui),
                    ));
                }
                let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                let expected = if hidden {
                    color.gamma_multiply(0.5)
                } else {
                    color
                };
                assert!(output.unwrap().shapes.iter().any(|shape| has_icon_color(
                    &shape.shape,
                    expected,
                    id == group
                )));
            }
        }
    }

    #[test]
    fn search_finds_nested_objects_and_keeps_only_matches_and_ancestors() {
        let (mut app, group, component, _) = nested_model();
        let model = &mut app.doc.model;
        let edge = model.add_edge(DVec3::Y, DVec3::Y + DVec3::X).unwrap();
        model
            .make_object(&BTreeSet::from([edge]), false, "Unrelated")
            .unwrap();
        assert_eq!(
            matching_outliner_objects(model, "  hAnDlE  "),
            Some(BTreeSet::from([group, component]))
        );
        assert_eq!(
            matching_outliner_objects(model, &format!("#{component}")),
            Some(BTreeSet::from([group, component]))
        );
        assert_eq!(
            matching_outliner_objects(model, "Cabinet"),
            Some(BTreeSet::from([group]))
        );
        assert_eq!(
            matching_outliner_objects(model, "missing"),
            Some(BTreeSet::new())
        );
        assert_eq!(matching_outliner_objects(model, "  "), None);
    }

    #[test]
    fn search_reveals_collapsed_matches_without_changing_disclosure_preferences() {
        fn labels(app: &mut KetchupApp, ctx: &egui::Context) -> Vec<String> {
            fn collect(shape: &egui::Shape, labels: &mut Vec<String>) {
                match shape {
                    egui::Shape::Text(text) => labels.push(text.galley.job.text.clone()),
                    egui::Shape::Vec(shapes) => {
                        for shape in shapes {
                            collect(shape, labels);
                        }
                    }
                    _ => {}
                }
            }
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600., 900.))),
                    ..Default::default()
                },
                |ui| app.outliner_contents(ui),
            );
            let mut labels = Vec::new();
            for shape in &output.shapes {
                collect(&shape.shape, &mut labels);
            }
            labels
        }

        let (mut app, group, component, _) = nested_model();
        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        set_section_open(&ctx, &format!("outliner_object/{group}"), false);
        set_section_open(&ctx, &format!("outliner_object/{component}"), true);
        let before = ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<SectionMemory>(section_memory_id())
                .0
                .clone()
        });
        app.outliner_filter = "handle".into();
        let rendered = labels(&mut app, &ctx);
        assert!(rendered.contains(&"Cabinet".into()));
        assert!(rendered.contains(&"Handle".into()));
        assert!(!rendered.iter().any(|label| label.starts_with("Geometry (")));
        let after = ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<SectionMemory>(section_memory_id())
                .0
                .clone()
        });
        assert_eq!(before, after);

        app.outliner_filter = "missing".into();
        let rendered = labels(&mut app, &ctx);
        assert!(rendered.contains(&"No matching objects".into()));
        assert!(!rendered.contains(&"Cabinet".into()));

        app.outliner_filter.clear();
        let rendered = labels(&mut app, &ctx);
        assert!(rendered.contains(&"Cabinet".into()));
        assert!(!rendered.contains(&"Handle".into()));
    }

    #[test]
    fn search_ranges_skip_filtered_out_siblings() {
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let ids: Vec<_> = ["Part A", "Other", "Part B"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                let start = DVec3::Y * index as f64;
                let edge = app.doc.model.add_edge(start, start + DVec3::X).unwrap();
                app.doc
                    .model
                    .make_object(&BTreeSet::from([edge]), false, name)
                    .unwrap()
            })
            .collect();
        app.outliner_filter = "part".into();
        assert_eq!(
            app.outliner_range(ids[0], ids[2]),
            BTreeSet::from([ids[0], ids[2]])
        );
    }

    #[test]
    fn child_expansion_updates_unrendered_descendants_without_touching_other_sections() {
        let (mut app, group, component, _) = nested_model();
        let ctx = egui::Context::default();
        app.selected = BTreeSet::from([group, component]);
        set_section_open(&ctx, "outliner_geometry/None", false);
        set_section_open(&ctx, "outliner_object/9999", true);
        for open in [true, false] {
            app.set_outliner_children_open(&ctx, open);
            let sections = ctx.data_mut(|data| {
                data.get_temp_mut_or_default::<SectionMemory>(section_memory_id())
                    .0
                    .clone()
            });
            for id in [group, component] {
                assert_eq!(sections[&format!("outliner_object/{id}")], open);
            }
            assert_eq!(
                sections[&format!("outliner_geometry/{:?}", Some(component))],
                open
            );
            assert!(!sections["outliner_geometry/None"]);
            assert!(sections["outliner_object/9999"]);
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
        assert!(labels.contains(&"Cabinet · Locked".into()));
        assert!(labels.contains(&"Handle · Locked".into()));
        assert!(labels.contains(&"Geometry (1)".into()));
        assert!(labels.contains(&format!("#{edge} · Locked")));
        assert!(labels.iter().all(|label| !label.contains(" · Hidden")));
    }

    #[test]
    fn visibility_button_is_right_aligned_and_toggles_only_its_entity_with_undo() {
        fn frame(
            app: &mut KetchupApp,
            ctx: &egui::Context,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600., 900.))),
                    events,
                    ..Default::default()
                },
                |ui| app.outliner_contents(ui),
            )
        }
        fn bounds(output: &egui::FullOutput, name: &str) -> Rect {
            let node = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(name))
                .unwrap();
            let rect = node.1.bounds().unwrap();
            Rect::from_min_max(
                Pos2::new(rect.x0 as f32, rect.y0 as f32),
                Pos2::new(rect.x1 as f32, rect.y1 as f32),
            )
        }

        let (mut app, group, component, _) = nested_model();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        set_section_open(&ctx, &format!("outliner_object/{group}"), true);
        app.selected.insert(group);
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let eye = bounds(&output, "Hide Component: Handle");
        let parent_eye = bounds(&output, "Hide Group: Cabinet");
        let row = bounds(&output, "Component: Handle");
        let material = bounds(&output, "No material assigned — Component: Handle");
        let parent_material = bounds(&output, "No material assigned — Group: Cabinet");
        assert!(material.left() > row.right());
        assert!(eye.left() > material.right());
        assert!((material.right() - parent_material.right()).abs() < 1.);
        assert!((eye.right() - parent_eye.right()).abs() < 1.);

        for name in ["Show Component: Handle", "Hide Component: Handle"] {
            for pressed in [true, false] {
                frame(
                    &mut app,
                    &ctx,
                    vec![
                        egui::Event::PointerMoved(eye.center()),
                        egui::Event::PointerButton {
                            pos: eye.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            let output = frame(&mut app, &ctx, vec![]);
            bounds(&output, name);
            assert_eq!(
                app.doc.model.hidden.contains(&component),
                name.starts_with("Show")
            );
            assert!(!app.doc.model.hidden.contains(&group));
            assert_eq!(app.selected, BTreeSet::from([group]));
            assert_eq!(app.doc.model.edit_context, None);
        }
        assert_eq!(app.history.undo_label(), Some("Unhide"));
        app.undo(&ctx, false);
        assert!(app.doc.model.hidden.contains(&component));
        let restored = Document::from_json(&serde_json::to_string(&app.doc).unwrap()).unwrap();
        assert!(restored.model.hidden.contains(&component));
        app.undo(&ctx, true);
        assert!(!app.doc.model.hidden.contains(&component));
    }
}

impl KetchupApp {
    pub(super) fn outliner_contents(&mut self, ui: &mut egui::Ui) {
        let search = ui
            .add(
                egui::TextEdit::singleline(&mut self.outliner_filter)
                    .id(egui::Id::unique(OUTLINER_SEARCH_ID))
                    .prefix(egui::Atom::paint(egui::vec2(18., 18.), |ui, args| {
                        icons::line_icon(
                            ui.painter(),
                            args.rect.center(),
                            "search",
                            Palette::for_ui(ui).muted,
                        );
                    }))
                    .hint_text("Search objects")
                    .desired_width(f32::INFINITY),
            )
            .accessible_name("Search objects")
            .on_hover_text("Find groups and components at any depth by name or #ID · Ctrl/Cmd+F");
        if self.focus_outliner_search {
            search.request_focus();
            search.scroll_to_me(Some(egui::Align::Center));
            self.focus_outliner_search = false;
        }
        let revision = self.render_model.update(&self.doc.model);
        self.outliner_cache
            .update(&self.doc.model, revision, &self.outliner_filter);
        let roots = &self.outliner_cache.roots;
        if roots.is_empty() {
            empty_outliner_label(ui, "geometry", "No objects or geometry yet");
            return;
        }
        let filtered_objects = &self.outliner_cache.filtered_objects;
        if filtered_objects.as_ref().is_some_and(BTreeSet::is_empty) {
            empty_outliner_label(ui, "outliner", "No matching objects");
            return;
        }
        self.entity_cache.update(&self.doc.model);
        let hover_id = ui.scope_id().with("outliner_drag_hover");
        let mut tree = Outliner {
            model: &self.doc.model,
            materials: &self.outliner_cache.materials,
            paint: &self.outliner_cache.paint,
            filtered_objects: filtered_objects.as_ref(),
            selected: &self.selected,
            entities: &self.entity_cache,
            action: None,
            toggle_visibility: None,
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
        let scroll = restored_scroll_area(
            ui,
            "outliner_tree",
            egui::ScrollArea::horizontal()
                .id_salt("outliner_tree")
                .auto_shrink([false, true]),
        )
        .show(ui, |ui| {
            ui.spacing_mut().indent = 16.;
            tree.level(ui, roots, None);
            tree.root_drop_target(ui);
        });
        remember_scroll(ui.ctx(), "outliner_tree", scroll.state.clamped_offset());
        let Outliner {
            action,
            toggle_visibility,
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
                ui.scope_id().with("outliner_drag_label"),
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
        if let Some(id) = toggle_visibility {
            self.toggle_outliner_visibility(id, ui.ctx());
        }
        if let Some((id, response)) = context_menu {
            if response.secondary_clicked() {
                if !self.prepare_outliner_selection(id, ui.ctx()) {
                    return;
                }
                self.select_context_target(Some(id));
                self.context_position = None;
            }
            self.context_menu_with(&response, |app, ui| {
                let has_children = app.selected.iter().any(|id| {
                    app.doc
                        .model
                        .objects
                        .get(id)
                        .is_some_and(|object| !object.entities.is_empty())
                });
                ui.separator();
                for (label, open) in [("Expand Children", true), ("Collapse Children", false)] {
                    if ui
                        .add_enabled(has_children, egui::Button::new(label))
                        .clicked()
                    {
                        app.set_outliner_children_open(ui.ctx(), open);
                        ui.close();
                    }
                }
            });
        }
    }

    fn toggle_outliner_visibility(&mut self, id: u64, ctx: &egui::Context) {
        if !self.doc.model.contains_entity(&id) {
            return;
        }
        let before = self.doc.model.clone();
        let show = self.doc.model.hidden.remove(&id);
        if !show {
            self.doc.model.hidden.insert(id);
        }
        // A child can be toggled without entering its object. Propagate changes
        // within component definitions just as the shared Hide/Unhide commands do.
        let context = self.doc.model.edit_context;
        self.doc.model.edit_context = self.doc.model.parent(id);
        let result = self.doc.model.synchronize_edit_context();
        self.doc.model.edit_context = context;
        if let Err(message) = result {
            self.doc.model = before;
            self.say(ctx, message);
            return;
        }
        self.finish_edit(if show { "Unhide" } else { "Hide" }, before, ctx);
        if context.is_some_and(|id| self.doc.model.is_hidden(id)) {
            self.navigate_context(None, ctx);
        }
        self.hover = None;
        self.snap = None;
        self.outliner_hover.clear();
    }

    fn set_outliner_children_open(&self, ctx: &egui::Context, open: bool) {
        let mut pending: Vec<_> = self.selected.iter().copied().collect();
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            let Some(object) = self.doc.model.objects.get(&id) else {
                continue;
            };
            if !visited.insert(id) {
                continue;
            }
            set_section_open(ctx, &format!("outliner_object/{id}"), open);
            if object
                .entities
                .iter()
                .any(|id| !self.doc.model.objects.contains_key(id))
            {
                set_section_open(ctx, &format!("outliner_geometry/{:?}", Some(id)), open);
            }
            // Walk objects only; large meshes need no per-entity expansion state.
            pending.extend(
                object
                    .entities
                    .iter()
                    .filter(|id| self.doc.model.objects.contains_key(id))
                    .copied(),
            );
        }
        ctx.request_repaint();
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
        if let Some(visible) = matching_outliner_objects(model, &self.outliner_filter) {
            order.retain(|id| visible.contains(id));
        }
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
