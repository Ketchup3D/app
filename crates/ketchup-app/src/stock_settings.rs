//! Reusable purchasing catalog. Geometry and project assignments remain in documents.
#[cfg(test)]
use crate::test_ui::ContextExt as _;
use super::*;
use crate::inspector_table::{Column, InspectorTable};
use ketchup_core::LengthUnit;

/// Purchasing categories; existing stock remains unspecified until classified.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum WoodType {
    #[default]
    Unspecified,
    SolidWood,
    Plywood,
    Mdf,
    Particleboard,
    Osb,
    Hardboard,
    Veneer,
    Other,
}

impl WoodType {
    const ALL: [Self; 9] = [
        Self::Unspecified,
        Self::SolidWood,
        Self::Plywood,
        Self::Mdf,
        Self::Particleboard,
        Self::Osb,
        Self::Hardboard,
        Self::Veneer,
        Self::Other,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Unspecified => "Unspecified",
            Self::SolidWood => "Solid wood",
            Self::Plywood => "Plywood",
            Self::Mdf => "MDF",
            Self::Particleboard => "Particleboard",
            Self::Osb => "OSB",
            Self::Hardboard => "Hardboard",
            Self::Veneer => "Veneer",
            Self::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Stock {
    #[serde(default)]
    pub id: ketchup_core::MaterialId,
    pub name: String,
    pub wood_type: WoodType,
    pub thickness_mm: f64,
    pub width_mm: f64,
    pub length_mm: f64,
    pub price: f64,
}

impl Default for Stock {
    fn default() -> Self {
        Self {
            id: ketchup_core::MaterialId::new_v4(),
            name: "Wood stock".into(),
            wood_type: WoodType::default(),
            thickness_mm: 19.,
            width_mm: 89.,
            length_mm: 2440.,
            price: 0.,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Settings {
    pub currency: String,
    pub currency_filter: String,
    pub units: LengthUnit,
    pub wood: Vec<Stock>,
    pub projects: std::collections::BTreeMap<String, ProjectUsage>,
    pub finishes: Vec<finish_settings::Finish>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct ProjectUsage {
    pub project_id: Option<ketchup_core::MaterialId>,
    pub name: String,
    pub path: Option<String>,
    pub materials: std::collections::BTreeMap<ketchup_core::MaterialId, usize>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            currency: currency::DEFAULT.into(),
            currency_filter: String::new(),
            units: LengthUnit::Millimeters,
            wood: Vec::new(),
            projects: Default::default(),
            finishes: Vec::new(),
        }
    }
}

impl Settings {
    pub(super) fn normalize(&mut self) {
        currency::normalize(&mut self.currency);
        let mut used = BTreeSet::new();
        for stock in &mut self.wood {
            if stock.id.is_nil() || !used.insert(stock.id) {
                stock.id = ketchup_core::MaterialId::new_v4();
                used.insert(stock.id);
            }
            for dimension in [
                &mut stock.thickness_mm,
                &mut stock.width_mm,
                &mut stock.length_mm,
            ] {
                *dimension = if dimension.is_finite() {
                    dimension.clamp(0.01, 1_000_000.)
                } else {
                    1.
                };
            }
            stock.price = if stock.price.is_finite() {
                stock.price.clamp(0., 1e9)
            } else {
                0.
            };
        }
        finish_settings::normalize(&mut self.finishes);
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        type_filters: &mut std::collections::BTreeMap<String, String>,
    ) -> Option<ketchup_core::MaterialId> {
        let mut requested_removal = None;
        ui.text_weak("Save the wood stock and finishing products you can buy, with your supplier’s prices. Use actual dimensions for boards or sheets.");
        ui.horizontal_wrapped(|ui| {
            ui.text_label("Currency");
            currency::selector(ui, &mut self.currency, &mut self.currency_filter)
                .on_hover_text("Used for all prices and cost estimates in the app. Changing currency does not convert entered prices.");
            ui.text_label("Sizing units");
            egui::ComboBox::from_id_salt("stock_sizing_units")
                .selected_text(self.units.label())
                .show_ui(ui, |ui| {
                    for &unit in LengthUnit::ALL {
                        ui.selectable_value(&mut self.units, unit, unit.label());
                    }
                });
        });
        sidebar::inspector_section(ui, "Wood", "settings/stock/wood", true, |ui| {
            if self.wood.is_empty() {
                ui.text_weak("No wood stock yet. Add a board or sheet, then enter its size and price.");
            } else {
                let key = "settings/stock/wood/table";
                let scroll = sidebar::restored_scroll_area(
                    ui,
                    key,
                    egui::ScrollArea::horizontal()
                        .id_salt(key)
                        .auto_shrink([false, true]),
                );
                let mut table = InspectorTable::new(
                    ui.available_width(),
                    [
                        Column::at_least(100.),
                        Column::at_least(150.),
                        Column::at_least(66.),
                        Column::at_least(66.),
                        Column::at_least(66.),
                        Column::at_least(112.),
                        Column::fixed(28.),
                    ],
                )
                .striped();
                let output = scroll.show(ui, |ui| {
                    table.header(
                        ui,
                        [
                            "Stock",
                            "Type",
                            "Thickness",
                            "Width",
                            "Length",
                            "Price / piece",
                            "",
                        ],
                    );
                    for stock in &mut self.wood {
                        table.row(ui, stock.id, |column, ui| {
                            let factor = self.units.factor() * 1000.;
                            match column {
                                0 => {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut stock.name)
                                            .hint_text("e.g. Pine 2 × 4")
                                            .char_limit(120),
                                    );
                                }
                                1 => {
                                    use super::searchable_selector::{Choice, SearchableSelector};
                                    let key = format!("settings/stock/wood/{}/type", stock.id);
                                    SearchableSelector {
                                        id_salt: egui::IdSalt::new((&key, "selector")),
                                        scroll_key: &format!("{key}/results"),
                                        selected_label: stock.wood_type.label(),
                                        width: ui.available_width(),
                                        popup_width: 240.,
                                        hint: "Search wood types…",
                                        empty_text: "No wood types match your search.",
                                    }
                                    .show(
                                        ui,
                                        &mut stock.wood_type,
                                        type_filters.entry(key.clone()).or_default(),
                                        WoodType::ALL
                                            .into_iter()
                                            .map(|kind| Choice::new(kind, kind.label()))
                                            .collect(),
                                    );
                                }
                                2..=4 => {
                                    let dimension = match column {
                                        2 => &mut stock.thickness_mm,
                                        3 => &mut stock.width_mm,
                                        _ => &mut stock.length_mm,
                                    };
                                    let mut value = *dimension / factor;
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut value)
                                                .range((0.01 / factor)..=(1_000_000. / factor))
                                                .speed(1. / factor)
                                                .max_decimals(4),
                                        )
                                        .changed()
                                    {
                                        *dimension = value * factor;
                                    }
                                }
                                5 => {
                                    ui.add(currency::price_input(&self.currency, &mut stock.price))
                                        .on_hover_text(format!("{} per piece", self.currency));
                                }
                                _ => {
                                    if ui
                                        .button("×")
                                        .on_hover_text(format!("Remove {}", stock.name))
                                        .clicked()
                                    {
                                        requested_removal = Some(stock.id);
                                    }
                                }
                            }
                        });
                    }
                });
                sidebar::remember_scroll(ui.ctx(), key, output.state.clamped_offset());
                ui.text_weak(format!(
                    "Sizes in {} · Prices in {} per piece",
                    self.units.symbol(),
                    self.currency
                ));
            }
            if ui.button("+ Add wood stock").clicked() {
                self.wood.push(Stock::default());
            }
        });
        finish_settings::show(ui, &mut self.finishes, &self.currency);
        requested_removal
    }
}

