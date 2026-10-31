//! Finishing estimates use actual model surfaces and reusable, user-adjustable rates.
use super::*;
use crate::inspector_table::{Column, InspectorTable, product_cell_text};
use ketchup_core::Model;
use std::collections::BTreeSet;

// Research checked 2026-10-29. These are planning assumptions, not universal benchmarks.
// Sanding: Teri Masaschi, 8–10 seconds/foot of travel:
// https://www.finewoodworking.com/project-guides/finishing/are-you-sanding-right
// Assume a 5-inch pad, 50% overlap and two traversals per grit:
// 2 / ((0.3048 / 9) * (0.127 / 2)) / 60 = 15.5 min/m², rounded to 16.
// Resene's stain-only allowance is 0.08 hours/m² = 4.8 min/m² (rounded to 5).
// Its three-coat polyurethane floor system is 0.29 hours/m² = 5.8 min/m²/coat
// (rounded to 6). This is a flat-surface proxy, not a furniture time study.
// Conditioner uses the stain apply/wipe rate as an explicit proxy.
// https://www.resene.com.au/paintdec/productivity-tables/index.htm
// Oil-based Minwax conditioner: 5–15 min penetration, stain within 2 hours.
// https://www.minwax.com/en/products/preparation/pre-stain-wood-conditioner
// PPG/Olympic penetrating oil-based stain: 8 hours minimum before topcoat.
// Faster formulations differ, so the user's label takes precedence.
// https://buyat.ppg.com/rep_pafpainttools_files/Olympic/TDB/626501.pdf
// Oil-based polyurethane: 4–6 hours between coats, 24 hours after the last
// coat before light use. Light use does not mean fully cured.
// https://www.minwax.com/en/products/protective-finishes/fast-drying-polyurethane
const GRITS: [u16; 6] = [80, 120, 150, 180, 220, 320];
type WoodMaterials =
    std::collections::BTreeMap<ketchup_core::MaterialId, ketchup_core::StockMaterial>;

pub(super) use ketchup_core::surface_treatment::{Duration, Settings, Step};

// Model collections allow direct edits, so compare a source snapshot rather than
// relying on edit notifications. Scrolling only compares inputs; it never repeats
// face triangulation, solid measurement, or stock packing. Prices and units stay
// outside this cache so changing them updates the UI immediately.
#[derive(Default)]
pub(super) struct EstimateCache {
    source: Option<(Model, BTreeSet<u64>)>,
    area: Option<(WoodMaterials, std::collections::BTreeMap<u64, f64>)>,
    materials: Option<(
        crate::cutting_layout::Settings,
        Result<MaterialEstimate, String>,
    )>,
    #[cfg(test)]
    material_recomputations: usize,
}

struct MaterialEstimate {
    requirements: Vec<crate::cutting_plan::MaterialRequirement>,
    skipped: usize,
}

impl EstimateCache {
    fn validate(&mut self, model: &Model, selected: &BTreeSet<u64>) {
        if self
            .source
            .as_ref()
            .is_none_or(|(source, selection)| source != model || selection != selected)
        {
            self.source = Some((model.clone(), selected.clone()));
            self.area = None;
            self.materials = None;
        }
    }

    fn area(
        &mut self,
        model: &Model,
        selected: &BTreeSet<u64>,
        catalog: &stock_settings::Settings,
    ) -> f64 {
        self.validate(model, selected);
        let wood: WoodMaterials = catalog
            .wood
            .iter()
            .map(|stock| (stock.id, stock.definition()))
            .collect();
        if self
            .area
            .as_ref()
            .is_none_or(|(previous, _)| *previous != wood)
        {
            let area = treatment_areas(model, selected, &wood);
            self.area = Some((wood, area));
        }
        self.area.as_ref().unwrap().1.values().sum()
    }

    fn materials(
        &mut self,
        model: &Model,
        selected: &BTreeSet<u64>,
        layout: crate::cutting_layout::Settings,
    ) -> &Result<MaterialEstimate, String> {
        self.validate(model, selected);
        if self
            .materials
            .as_ref()
            .is_none_or(|(previous, _)| *previous != layout)
        {
            #[cfg(test)]
            {
                self.material_recomputations += 1;
            }
            self.materials = Some((
                layout,
                crate::cutting_plan::CuttingPlan::collect_assigned(model, selected).map(|plan| {
                    MaterialEstimate {
                        requirements: plan.material_requirements(layout),
                        skipped: plan.skipped,
                    }
                }),
            ));
        }
        &self.materials.as_ref().unwrap().1
    }
}

