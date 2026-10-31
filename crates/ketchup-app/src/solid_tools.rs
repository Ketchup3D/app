use super::*;
use ketchup_core::SolidOperation;

// Solid volume imports and unions object geometry. Reuse both successful and
// failed measurements across redraws and selection changes. Model collections
// permit direct edits, so compare a snapshot to catch edits, undo and project
// changes even when entity IDs are reused.
#[derive(Default)]
pub(super) struct VolumeCache {
    source: Option<Model>,
    volumes: std::collections::BTreeMap<u64, Result<f64, &'static str>>,
    selection_size: Option<(BTreeSet<u64>, Option<DVec3>)>,
    #[cfg(test)]
    pub(super) computations: usize,
}

impl VolumeCache {
    fn update(&mut self, model: &Model) {
        if self.source.as_ref() != Some(model) {
            self.source = Some(model.clone());
            self.volumes.clear();
            self.selection_size = None;
        }
    }

    pub(super) fn volume(&mut self, model: &Model, id: u64) -> Result<f64, &'static str> {
        self.update(model);
        *self.volumes.entry(id).or_insert_with(|| {
            #[cfg(test)]
            {
                self.computations += 1;
            }
            model.solid_volume(id)
        })
    }

    pub(super) fn solid_selection_size(
        &mut self,
        model: &Model,
        selected: &BTreeSet<u64>,
    ) -> Option<DVec3> {
        self.update(model);
        if let Some((previous, size)) = &self.selection_size
            && previous == selected
        {
            return *size;
        }
        // Restrict validation to the selection so selecting one face does not
        // silently measure the rest of an unselected connected body.
        let copy = model.copy_entities(selected);
        let mut bounds = None;
        collect_solid_bounds(&copy, &mut bounds);
        let size = bounds.map(|(min, max)| max - min);
        self.selection_size = Some((selected.clone(), size));
        size
    }
}

fn collect_solid_bounds(model: &Model, bounds: &mut Option<(DVec3, DVec3)>) {
    let mut visited = BTreeSet::new();
    for id in model.root_entities() {
        if visited.contains(&id)
            || !(model.objects.contains_key(&id) || model.faces.contains_key(&id))
        {
            continue;
        }
        let Ok(body) = model.solid_entities(id) else {
            continue;
        };
        visited.extend(model.expand_entities(&body));
        if model.solid_volume(id).is_ok_and(|volume| volume > 0.) {
            for vertex in model.selection_vertices(&body) {
                let point = model.vertices[&vertex].position;
                *bounds = Some(match *bounds {
                    Some((min, max)) => (min.min(point), max.max(point)),
                    None => (point, point),
                });
            }
        } else if let Some(contents) = model.object_contents(id) {
            // Assemblies can contain both solid parts and non-solid items.
            // Rejecting the assembly must not discard its valid child solids.
            collect_solid_bounds(&contents, bounds);
        }
    }
}

