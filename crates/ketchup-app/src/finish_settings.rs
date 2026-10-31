//! Reusable finishing-product prices, stored with the device's material catalog.
use super::*;
use crate::inspector_table::{Column, InspectorTable};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum FinishType {
    PreStain,
    #[default]
    Stain,
    Paint,
    Varnish,
    Primer,
    Sealer,
    Lacquer,
    Oil,
    Wax,
    Other,
}

impl FinishType {
    pub(super) const ALL: [Self; 10] = [
        Self::PreStain,
        Self::Stain,
        Self::Paint,
        Self::Varnish,
        Self::Primer,
        Self::Sealer,
        Self::Lacquer,
        Self::Oil,
        Self::Wax,
        Self::Other,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::PreStain => "Pre-stain",
            Self::Stain => "Stain",
            Self::Paint => "Paint",
            Self::Varnish => "Varnish",
            Self::Primer => "Primer",
            Self::Sealer => "Sealer",
            Self::Lacquer => "Lacquer",
            Self::Oil => "Oil",
            Self::Wax => "Wax",
            Self::Other => "Other",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum ContainerUnit {
    Milliliters,
    #[default]
    Liters,
    UsFluidOunces,
    UsQuarts,
    UsGallons,
    Grams,
    Kilograms,
}

impl ContainerUnit {
    const ALL: [Self; 7] = [
        Self::Milliliters,
        Self::Liters,
        Self::UsFluidOunces,
        Self::UsQuarts,
        Self::UsGallons,
        Self::Grams,
        Self::Kilograms,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Milliliters => "mL",
            Self::Liters => "L",
            Self::UsFluidOunces => "US fl oz",
            Self::UsQuarts => "US qt",
            Self::UsGallons => "US gal",
            Self::Grams => "g",
            Self::Kilograms => "kg",
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Finish {
    #[serde(default)]
    pub id: ketchup_core::MaterialId,
    pub name: String,
    pub finish_type: FinishType,
    pub container_size: f64,
    pub container_unit: ContainerUnit,
    pub price: f64,
    /// Single-coat coverage of the configured container, in square metres.
    pub coverage_m2_per_container: Option<f64>,
}

impl Default for Finish {
    fn default() -> Self {
        Self {
            id: ketchup_core::MaterialId::new_v4(),
            name: "Finishing product".into(),
            finish_type: FinishType::default(),
            container_size: 1.,
            container_unit: ContainerUnit::default(),
            price: 0.,
            coverage_m2_per_container: None,
        }
    }
}

pub(super) fn normalize(finishes: &mut [Finish]) {
    let mut used = BTreeSet::new();
    for finish in finishes {
        if finish.id.is_nil() || !used.insert(finish.id) {
            finish.id = ketchup_core::MaterialId::new_v4();
            used.insert(finish.id);
        }
        finish.container_size = if finish.container_size.is_finite() {
            finish.container_size.clamp(0.001, 1_000_000.)
        } else {
            1.
        };
        finish.price = if finish.price.is_finite() {
            finish.price.clamp(0., 1e9)
        } else {
            0.
        };
        finish.coverage_m2_per_container = finish
            .coverage_m2_per_container
            .filter(|value| value.is_finite() && *value > 0.);
    }
}

fn matches_query(product: &Finish, query: &str) -> bool {
    let candidate = format!("{} {}", product.name, product.finish_type.label()).to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| candidate.contains(word))
}

pub(super) fn selector(
    ui: &mut egui::Ui,
    key: &str,
    selected_id: &mut Option<ketchup_core::MaterialId>,
    products: &[Finish],
    filter: &mut String,
) -> egui::Response {
    let id = (key, "product");
    let was_open = egui::ComboBox::is_open(ui.ctx(), ui.make_persistent_id(id));
    let selected = selected_id.map_or("No product", |id| {
        products
            .iter()
            .find(|product| product.id == id)
            .map_or("Unavailable product", |product| product.name.as_str())
    });
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(ui.available_width())
        .truncate()
        .height(f32::INFINITY)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            // Match the currency picker: fixed search row, independently scrolling results.
            let width = 260.;
            let row_height = ui.spacing().interact_size.y.max(26.);
            let mut query_changed = false;
            let search = ui
                .allocate_ui_with_layout(
                    egui::vec2(width, row_height),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let clear_width = 48.;
                        let search_width = width - clear_width - ui.spacing().item_spacing.x;
                        let search = ui.add_sized(
                            [search_width, row_height],
                            egui::TextEdit::singleline(filter)
                                .id_salt((key, "product_search"))
                                .return_key(None)
                                .hint_text("Search name or type…")
                                .desired_width(search_width),
                        );
                        if ui
                            .add_enabled(
                                !filter.is_empty(),
                                egui::Button::new("Clear")
                                    .min_size(egui::vec2(clear_width, row_height)),
                            )
                            .clicked()
                        {
                            filter.clear();
                            query_changed = true;
                            search.request_focus();
                        }
                        search
                    },
                )
                .inner;
            if !was_open {
                search.request_focus();
            }
            query_changed |= search.changed();
            ui.separator();
            let choices: Vec<_> = products
                .iter()
                .filter(|product| matches_query(product, filter))
                .collect();
            if search.has_focus()
                && choices.len() == 1
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter))
            {
                *selected_id = Some(choices[0].id);
                ui.close();
            }
            let scroll_key = format!("{key}/product/results");
            let mut results = sidebar::restored_scroll_area(
                ui,
                &scroll_key,
                egui::ScrollArea::vertical()
                    .id_salt(&scroll_key)
                    .max_height(ui.spacing().combo_height),
            );
            if query_changed {
                results = results.vertical_scroll_offset(0.);
            }
            let output = results.show(ui, |ui| {
                // Clearing the assignment stays available even with an unmatched search.
                if ui
                    .selectable_value(selected_id, None, "No product")
                    .clicked()
                {
                    ui.close();
                }
                if choices.is_empty() {
                    ui.weak("No products match your search.");
                }
                for product in choices {
                    ui.push_id(product.id, |ui| {
                        if ui
                            .selectable_value(selected_id, Some(product.id), &product.name)
                            .on_hover_text(product.finish_type.label())
                            .clicked()
                        {
                            ui.close();
                        }
                    });
                }
            });
            sidebar::remember_scroll(ui.ctx(), &scroll_key, output.state.clamped_offset());
        })
        .response
}