fn object_has_wood_material(
    model: &Model,
    mut id: u64,
    parents: &std::collections::BTreeMap<u64, u64>,
    wood: &WoodMaterials,
) -> bool {
    loop {
        let object = &model.objects[&id];
        if let Some(material) = model.object_stock_material(object) {
            return if let Some(id) = object.stock_material_id {
                wood.contains_key(&id)
            } else {
                wood.values().any(|stock| stock == material)
            };
        }
        let Some(parent) = parents.get(&id) else {
            return false;
        };
        id = *parent;
    }
}

// Attribute each face once to its nearest object. Material inheritance still follows
// the nearest assigned ancestor, so a child's material overrides its parent's.
fn treatment_areas(
    model: &Model,
    selected: &BTreeSet<u64>,
    wood: &WoodMaterials,
) -> std::collections::BTreeMap<u64, f64> {
    let expanded = model.expand_entities(selected);
    let parents: std::collections::BTreeMap<_, _> = model
        .objects
        .values()
        .flat_map(|object| object.entities.iter().map(move |id| (*id, object.id)))
        .collect();
    let mut areas = std::collections::BTreeMap::new();
    for face in model.faces.values().filter(|face| {
        !model.is_hidden(face.id) && (selected.is_empty() || expanded.contains(&face.id))
    }) {
        let Some(owner) = parents.get(&face.id).copied() else {
            continue;
        };
        let area = model.face_area(face);
        if object_has_wood_material(model, owner, &parents, wood) && area.is_finite() && area > 0. {
            *areas.entry(owner).or_default() += area;
        }
    }
    areas
}

// Apply only edited fields when a selection contains different configurations.
fn apply_treatment_changes(target: &mut Settings, before: &Settings, after: &Settings) {
    fn step(target: &mut Step, before: &Step, after: &Step) {
        macro_rules! apply { ($($field:ident),*) => { $(
            if before.$field != after.$field { target.$field = after.$field; }
        )* }; }
        apply!(
            enabled,
            product_id,
            minutes_per_m2,
            coats,
            drying_hours,
            final_drying_hours
        );
    }
    for i in 0..6 {
        step(
            &mut target.sanding[i],
            &before.sanding[i],
            &after.sanding[i],
        );
        if before.sanding_after_varnish[i] != after.sanding_after_varnish[i] {
            target.sanding_after_varnish[i] = after.sanding_after_varnish[i];
        }
    }
    step(&mut target.pre_stain, &before.pre_stain, &after.pre_stain);
    step(&mut target.stain, &before.stain, &after.stain);
    step(&mut target.varnish, &before.varnish, &after.varnish);
    target.sanitize();
}

fn object_tasks(
    model: &Model,
    areas: &std::collections::BTreeMap<u64, f64>,
    defaults: &Settings,
) -> Vec<(String, Duration, usize)> {
    let mut groups = std::collections::BTreeMap::<String, (Duration, usize)>::new();
    for (id, area) in areas {
        let mut settings = model.objects[id]
            .surface_treatment_override()
            .unwrap_or(defaults)
            .clone();
        settings.sanitize();
        for (title, duration) in settings.task_estimates(*area) {
            let (total, count) = groups.entry(title).or_default();
            total.work += duration.work;
            // Objects dry together after each coat; wait for the slowest one.
            total.wait = total.wait.max(duration.wait);
            *count += 1;
        }
    }
    let mut tasks: Vec<_> = groups
        .into_iter()
        .map(|(title, (duration, count))| (title, duration, count))
        .collect();
    // Keep the treatment sequence, including sanding between individual coats.
    tasks.sort_by_key(|(title, _, _)| {
        let number = |text: &str| {
            text.split(|c: char| !c.is_ascii_digit())
                .find(|part| !part.is_empty())
                .and_then(|part| part.parse::<u32>().ok())
                .unwrap_or(0)
        };
        if title.starts_with("Sanding") {
            if let Some((_, coat)) = title.split_once("after coat ") {
                (3, number(coat), number(title))
            } else {
                (0, 0, number(title))
            }
        } else if title.starts_with("Pre-stain") {
            (1, number(title), 0)
        } else if title.starts_with("Stain") {
            (2, number(title), 0)
        } else {
            (3, number(title), 0)
        }
    });
    tasks
}