impl Stock {
    pub(super) fn definition(&self) -> ketchup_core::StockMaterial {
        ketchup_core::StockMaterial {
            name: self.name.clone(),
            dimensions: [self.thickness_mm, self.width_mm, self.length_mm]
                .map(crate::cutting_layout::tenths),
        }
    }
}

impl KetchupApp {
    pub(super) fn sync_stock_materials(&mut self, ctx: &egui::Context) {
        self.general_settings.stock.normalize();
        let catalog = self
            .general_settings
            .stock
            .wood
            .iter()
            .map(|stock| (stock.id, stock.definition()))
            .collect();
        let mut document_changed = self.doc.model.sync_stock_materials(&catalog);
        let mut materials = std::collections::BTreeMap::new();
        for object in self.doc.model.objects.values() {
            if let Some(id) = object.stock_material_id {
                *materials.entry(id).or_insert(0) += 1;
            }
        }
        if !materials.is_empty() && self.doc.project_id.is_none() {
            self.doc.project_id = Some(ketchup_core::MaterialId::new_v4());
            document_changed = true;
        }
        if let Some(id) = self.doc.project_id {
            let path = self
                .project
                .path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            // Save As keeps the old file's last-known associations. Only retire
            // the temporary record for a project that has just gained a path.
            if path.is_some() {
                self.general_settings
                    .stock
                    .projects
                    .retain(|_, usage| usage.project_id != Some(id) || usage.path.is_some());
            }
            let key = self.material_usage_key().unwrap();
            self.general_settings.stock.projects.insert(
                key,
                ProjectUsage {
                    project_id: Some(id),
                    name: self.doc.name.clone(),
                    path,
                    materials,
                },
            );
        }
        if document_changed {
            self.changed(ctx);
        }
    }