pub(super) fn show(ui: &mut egui::Ui, finishes: &mut Vec<Finish>, currency: &str) {
    sidebar::inspector_section(
        ui,
        "Finishing products",
        "settings/stock/finishes",
        true,
        |ui| {
            ui.weak("Pre-stain conditioner, stain, paint, varnish, and other finishes. Enter the quantity and price for one container.");
            if finishes.is_empty() {
                ui.weak("No finishing products yet. Add a product to save its type, container size, and price.");
            } else {
                let key = "settings/stock/finishes/table";
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
                        Column::at_least(150.),
                        Column::at_least(100.),
                        Column::at_least(66.),
                        Column::at_least(80.),
                        Column::at_least(112.),
                        Column::at_least(110.),
                        Column::fixed(28.),
                    ],
                )
                .striped();
                let mut remove = None;
                let output = scroll.show(ui, |ui| {
                    table.header(
                        ui,
                        [
                            "Product",
                            "Type",
                            "Quantity",
                            "Unit",
                            "Price / container",
                            "m² / container / coat",
                            "",
                        ],
                    );
                    for finish in finishes.iter_mut() {
                        table.row(ui, finish.id, |column, ui| match column {
                            0 => {
                                ui.add(
                                    egui::TextEdit::singleline(&mut finish.name)
                                        .hint_text("e.g. Golden oak stain")
                                        .char_limit(120),
                                );
                            }
                            1 => {
                                egui::ComboBox::from_id_salt("type")
                                    .width(ui.available_width())
                                    .truncate()
                                    .selected_text(finish.finish_type.label())
                                    .show_ui(ui, |ui| {
                                        for kind in FinishType::ALL {
                                            ui.selectable_value(
                                                &mut finish.finish_type,
                                                kind,
                                                kind.label(),
                                            );
                                        }
                                    });
                            }
                            2 => {
                                ui.add(
                                    egui::DragValue::new(&mut finish.container_size)
                                        .range(0.001..=1_000_000.)
                                        .speed(0.1)
                                        .max_decimals(3),
                                );
                            }
                            3 => {
                                egui::ComboBox::from_id_salt("unit")
                                    .width(ui.available_width())
                                    .truncate()
                                    .selected_text(finish.container_unit.label())
                                    .show_ui(ui, |ui| {
                                        for unit in ContainerUnit::ALL {
                                            ui.selectable_value(
                                                &mut finish.container_unit,
                                                unit,
                                                unit.label(),
                                            );
                                        }
                                    });
                            }
                            4 => {
                                ui.add(currency::price_input(currency, &mut finish.price))
                                    .on_hover_text(format!("{currency} per container"));
                            }
                            5 => {
                                let mut coverage = finish.coverage_m2_per_container.unwrap_or(0.);
                                if ui.add(egui::DragValue::new(&mut coverage)
                                    .range(0.0..=1_000_000.)
                                    .speed(0.1)
                                    .max_decimals(3))
                                    .on_hover_text("Area covered by one container for one coat. Use the product label; 0 means unknown. Update this when changing the container size or unit.")
                                    .changed()
                                {
                                    finish.coverage_m2_per_container = (coverage > 0.).then_some(coverage);
                                }
                            }
                            _ => {
                                if ui
                                    .button("×")
                                    .on_hover_text(format!("Remove {}", finish.name))
                                    .clicked()
                                {
                                    remove = Some(finish.id);
                                }
                            }
                        });
                    }
                });
                sidebar::remember_scroll(ui.ctx(), key, output.state.clamped_offset());
                if let Some(id) = remove {
                    finishes.retain(|finish| finish.id != id);
                }
                ui.weak(format!("Prices in {currency} per container · Coverage in m² per container for one coat · 0 means unknown"));
            }
            if ui.button("+ Add finishing product").clicked() {
                finishes.push(Finish::default());
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_products_get_defaults_and_invalid_values_are_repaired() {
        let mut finish: Finish = serde_json::from_str(r#"{"name":"Oak stain"}"#).unwrap();
        assert_eq!(finish.finish_type, FinishType::Stain);
        assert_eq!(finish.container_size, 1.);
        assert_eq!(finish.container_unit, ContainerUnit::Liters);
        assert_eq!(finish.price, 0.);
        assert_eq!(finish.coverage_m2_per_container, None);
        assert!(finish.id.is_nil());
        finish.container_size = f64::NAN;
        finish.price = f64::INFINITY;
        let mut finishes = vec![finish];
        normalize(&mut finishes);
        assert!(!finishes[0].id.is_nil());
        assert_eq!(finishes[0].container_size, 1.);
        assert_eq!(finishes[0].price, 0.);
        finishes[0].coverage_m2_per_container = Some(f64::NAN);
        normalize(&mut finishes);
        assert_eq!(finishes[0].coverage_m2_per_container, None);
        finishes.push(finishes[0].clone());
        finishes[0].container_size = -1.;
        finishes[0].price = -10.;
        let id = finishes[0].id;
        normalize(&mut finishes);
        assert_eq!(finishes[0].id, id);
        assert_ne!(finishes[1].id, id);
        assert_eq!(finishes[0].container_size, 0.001);
        assert_eq!(finishes[0].price, 0.);
    }

    #[test]
    fn existing_wood_catalogs_gain_an_empty_finish_catalog() {
        let mut settings: stock_settings::Settings =
            serde_json::from_str(r#"{"currency":"CAD","wood":[{"name":"Oak","price":12.50}]}"#)
                .unwrap();
        settings.normalize();
        assert!(settings.finishes.is_empty());
        assert_eq!(settings.currency, "CAD");
        assert_eq!(settings.wood.len(), 1);
        assert_eq!(settings.wood[0].name, "Oak");
        assert_eq!(settings.wood[0].price, 12.50);
    }
}