#[derive(Debug)]
struct FinishCost {
    name: String,
    kind: Option<finish_settings::FinishType>,
    containers: Option<f64>,
    price: Option<f64>,
    amount: Option<f64>,
    issue: Option<&'static str>,
}

fn finishing_costs(
    model: &Model,
    areas: &std::collections::BTreeMap<u64, f64>,
    defaults: &Settings,
    products: &[finish_settings::Finish],
) -> Vec<FinishCost> {
    let mut demand = std::collections::BTreeMap::<Option<ketchup_core::MaterialId>, f64>::new();
    for (id, area) in areas {
        if !area.is_finite() || *area <= 0. {
            continue;
        }
        let mut settings = model.objects[id]
            .surface_treatment_override()
            .unwrap_or(defaults)
            .clone();
        settings.sanitize();
        for step in [&settings.pre_stain, &settings.stain, &settings.varnish] {
            if step.enabled {
                *demand.entry(step.product_id).or_default() += area * f64::from(step.coats);
            }
        }
    }
    demand
        .into_iter()
        .map(|(id, area)| {
            let product = id.and_then(|id| products.iter().find(|product| product.id == id));
            let Some(product) = product else {
                return FinishCost {
                    name: if id.is_some() {
                        "Unavailable finishing product"
                    } else {
                        "Unassigned finishing product"
                    }
                    .into(),
                    kind: None,
                    containers: None,
                    price: None,
                    amount: None,
                    issue: Some("Choose a product for each enabled finishing step."),
                };
            };
            // Coverage is specified for the exact configured container, regardless of
            // its volume or mass unit. Never convert mass to volume or assume density.
            let containers = product
                .coverage_m2_per_container
                .filter(|coverage| coverage.is_finite() && *coverage > 0.)
                .map(|coverage| (area / coverage).ceil())
                .filter(|quantity| quantity.is_finite() && *quantity > 0.);
            let price = (product.price.is_finite() && product.price > 0.).then_some(product.price);
            let amount = containers
                .zip(price)
                .map(|(quantity, price)| quantity * price)
                .filter(|amount| amount.is_finite());
            FinishCost {
                name: product.name.clone(),
                kind: Some(product.finish_type),
                containers,
                price,
                amount,
                issue: if containers.is_none() {
                    Some("Coverage missing: set m² per container per coat in Settings → Material.")
                } else if price.is_none() {
                    Some("Price missing: set the price per container in Settings → Material.")
                } else if amount.is_none() {
                    Some("Cost exceeds estimate range.")
                } else {
                    None
                },
            }
        })
        .collect()
}

fn cost_group(ui: &mut egui::Ui, label: &str) {
    let colors = Palette::for_ui(ui);
    ui.separator();
    ui.add(egui::Label::new(RichText::new(label).small().strong().color(colors.muted)).truncate());
}

fn finishing_cost_rows(
    ui: &mut egui::Ui,
    table: &mut InspectorTable<4>,
    costs: &[FinishCost],
    currency: &str,
) {
    let colors = Palette::for_ui(ui);
    for kind in finish_settings::FinishType::ALL
        .into_iter()
        .map(Some)
        .chain([None])
    {
        if !costs.iter().any(|cost| cost.kind == kind) {
            continue;
        }
        cost_group(
            ui,
            kind.map_or("Unresolved finishing products", |kind| kind.label()),
        );
        for (index, cost) in costs
            .iter()
            .enumerate()
            .filter(|(_, cost)| cost.kind == kind)
        {
            table.row(ui, ("finish_cost", index), |column, ui| {
                let text = match column {
                    0 => cost.name.clone(),
                    1 => cost
                        .containers
                        .map(|value| format!("{value:.0}"))
                        .unwrap_or_else(|| "—".into()),
                    2 => cost
                        .price
                        .map(|value| currency::format_value(currency, value))
                        .unwrap_or_else(|| "—".into()),
                    _ => cost
                        .amount
                        .map(|value| currency::format_value(currency, value))
                        .unwrap_or_else(|| "—".into()),
                };
                let mut lines = vec![RichText::new(text).strong()];
                if column == 0 {
                    if let Some(issue) = cost.issue {
                        lines.push(RichText::new(issue).small().color(colors.accent));
                    }
                }
                product_cell_text(ui, lines);
            });
        }
    }
}