    fn material_usage_key(&self) -> Option<String> {
        self.doc.project_id.map(|id| {
            format!(
                "{id}/{}",
                self.project
                    .path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| self.doc.name.clone())
            )
        })
    }

    pub(super) fn material_removal_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.material_removal else {
            return;
        };
        let Some(stock) = self
            .general_settings
            .stock
            .wood
            .iter()
            .find(|stock| stock.id == id)
        else {
            self.material_removal = None;
            return;
        };
        let name = stock.name.clone();
        let current_key = self.material_usage_key();
        let mut remove = false;
        let mut cancel = false;
        let response = Modal::new("remove_stock_material", 520.).show(ctx, |modal| {
            modal.header(|ui| { ui.text_heading("Remove material?"); });
            modal.body(|ui| {
                ui.text_label(format!("Remove “{name}” from Settings → Material?"));
                ui.text_label("This material may be associated with objects in other projects.");
                ui.add_space(8.);
                ui.text_strong("Known project associations on this device");
                let mut found = false;
                for (project_key, usage) in &self.general_settings.stock.projects {
                    if let Some(count) = usage.materials.get(&id).filter(|count| **count > 0) {
                        found = true;
                        let current = if Some(project_key) == current_key.as_ref() { " (current project)" } else { "" };
                        let label = format!("{}{} · {} assigned object{}", usage.name, current, count, if *count == 1 { "" } else { "s" });
                        let response = ui.text_label(label);
                        if let Some(path) = &usage.path { response.on_hover_text(path); }
                    }
                }
                if !found { ui.text_weak("No known project associations."); }
                ui.text_weak("Usage is based on projects opened on this device and may be out of date. Unopened projects may also reference this material.");
                ui.add_space(8.);
                ui.text_label("Existing projects keep their material definitions and assignments. They will no longer receive catalog updates for this material.");
            });
            modal.footer(|ui| {
                if ui.button("Cancel").clicked() { cancel = true; }
                if primary_button(ui, "Remove material", true).clicked() { remove = true; }
            });
        });
        if remove {
            self.general_settings
                .stock
                .wood
                .retain(|stock| stock.id != id);
        }
        if remove || cancel || response.should_close() {
            self.material_removal = None;
        }
    }
}

const MATERIAL_RESULTS_SCROLL_KEY: &str = "entity/material/results";

struct MaterialChoice {
    id: ketchup_core::MaterialId,
    label: String,
}

fn material_choices(
    wood: &[Stock],
    project_materials: &std::collections::BTreeMap<
        ketchup_core::MaterialId,
        ketchup_core::StockMaterial,
    >,
    query: &str,
) -> Vec<MaterialChoice> {
    let mut choices = Vec::new();
    let mut listed = BTreeSet::new();
    for stock in wood {
        listed.insert(stock.id);
        choices.push(MaterialChoice {
            id: stock.id,
            label: format!(
                "{} ({} × {} × {} mm)",
                stock.name, stock.thickness_mm, stock.width_mm, stock.length_mm
            ),
        });
    }
    for (id, material) in project_materials {
        if !listed.contains(id) {
            choices.push(MaterialChoice {
                id: *id,
                label: format!("{} (project material)", material.name),
            });
        }
    }
    choices.retain(|choice| super::searchable_selector::matches_query(&choice.label, query));
    choices
}