fn operation(tool: Tool) -> Option<SolidOperation> {
    Some(match tool {
        Tool::OuterShell => SolidOperation::OuterShell,
        Tool::Union => SolidOperation::Union,
        Tool::Intersect => SolidOperation::Intersect,
        Tool::Trim => SolidOperation::Trim,
        Tool::Split => SolidOperation::Split,
        _ => return None,
    })
}
impl KetchupApp {
    pub(super) fn start_solid_tool(&mut self, tool: Tool, ctx: &egui::Context) {
        self.set_tool(tool);
        self.notice = None;
        if tool == Tool::Flip {
            return;
        }
        let mut operands: Vec<(u64, BTreeSet<u64>)> = vec![];
        for &id in &self.selected {
            if let Ok(ids) = self.doc.model.solid_entities(id) {
                if !operands.iter().any(|(_, previous)| *previous == ids) {
                    operands.push((id, ids));
                }
            }
        }
        if operands.len() == 2 && tool != Tool::Trim {
            self.apply_solid(operands[0].0, operands[1].0, ctx);
        } else if operands.len() == 1 {
            let first = operands[0].0;
            match self
                .doc
                .model
                .validate_solid_operand(first, operation(tool).unwrap())
            {
                Ok(_) if !self.doc.model.selection_locked(&operands[0].1) => {
                    self.solid_first = Some(first);
                    self.selected = operands.remove(0).1;
                }
                Err(message) => self.say(ctx, message),
                _ => self.say(ctx, "Unlock the body before editing it"),
            }
        }
    }
    fn apply_solid(&mut self, first: u64, second: u64, ctx: &egui::Context) {
        let Some(operation) = operation(self.tool) else {
            return;
        };
        let before = self.doc.model.clone();
        match self.doc.model.boolean_solids(first, second, operation) {
            Ok(result) => {
                self.solid_first = None;
                self.selected = result;
                self.finish_edit(operation.label(), before, ctx);
                if self.selected.is_empty() {
                    self.say(ctx, "The target was entirely removed by the cutter");
                }
            }
            Err(message) => self.say(ctx, message),
        }
    }
    pub(super) fn apply_flip(&mut self, axis: Axis, ctx: &egui::Context) {
        let before = self.doc.model.clone();
        match self.doc.model.flip_selection(&self.selected, axis) {
            Ok(()) => self.finish_edit(
                match axis {
                    Axis::X => "Flip X",
                    Axis::Y => "Flip Y",
                    Axis::Z => "Flip Z",
                },
                before,
                ctx,
            ),
            Err(message) => self.say(ctx, message),
        }
    }
    pub(super) fn solid_click(&mut self, pos: Pos2, ctx: &egui::Context) {
        if self.tool == Tool::Flip && !self.selected.is_empty() {
            if let Some(axis) = self.flip_plane_at(pos) {
                self.apply_flip(axis, ctx);
                return;
            }
        }
        let Some(hit) = selection::pick_entity(
            &self.doc.model,
            &self.doc.camera,
            dv(self.viewport.size()),
            dv(pos - self.viewport.min),
            8.,
        ) else {
            self.say(ctx, "Click a group, component, or connected body");
            return;
        };
        let ids = match self.doc.model.solid_entities(hit) {
            Ok(ids) => ids,
            Err(message) => {
                self.say(ctx, message);
                return;
            }
        };
        if self.doc.model.selection_locked(&ids) {
            self.say(ctx, "Unlock the body before editing it");
            return;
        }
        if self.tool == Tool::Flip {
            self.selected = ids;
            return;
        }
        if let Some(first) = self.solid_first {
            self.apply_solid(first, hit, ctx);
        } else {
            match self
                .doc
                .model
                .validate_solid_operand(hit, operation(self.tool).unwrap())
            {
                Ok(_) => {
                    self.solid_first = Some(hit);
                    self.selected = ids;
                }
                Err(message) => self.say(ctx, message),
            }
        }
    }
    pub(super) fn solid_status(&self) -> &'static str {
        if self.solid_first.is_some() {
            if self.tool == Tool::Trim {
                "Cutter selected · Click the target solid to trim · Esc to cancel"
            } else if self.tool == Tool::Intersect {
                "First body selected · Click the second body or face · Esc to cancel"
            } else {
                "First body selected · Click the second body · Esc to cancel"
            }
        } else {
            self.tool.instructions()
        }
    }
    pub(super) fn solid_inspector(&mut self, ui: &mut egui::Ui) {
        if !self.tool.is_solid() {
            return;
        }
        ui.heading(self.tool.label());
        ui.label(
            RichText::new(self.solid_status())
                .small()
                .color(Palette::for_ui(ui).muted),
        );
        if self.tool == Tool::Flip {
            ui.add_space(8.);
            ui.label("Mirror around the selection center:");
            ui.horizontal(|ui| {
                for (axis, label) in [
                    (Axis::X, "X / red"),
                    (Axis::Y, "Y / green"),
                    (Axis::Z, "Z / blue"),
                ] {
                    if ui
                        .add_enabled(
                            !self.selected.is_empty(),
                            egui::Button::new(RichText::new(label).color(self.axis_color(axis))),
                        )
                        .clicked()
                    {
                        self.apply_flip(axis, ui.ctx());
                    }
                }
            });
        } else if let Some(first) = self.solid_first {
            ui.add_space(8.);
            let name = self
                .doc
                .model
                .objects
                .get(&first)
                .map(|o| o.name.as_str())
                .unwrap_or("Connected body");
            ui.label(RichText::new(name).strong());
            if let Ok(volume) = self.solid_volume_cache.volume(&self.doc.model, first) {
                Self::property(ui, "Volume", &self.doc.display.units.format_volume(volume));
            }
            if ui.button("Choose first body again").clicked() {
                self.solid_first = None;
                self.selected.clear();
            }
        } else {
            ui.add_space(8.);
            ui.label(
                "Choose groups, components, or connected bodies. Results are editable groups.",
            );
            if self.tool == Tool::Intersect {
                ui.label("Intersect also keeps the portion of a face inside a closed body.");
            } else {
                ui.label("Use closed bodies with no stray edges or missing faces.");
            }
            if self.tool == Tool::Trim {
                ui.label("The first click chooses the cutter. It stays in the model.");
            }
        }
        ui.add_space(14.);
    }
    fn flip_planes(&self) -> Vec<(Axis, [DVec3; 4])> {
        let Some(center) = self.doc.model.selection_center(&self.selected) else {
            return vec![];
        };
        let ids = self.doc.model.selection_vertices(&self.selected);
        let extent = ids
            .iter()
            .map(|id| self.doc.model.vertices[id].position.distance(center))
            .fold(self.doc.camera.distance * 0.04, f64::max)
            * 0.8;
        [Axis::X, Axis::Y, Axis::Z]
            .into_iter()
            .map(|axis| {
                let (u, v) = match axis {
                    Axis::X => (DVec3::Y, DVec3::Z),
                    Axis::Y => (DVec3::X, DVec3::Z),
                    Axis::Z => (DVec3::X, DVec3::Y),
                };
                (
                    axis,
                    [
                        center - u * extent - v * extent,
                        center + u * extent - v * extent,
                        center + u * extent + v * extent,
                        center - u * extent + v * extent,
                    ],
                )
            })
            .collect()
    }
    pub(super) fn flip_plane_at(&self, pos: Pos2) -> Option<Axis> {
        let ray = self
            .doc
            .camera
            .ray(dv(pos - self.viewport.min), dv(self.viewport.size()));
        self.flip_planes()
            .into_iter()
            .filter_map(|(axis, quad)| {
                let denominator = ray.direction.dot(axis.vector());
                if denominator.abs() < 1e-8 {
                    return None;
                }
                let t = (quad[0] - ray.origin).dot(axis.vector()) / denominator;
                if t <= 0. {
                    return None;
                }
                let point = ray.origin + ray.direction * t;
                let (u, v) = (quad[1] - quad[0], quad[3] - quad[0]);
                let (a, b) = (
                    (point - quad[0]).dot(u) / u.length_squared(),
                    (point - quad[0]).dot(v) / v.length_squared(),
                );
                ((0. ..=1.).contains(&a) && (0. ..=1.).contains(&b)).then_some((axis, t))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(axis, _)| axis)
    }
    pub(super) fn paint_solid_tools(&self, p: &egui::Painter, rect: Rect) {
        if self.tool != Tool::Flip {
            return;
        }
        let size = dv(rect.size());
        let mut planes = self.flip_planes();
        planes.sort_by(|a, b| {
            let depth = |q: &[DVec3; 4]| {
                q.iter()
                    .map(|p| p.distance_squared(self.doc.camera.eye()))
                    .sum::<f64>()
            };
            depth(&b.1).total_cmp(&depth(&a.1))
        });
        for (axis, quad) in planes {
            let screen: Option<Vec<_>> = quad
                .iter()
                .map(|pt| self.doc.camera.project(*pt, size).map(|v| rect.min + pv(v)))
                .collect();
            if let Some(points) = screen {
                let color = self.axis_color(axis);
                let alpha = if self.flip_hover == Some(axis) {
                    65
                } else {
                    24
                };
                p.add(egui::Shape::convex_polygon(
                    points.clone(),
                    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha),
                    Stroke::new(
                        if self.flip_hover == Some(axis) {
                            2.
                        } else {
                            1.
                        },
                        color,
                    ),
                ));
                let label = match axis {
                    Axis::X => "Flip X",
                    Axis::Y => "Flip Y",
                    Axis::Z => "Flip Z",
                };
                p.text(
                    points[2],
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(12.),
                    color,
                );
            }
        }
    }
}