fn format_minutes(minutes: f64) -> String {
    // Keep fractional-minute work visible without inflating the total of zero steps.
    if !minutes.is_finite() || minutes >= u64::MAX as f64 {
        return "Exceeds estimate range".into();
    }
    if minutes > 0. && minutes < 1. {
        return "<1 min".into();
    }
    let rounded = minutes.round() as u64;
    format!("{} h {} min", rounded / 60, rounded % 60)
}

fn estimate_kpi(
    ui: &mut egui::Ui,
    value: &str,
    label: &str,
    size: f32,
    accent: egui::Color32,
    corners: egui::CornerRadius,
) {
    let colors = Palette::for_ui(ui);
    let tint = |amount: f32| {
        let blend = |base: u8, color: u8| {
            (base as f32 + (color as f32 - base as f32) * amount).round() as u8
        };
        egui::Color32::from_rgb(
            blend(colors.surface.r(), accent.r()),
            blend(colors.surface.g(), accent.g()),
            blend(colors.surface.b(), accent.b()),
        )
    };
    egui::Frame::new()
        .fill(tint(if ui.visuals().dark_mode { 0.10 } else { 0.06 }))
        .stroke(Stroke::new(1., tint(0.25)))
        .corner_radius(corners)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            // Table cells justify their children; KPI text needs natural word spacing.
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add(
                    egui::Label::new(RichText::new(value).size(size).strong().color(accent)).wrap(),
                );
                ui.add_space(3.);
                ui.label(RichText::new(label).small().color(colors.ink));
            });
        });
}

fn rate_input(ui: &mut egui::Ui, rate: &mut f64) {
    ui.add(egui::DragValue::new(rate).speed(0.5).range(0.0..=f64::MAX));
}

fn wait_input(ui: &mut egui::Ui, hours: &mut f64) {
    ui.add(
        egui::DragValue::new(hours)
            .speed(0.25)
            .range(0.0..=f64::MAX)
            .suffix(" h"),
    );
}

fn finish_controls(
    ui: &mut egui::Ui,
    title: &str,
    key: &'static str,
    step: &mut Step,
    products: &[finish_settings::Finish],
    filter: &mut String,
) {
    sidebar::inspector_section(ui, title, key, false, |ui| {
        ui.checkbox(&mut step.enabled, "Include in estimate");
        if !step.enabled {
            return;
        }
        let mut table =
            InspectorTable::new(ui.available_width(), [Column::flex(0.4), Column::flex(0.6)]);
        for (row, label) in [
            "Product",
            "Minutes / m²",
            "Coats",
            if title == "Pre-stain" {
                "Soak per coat"
            } else {
                "Wait per coat"
            },
        ]
        .into_iter()
        .enumerate()
        {
            table.row(ui, (key, row), |column, ui| {
                if column == 0 {
                    ui.label(label);
                } else {
                    match row {
                        0 => {
                            finish_settings::selector(
                                ui,
                                key,
                                &mut step.product_id,
                                products,
                                filter,
                            );
                        }
                        1 => rate_input(ui, &mut step.minutes_per_m2),
                        2 => {
                            ui.add(egui::DragValue::new(&mut step.coats).range(1..=100));
                        }
                        _ => wait_input(ui, &mut step.drying_hours),
                    }
                }
            });
        }
        if products.is_empty() {
            ui.weak("Add finishing products in Settings → Material.");
        } else if step
            .product_id
            .is_some_and(|id| !products.iter().any(|product| product.id == id))
        {
            ui.weak("The selected product is no longer in Settings → Material. Choose a replacement or clear it.");
        }
        let mut separate = step.final_drying_hours.is_some();
        if ui
            .checkbox(&mut separate, "Different wait after final coat")
            .changed()
        {
            step.final_drying_hours = separate.then_some(step.drying_hours);
        }
        if let Some(hours) = &mut step.final_drying_hours {
            table.row(ui, (key, "final_wait"), |column, ui| {
                if column == 0 {
                    ui.label("Final wait");
                } else {
                    wait_input(ui, hours);
                }
            });
        }
    });
}