/// Search catalog and project-only stock without changing assignments until a
/// result is explicitly picked. Return the action separately from the value so
/// choosing an unchanged value can still unify a mixed selection.
pub(super) fn material_selector(
    ui: &mut egui::Ui,
    selected_label: &str,
    selected_id: &mut Option<ketchup_core::MaterialId>,
    wood: &[Stock],
    project_materials: &std::collections::BTreeMap<
        ketchup_core::MaterialId,
        ketchup_core::StockMaterial,
    >,
    filter: &mut String,
) -> egui::InnerResponse<bool> {
    use super::searchable_selector::{Choice, SearchableSelector};
    let mut none = Choice::new(None, "Unassigned");
    none.always_visible = true;
    let mut choices = vec![none];
    choices.extend(
        material_choices(wood, project_materials, "")
            .into_iter()
            .map(|choice| Choice::new(Some(choice.id), choice.label)),
    );
    SearchableSelector {
        id_salt: egui::IdSalt::new("entity_stock_material"),
        scroll_key: MATERIAL_RESULTS_SCROLL_KEY,
        selected_label,
        width: ui.available_width(),
        popup_width: 300.,
        hint: "Search name or size…",
        empty_text: "No stock materials match your search.",
    }
    .show(ui, selected_id, filter, choices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_stock_defaults_to_unspecified_and_all_types_round_trip() {
        let old: Stock = serde_json::from_str(r#"{"name":"Oak","price":12.5}"#).unwrap();
        assert_eq!(old.wood_type, WoodType::Unspecified);
        assert_eq!(old.name, "Oak");
        for wood_type in WoodType::ALL {
            let stock = Stock {
                wood_type,
                ..old.clone()
            };
            let saved = serde_json::to_string(&stock).unwrap();
            assert_eq!(serde_json::from_str::<Stock>(&saved).unwrap(), stock);
        }
    }

    #[test]
    fn both_catalogs_show_the_selected_currency_on_editable_prices() {
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
        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        let mut settings = Settings {
            wood: vec![Stock::default()],
            finishes: vec![finish_settings::Finish::default()],
            ..Default::default()
        };
        for currency in ["USD", "EUR"] {
            settings.currency = currency.into();
            let mut labels = Vec::new();
            for _ in 0..3 {
                let output = ctx.run_test_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200., 900.))),
                        ..Default::default()
                    },
                    |ui| {
                        settings.show(ui, &mut Default::default());
                    },
                );
                labels.clear();
                for shape in &output.shapes {
                    collect(&shape.shape, &mut labels);
                }
            }
            assert_eq!(
                labels.iter().filter(|text| text.trim() == currency).count(),
                2,
                "Wood and finishing prices must both display the selected currency: {labels:?}"
            );
            assert!(
                labels
                    .iter()
                    .any(|text| text == &format!("Prices in {currency} per container"))
            );
            assert!(
                labels
                    .iter()
                    .any(|text| text.contains(&format!("Prices in {currency} per piece")))
            );
        }
    }

    #[test]
    fn catalog_ids_are_stable_and_missing_or_duplicate_ids_are_upgraded() {
        let mut old: Settings =
            serde_json::from_str(r#"{"wood":[{"name":"Oak"},{"name":"Oak"}]}"#).unwrap();
        assert!(old.wood[0].id.is_nil());
        old.normalize();
        let first = old.wood[0].id;
        let second = old.wood[1].id;
        assert_ne!(first, second);
        old.wood[0].name = "Renamed oak".into();
        old.wood.swap(0, 1);
        old.normalize();
        assert_eq!(old.wood[1].id, first);
        let saved = serde_json::to_string(&old).unwrap();
        let restored: Settings = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored, old);
        old.wood[0].id = first;
        old.normalize();
        assert_ne!(old.wood[0].id, old.wood[1].id);
    }

    #[test]
    fn old_catalogs_default_empty_and_invalid_values_are_normalized() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old, Settings::default());
        let mut settings = Settings {
            wood: vec![Stock {
                thickness_mm: -1.,
                width_mm: f64::NAN,
                length_mm: f64::INFINITY,
                price: -5.,
                ..Default::default()
            }],
            ..Default::default()
        };
        settings.normalize();
        assert_eq!(settings.wood[0].thickness_mm, 0.01);
        assert_eq!(settings.wood[0].width_mm, 1.);
        assert_eq!(settings.wood[0].length_mm, 1.);
        assert_eq!(settings.wood[0].price, 0.);
    }
}

#[cfg(test)]
#[path = "stock_settings_tests.rs"]
mod material_selector_tests;