/// Shared editor for device defaults and document-owned object configurations.
pub(super) fn treatment_controls(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    defaults: bool,
    products: &[finish_settings::Finish],
    filters: &mut std::collections::BTreeMap<String, String>,
) {
    let colors = Palette::for_ui(ui);
    let keys = if defaults {
        [
            "settings/treatment/sanding",
            "settings/treatment/finishing",
            "settings/treatment/pre_stain",
            "settings/treatment/stain",
            "settings/treatment/varnish",
        ]
    } else {
        [
            "entity/treatment/sanding",
            "entity/treatment/finishing",
            "entity/treatment/pre_stain",
            "entity/treatment/stain",
            "entity/treatment/varnish",
        ]
    };
    sidebar::inspector_section(ui, "Sanding", keys[0], false, |ui| {
        let mut table = InspectorTable::new(
            ui.available_width(),
            [Column::flex(1.), Column::flex(1.4), Column::flex(1.)],
        );
        table.header(ui, ["Grit", "Between coats", "Min / m²"]);
        for ((grit, step), after_varnish) in GRITS
            .iter()
            .zip(&mut settings.sanding)
            .zip(&mut settings.sanding_after_varnish)
        {
            table.row(ui, ("sanding", grit), |column, ui| {
                match column {
                    0 => { ui.checkbox(&mut step.enabled, grit.to_string()); }
                    1 => { ui.checkbox(after_varnish, "").on_hover_text("Between varnish coats: sand after each coat except the last, instead of before finishing."); }
                    _ => { ui.add_enabled_ui(step.enabled, |ui| rate_input(ui, &mut step.minutes_per_m2)); }
                }
            });
        }
        ui.label(
            RichText::new(
                "Between coats applies to varnish only. Enable varnish with at least 2 coats.",
            )
            .small()
            .color(colors.muted),
        );
    });
    ui.add_space(8.);
    sidebar::inspector_section(ui, "Finishing", keys[1], false, |ui| {
        for (title, key, step) in [
            ("Pre-stain", keys[2], &mut settings.pre_stain),
            ("Stain", keys[3], &mut settings.stain),
            ("Varnish", keys[4], &mut settings.varnish),
        ] {
            finish_controls(
                ui,
                title,
                key,
                step,
                products,
                filters.entry(key.into()).or_default(),
            );
        }
    });
    ui.add_space(8.);
    settings.sanitize();
}

impl KetchupApp {
    fn material_cost_estimate_contents(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        self.estimate_cache.area(
            &self.doc.model,
            &self.selected,
            &self.general_settings.stock,
        );
        let finish_costs = finishing_costs(
            &self.doc.model,
            &self.estimate_cache.area.as_ref().unwrap().1,
            &self.time_estimates,
            &self.general_settings.stock.finishes,
        );
        let empty_plan = MaterialEstimate {
            requirements: Vec::new(),
            skipped: 0,
        };
        let (plan, stock_issue) = match self.estimate_cache.materials(
            &self.doc.model,
            &self.selected,
            self.project.cutting_plan_layout,
        ) {
            Ok(plan) => (plan, None),
            Err(message) => (&empty_plan, Some(message.as_str())),
        };
        let catalog = &self.general_settings.stock;
        let requirements: Vec<_> = plan
            .requirements
            .iter()
            .map(|required| {
                let price = required
                    .material_id
                    .and_then(|id| catalog.wood.iter().find(|stock| stock.id == id))
                    .filter(|stock| stock.definition().dimensions == required.material.dimensions)
                    .map(|stock| stock.price);
                (required, price)
            })
            .collect();
        let unpriced: usize = requirements
            .iter()
            .filter(|(_, price)| price.is_none())
            .map(|(required, _)| required.quantity)
            .sum();
        let excluded: usize = requirements
            .iter()
            .map(|(required, _)| required.excluded)
            .sum();
        let partial = unpriced > 0
            || excluded > 0
            || plan.skipped > 0
            || finish_costs.iter().any(|cost| cost.amount.is_none());

        egui::Frame::new()
            .fill(colors.canvas)
            .stroke(Stroke::new(1., colors.border))
            .corner_radius(5)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new("Cost estimate").small().color(colors.muted));
                ui.separator();
                let mut table = InspectorTable::new(
                    ui.available_width(),
                    [
                        Column::flex(0.4),
                        Column::flex(0.14),
                        Column::flex(0.23),
                        Column::flex(0.23),
                    ],
                )
                .striped();
                let currency = currency::code(&catalog.currency);
                let headers = [
                    "Material / Product".to_owned(),
                    "Quantity".to_owned(),
                    format!("Unit price ({currency})"),
                    format!("Amount ({currency})"),
                ];
                table.header(ui, headers.each_ref().map(String::as_str));
                if !requirements.is_empty() { cost_group(ui, "Wood stock"); }
                for (required, price) in &requirements {
                    let values = [
                        required.quantity.to_string(),
                        price
                            .map(|price| currency::format_value(&catalog.currency, price))
                            .unwrap_or_else(|| "—".into()),
                        price
                            .map(|price| {
                                currency::format_value(
                                    &catalog.currency,
                                    required.quantity as f64 * price,
                                )
                            })
                            .unwrap_or_else(|| "—".into()),
                    ];
                    table.row(
                        ui,
                        ("material_cost", &required.material.name),
                        |column, ui| {
                            if column == 0 {
                                let mut lines =
                                    vec![RichText::new(&required.material.name).strong()];
                                if price.is_none() {
                                    lines.push(
                                        RichText::new("Price unavailable")
                                            .small()
                                            .color(colors.accent),
                                    );
                                }
                                if required.excluded > 0 {
                                    lines.push(
                                        RichText::new(format!(
                                            "{} cuts do not fit · Excluded from cost",
                                            required.excluded,
                                        ))
                                        .small()
                                        .color(colors.accent),
                                    );
                                }
                                product_cell_text(ui, lines);
                            } else {
                                product_cell_text(
                                    ui,
                                    vec![RichText::new(&values[column - 1]).strong()],
                                );
                            }
                        },
                    );
                }
                finishing_cost_rows(ui, &mut table, &finish_costs, currency);
                if let Some(message) = stock_issue {
                    ui.label(RichText::new("No stock estimate yet").small().color(colors.muted))
                        .on_hover_text(message);
                }
                if !finish_costs.is_empty() {
                    ui.label(RichText::new("Finishes: whole containers · Needs combined across objects and coats before rounding")
                        .small().color(colors.muted));
                }
            });
        ui.add_space(6.);
        if partial {
            egui::Frame::new()
                .fill(colors.canvas)
                .corner_radius(5)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new("Estimate incomplete").strong().color(colors.accent));
                    if unpriced > 0 {
                        ui.label(format!("{unpriced} stock pieces have no price. Add prices in Settings → Material."));
                    }
                    if excluded > 0 {
                        ui.label("Some cuts need larger stock. Their cost is excluded.");
                    }
                    if plan.skipped > 0 {
                        ui.label(format!("{} assigned items omitted: no usable thickness.", plan.skipped));
                    }
                });
            ui.add_space(6.);
        }
        ui.label(RichText::new(format!("Kerf {} · Rotation {}",
            self.doc.display.units.format_length(self.project.cutting_plan_layout.kerf_mm / 1000.),
            if self.project.cutting_plan_layout.allow_rotation { "on" } else { "off" }
        )).small().color(colors.muted))
            .on_hover_text("Uses each part's full bounding box with the same stock packing and settings as Prepare cutting plan. Notches and cutouts do not reduce the required blank. Prices cover purchased material, including cutting waste. Labor is not included. Finishing products are estimated separately from surface area, coats and container coverage.");
    }

    pub(super) fn surface_treatment_info(&mut self, ui: &mut egui::Ui) {
        let wood: WoodMaterials = self
            .general_settings
            .stock
            .wood
            .iter()
            .map(|stock| (stock.id, stock.definition()))
            .collect();
        let parents = self
            .doc
            .model
            .objects
            .values()
            .flat_map(|object| object.entities.iter().map(move |id| (*id, object.id)))
            .collect();
        let objects: Vec<_> = self
            .doc
            .model
            .expand_entities(&self.selected)
            .into_iter()
            .filter(|id| self.doc.model.objects.contains_key(id))
            .filter(|id| object_has_wood_material(&self.doc.model, *id, &parents, &wood))
            .collect();
        let Some(first) = objects.first() else {
            return;
        };
        let effective = |id: &u64| {
            self.doc.model.objects[id]
                .surface_treatment_override()
                .unwrap_or(&self.time_estimates)
                .clone()
        };
        let mut settings = effective(first);
        let original = settings.clone();
        let mixed = objects.iter().any(|id| effective(id) != original);
        let all_defaults = objects.iter().all(|id| {
            self.doc.model.objects[id]
                .surface_treatment_override()
                .is_none()
        });
        if mixed {
            ui.weak("Mixed sanding and finishing settings. Changes apply to selected wood objects, including nested wood objects.");
        }
        let mut reset = false;
        ui.add_enabled_ui(!self.doc.model.selection_locked(&self.selected), |ui| {
            treatment_controls(
                ui,
                &mut settings,
                false,
                &self.general_settings.stock.finishes,
                &mut self.finish_product_filters,
            );
            reset = ui
                .add_enabled(!all_defaults, egui::Button::new("Revert to defaults"))
                .on_hover_text("Restore sanding and finishing defaults from Settings for the selected wood objects.")
                .clicked();
        });
        if reset || settings != original {
            let before = self.doc.model.clone();
            for id in objects {
                let object = self.doc.model.objects.get_mut(&id).unwrap();
                if reset {
                    object.surface_treatment = Settings::default();
                    object.surface_treatment_customized = false;
                } else {
                    let mut target = object
                        .surface_treatment_override()
                        .unwrap_or(&self.time_estimates)
                        .clone();
                    apply_treatment_changes(&mut target, &original, &settings);
                    object.surface_treatment = target;
                    object.surface_treatment_customized = true;
                }
            }
            self.finish_edit("Change sanding and finishing", before, ui.ctx());
        }
    }

    pub(super) fn time_estimates_contents(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let area = self.estimate_cache.area(
            &self.doc.model,
            &self.selected,
            &self.general_settings.stock,
        );
        let tasks = object_tasks(
            &self.doc.model,
            &self.estimate_cache.area.as_ref().unwrap().1,
            &self.time_estimates,
        );
        let finish_costs = finishing_costs(
            &self.doc.model,
            &self.estimate_cache.area.as_ref().unwrap().1,
            &self.time_estimates,
            &self.general_settings.stock.finishes,
        );
        let finish_total: f64 = finish_costs.iter().filter_map(|cost| cost.amount).sum();
        let finish_partial = finish_costs.iter().any(|cost| cost.amount.is_none());
        let duration = tasks
            .iter()
            .fold(Duration::default(), |total, (_, task, _)| Duration {
                work: total.work + task.work,
                wait: total.wait + task.wait,
            });
        let catalog = &self.general_settings.stock;
        let material_summary = match self.estimate_cache.materials(
            &self.doc.model,
            &self.selected,
            self.project.cutting_plan_layout,
        ) {
            Ok(plan) => {
                let mut total = finish_total;
                let mut partial = plan.skipped > 0 || finish_partial;
                for required in &plan.requirements {
                    let price = required
                        .material_id
                        .and_then(|id| catalog.wood.iter().find(|stock| stock.id == id))
                        .filter(|stock| {
                            stock.definition().dimensions == required.material.dimensions
                        })
                        .map(|stock| stock.price);
                    total += required.quantity as f64 * price.unwrap_or(0.);
                    partial |= price.is_none() || required.excluded > 0;
                }
                format!(
                    "{} {}{}",
                    currency::code(&catalog.currency),
                    currency::format_value(&catalog.currency, total),
                    if partial { " (partial)" } else { "" },
                )
            }
            Err(_) if !finish_costs.is_empty() => format!(
                "{} {} (partial)",
                currency::code(&catalog.currency),
                currency::format_value(&catalog.currency, finish_total),
            ),
            Err(_) => "Unavailable".to_owned(),
        };
        estimate_kpi(
            ui,
            &material_summary,
            if self.selected.is_empty() {
                "Materials · All visible parts"
            } else {
                "Materials · Selected parts"
            },
            30.,
            colors.green,
            egui::CornerRadius {
                nw: 8,
                ne: 8,
                sw: 0,
                se: 0,
            },
        );
        // The material tile and the two time tiles form one joined dashboard.
        ui.add_space(-ui.spacing().item_spacing.y);
        let mut summary = InspectorTable::with_column_spacing(
            ui.available_width(),
            [Column::flex(1.), Column::flex(1.)],
            0.,
        );
        summary.row(ui, "estimate_summary_times", |column, cell| {
            let (minutes, label, background) = if column == 0 {
                (duration.work, "Hands-on work", colors.accent)
            } else {
                (duration.wait, "Drying / Waiting", colors.muted)
            };
            estimate_kpi(
                cell,
                &format_minutes(minutes),
                label,
                20.,
                background,
                egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: if column == 0 { 8 } else { 0 },
                    se: if column == 1 { 8 } else { 0 },
                },
            );
        });
        ui.add_space(8.);
        ui.label(
            RichText::new("Time covers sanding and finishing, including final drying.")
                .small()
                .color(colors.muted),
        );
        ui.add_space(8.);
        ui.separator();
        ui.add_space(4.);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(self.doc.display.units.format_area(area)).strong());
            ui.label(
                RichText::new(if self.selected.is_empty() {
                    "Visible wood surfaces"
                } else {
                    "Selected wood surfaces"
                })
                .small()
                .color(colors.muted),
            );
        });
        ui.label(RichText::new("Finishing applies only to objects assigned a material from Settings → Material → Wood.").small().color(colors.muted));
        ui.add_space(6.);
        egui::Frame::new()
            .fill(colors.canvas)
            .stroke(Stroke::new(1., colors.border))
            .corner_radius(5)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if area <= 0. {
                    ui.label(if self.selected.is_empty() {
                        "No visible surfaces with assigned wood materials."
                    } else {
                        "Select faces or groups with assigned wood materials."
                    });
                } else if tasks.is_empty() {
                    ui.label("Choose sanding grits or finishes for objects in Materials.");
                } else {
                    ui.label(
                        RichText::new("Sequential total")
                            .small()
                            .color(colors.muted),
                    );
                    ui.label(
                        RichText::new(format_minutes(duration.work + duration.wait))
                            .size(19.)
                            .strong()
                            .color(colors.green),
                    );
                    ui.separator();
                    let mut table = InspectorTable::new(
                        ui.available_width(),
                        [Column::flex(1.), Column::flex(1.), Column::flex(1.)],
                    )
                    .striped();
                    table.header(ui, ["Task", "Hands-on", "Drying / Waiting"]);
                    for (index, (title, task, _)) in tasks.iter().enumerate() {
                        table.row(ui, ("task", index), |column, ui| {
                            if column == 0 {
                                ui.label(
                                    RichText::new(
                                        title.strip_prefix("Sanding · ").unwrap_or(title),
                                    )
                                    .small(),
                                )
                                .on_hover_text(title.as_str());
                            } else {
                                let minutes = if column == 1 { task.work } else { task.wait };
                                ui.label(
                                    RichText::new(if minutes == 0. {
                                        "-".to_owned()
                                    } else {
                                        format_minutes(minutes)
                                    })
                                    .small(),
                                );
                            }
                        });
                    }
                    ui.separator();
                    table.row(ui, "total", |column, ui| {
                        let text = match column {
                            0 => "Total".to_owned(),
                            _ => {
                                let minutes = if column == 1 {
                                    duration.work
                                } else {
                                    duration.wait
                                };
                                if minutes == 0. {
                                    "-".to_owned()
                                } else {
                                    format_minutes(minutes)
                                }
                            }
                        };
                        ui.label(RichText::new(text).small().strong());
                    });
                    ui.label(
                        RichText::new("Includes final drying · Objects dry together between steps")
                            .small()
                            .color(colors.muted),
                    );
                }
            });
        ui.add_space(8.);
        self.material_cost_estimate_contents(ui);
    }
}

#[cfg(test)]
#[path = "time_estimates_tests.rs"]
mod tests;
