use super::*;
use ketchup_core::{DVec3, Face, Vertex};

fn rectangle(model: &mut Model, offset: DVec3) -> u64 {
    let corners = [
        DVec3::ZERO,
        DVec3::X * 2.,
        DVec3::new(2., 3., 0.),
        DVec3::Y * 3.,
    ];
    for i in 0..4 {
        model
            .add_edge(corners[i] + offset, corners[(i + 1) % 4] + offset)
            .unwrap();
    }
    model
        .faces
        .values()
        .find(|face| {
            face.vertices
                .iter()
                .all(|id| model.vertices[id].position.x >= offset.x)
        })
        .unwrap()
        .id
}

fn wood_catalog() -> stock_settings::Settings {
    stock_settings::Settings {
        wood: vec![stock_settings::Stock {
            id: ketchup_core::MaterialId::nil(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn assign_wood(model: &mut Model, object: u64) {
    let stock = wood_catalog().wood.remove(0);
    model.stock_materials.insert(stock.id, stock.definition());
    model.objects.get_mut(&object).unwrap().stock_material_id = Some(stock.id);
}

fn wood_area(model: &Model, selected: &BTreeSet<u64>) -> f64 {
    EstimateCache::default().area(model, selected, &wood_catalog())
}

#[test]
fn cost_table_keeps_long_cells_inside_columns_and_wrapped_rows_apart() {
    for width in [200., 280., 420., 640.] {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(width);
            let mut table = InspectorTable::new(
                width,
                [
                    Column::flex(0.4),
                    Column::flex(0.14),
                    Column::flex(0.23),
                    Column::flex(0.23),
                ],
            );
            let mut rows = Vec::new();
            for row_index in 0..3 {
                let mut cells = Vec::new();
                let row = table.row(ui, row_index, |column, cell| {
                    let bounds = cell.max_rect();
                    if column == 0 {
                        cell.add(
                            egui::Label::new("Long material name with details and dimensions")
                                .wrap(),
                        );
                        cell.add(
                            egui::Label::new("12 cuts do not fit · Excluded from cost").wrap(),
                        );
                    } else {
                        cell.add(egui::Label::new("CAD 1234567890.00").truncate());
                    }
                    // Painted text cannot spill into an adjacent column, even
                    // when its intrinsic width is larger than the entire table.
                    assert!(cell.clip_rect().left() >= bounds.left());
                    assert!(cell.clip_rect().right() <= bounds.right());
                    cells.push((bounds.left(), bounds.right(), cell.min_rect().bottom()));
                });
                assert!((row.width() - width).abs() < 0.1);
                for pair in cells.windows(2) {
                    assert!(pair[0].1 < pair[1].0);
                }
                for (_, _, bottom) in cells {
                    assert!(
                        bottom <= row.bottom() + 0.1,
                        "row {row_index} at width {width}"
                    );
                }
                rows.push(row);
                ui.add_space(8.);
            }
            for pair in rows.windows(2) {
                assert!(pair[0].bottom() <= pair[1].top());
            }
        });
    }
}

#[test]
fn scope_expands_nested_objects_deduplicates_faces_and_excludes_hidden_surfaces() {
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO);
    let child = model
        .make_object(&BTreeSet::from([face]), true, "Panel")
        .unwrap();
    let parent = model
        .make_object(&BTreeSet::from([child]), false, "Assembly")
        .unwrap();
    // Group creation copies entities, so use the resulting nested IDs.
    let contents = model.expand_entities(&BTreeSet::from([parent]));
    let child = *contents
        .iter()
        .find(|id| **id != parent && model.objects.contains_key(id))
        .unwrap();
    let face = *contents
        .iter()
        .find(|id| model.faces.contains_key(id))
        .unwrap();
    assign_wood(&mut model, parent);
    rectangle(&mut model, DVec3::X * 10.);
    let other = model
        .make_object(
            &model
                .root_entities()
                .difference(&BTreeSet::from([parent]))
                .copied()
                .collect(),
            false,
            "Other",
        )
        .unwrap();
    assign_wood(&mut model, other);
    assert!((wood_area(&model, &BTreeSet::new()) - 12.).abs() < 1e-8);
    assert!((wood_area(&model, &BTreeSet::from([parent, child, face])) - 6.).abs() < 1e-8);
    model.hidden.insert(parent);
    assert_eq!(wood_area(&model, &BTreeSet::from([parent])), 0.);
    assert!((wood_area(&model, &BTreeSet::new()) - 6.).abs() < 1e-8);
    model.hidden.insert(other);
    assert_eq!(wood_area(&model, &BTreeSet::new()), 0.);
}

#[test]
fn area_subtracts_holes_and_does_not_double_front_and_back() {
    let mut model = Model::default();
    for (id, x, y) in [
        (1, 0., 0.),
        (2, 4., 0.),
        (3, 4., 4.),
        (4, 0., 4.),
        (5, 1., 1.),
        (6, 2., 1.),
        (7, 2., 2.),
        (8, 1., 2.),
    ] {
        model.vertices.insert(
            id,
            Vertex {
                id,
                position: DVec3::new(x, y, 0.),
            },
        );
    }
    model.faces.insert(
        9,
        Face {
            id: 9,
            vertices: vec![1, 2, 3, 4],
            holes: vec![vec![5, 6, 7, 8]],
        },
    );
    let object = model
        .make_object(&model.root_entities(), false, "Panel")
        .unwrap();
    assign_wood(&mut model, object);
    assert!((wood_area(&model, &BTreeSet::new()) - 15.).abs() < 1e-8);
}

#[test]
fn empty_model_and_non_surface_selection_have_no_area() {
    let mut model = Model::default();
    assert_eq!(wood_area(&model, &BTreeSet::new()), 0.);
    rectangle(&mut model, DVec3::ZERO);
    let edge = *model.edges.keys().next().unwrap();
    let vertex = *model.vertices.keys().next().unwrap();
    assert_eq!(wood_area(&model, &BTreeSet::from([edge, vertex])), 0.);
}

#[test]
fn selected_grits_and_multiple_coats_sum_work_and_all_drying_including_final_coat() {
    let mut settings = Settings::default();
    assert!(!settings.any_enabled());
    assert_eq!(settings.estimate(2.), Duration::default());
    settings.sanding[0].enabled = true;
    settings.sanding[0].minutes_per_m2 = 8.;
    settings.sanding[3].enabled = true;
    settings.sanding[3].minutes_per_m2 = 5.;
    settings.pre_stain.enabled = true;
    settings.stain.enabled = true;
    settings.varnish.enabled = true;
    assert!(settings.any_enabled());
    assert_eq!(
        settings.estimate(2.),
        Duration {
            work: 82.,
            wait: 2655.
        }
    );
    settings.varnish.enabled = false;
    assert_eq!(
        settings.estimate(2.),
        Duration {
            work: 46.,
            wait: 495.
        }
    );
    assert_eq!(settings.estimate(0.), Duration::default());
}

#[test]
fn settings_defaults_partial_data_and_invalid_values_restore_safely() {
    let mut settings: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings, Settings::default());
    assert_eq!(settings.pre_stain.minutes_per_m2, 5.);
    assert_eq!(settings.stain.drying_hours, 8.);
    assert_eq!(settings.varnish.coats, 3);
    let partial: Settings = serde_json::from_str(r#"{"stain":{"enabled":true}}"#).unwrap();
    assert!(partial.stain.enabled);
    assert_eq!(partial.stain.product_id, None);
    assert_eq!(partial.varnish, Settings::default().varnish);
    settings.sanding[0].coats = 12;
    settings.sanding[0].drying_hours = 2.;
    settings.sanding[0].product_id = Some(ketchup_core::MaterialId::new_v4());
    settings.pre_stain.minutes_per_m2 = f64::NAN;
    settings.stain.drying_hours = f64::INFINITY;
    settings.varnish.minutes_per_m2 = -2.;
    settings.varnish.coats = 0;
    settings.stain.coats = 101;
    settings.sanitize();
    assert_eq!(settings.sanding[0], Step::default());
    assert_eq!(settings.pre_stain.minutes_per_m2, 5.);
    assert_eq!(settings.stain.drying_hours, 8.);
    assert_eq!(settings.varnish.minutes_per_m2, 6.);
    assert_eq!(settings.varnish.coats, 1);
    assert_eq!(settings.stain.coats, 100);
    settings.stain.enabled = true;
    settings.stain.minutes_per_m2 = 0.;
    settings.stain.drying_hours = 0.;
    settings.sanitize();
    assert_eq!(settings.estimate(2.), Duration::default());
}

#[test]
fn duration_formatting_handles_rounding_small_work_and_overflow() {
    assert_eq!(format_minutes(0.), "0 h 0 min");
    assert_eq!(format_minutes(0.1), "<1 min");
    assert_eq!(format_minutes(59.9), "1 h 0 min");
    assert_eq!(format_minutes(1501.), "25 h 1 min");
    assert_eq!(format_minutes(f64::INFINITY), "Exceeds estimate range");
}

#[test]
fn researched_defaults_have_explicit_rates_and_separate_recoat_and_final_waits() {
    let settings = Settings::default();
    assert!(settings.sanding.iter().all(|s| s.minutes_per_m2 == 16.));
    assert_eq!(settings.pre_stain.minutes_per_m2, 5.);
    assert_eq!(settings.pre_stain.drying_hours, 0.25);
    assert_eq!(settings.stain.minutes_per_m2, 5.);
    assert_eq!(settings.stain.drying_hours, 8.);
    assert_eq!(settings.varnish.minutes_per_m2, 6.);
    assert_eq!(settings.varnish.drying_hours, 6.);
    assert_eq!(settings.varnish.final_drying_hours, Some(24.));
    let mut varnish = settings.varnish;
    varnish.enabled = true;
    assert_eq!(
        varnish.estimate(1.),
        Duration {
            work: 18.,
            wait: 2160.
        }
    );
    varnish.coats = 1;
    assert_eq!(
        varnish.estimate(1.),
        Duration {
            work: 6.,
            wait: 1440.
        }
    );
    varnish.final_drying_hours = Some(0.);
    assert_eq!(varnish.estimate(1.).wait, 0.);
    varnish.final_drying_hours = Some(f64::NAN);
    varnish.sanitize(&Settings::default().varnish, false);
    assert_eq!(varnish.final_drying_hours, Some(24.));
}

#[test]
fn old_saves_keep_custom_waits_and_settings_roundtrip() {
    let mut older: Settings = serde_json::from_str(
        r#"{
        "varnish":{"enabled":true,"minutes_per_m2":12,"coats":3,"drying_hours":18}
    }"#,
    )
    .unwrap();
    older.sanitize();
    assert_eq!(older.varnish.final_drying_hours, None);
    assert_eq!(
        older.varnish.estimate(1.),
        Duration {
            work: 36.,
            wait: 3240.
        }
    );
    let roundtrip: Settings =
        serde_json::from_str(&serde_json::to_string(&older).unwrap()).unwrap();
    assert_eq!(roundtrip, older);
}

#[test]
fn task_breakdown_lists_only_selected_tasks_and_matches_the_total() {
    let mut settings = Settings::default();
    assert_eq!(settings.task_estimates(2.).count(), 0);
    settings.sanding[1].enabled = true;
    settings.sanding[4].enabled = true;
    settings.stain.enabled = true;
    settings.varnish.enabled = true;
    let tasks: Vec<_> = settings.task_estimates(2.).collect();
    assert_eq!(
        tasks
            .iter()
            .map(|(title, _)| title.as_str())
            .collect::<Vec<_>>(),
        [
            "Sanding · 120 grit",
            "Sanding · 220 grit",
            "Stain · coat 1",
            "Varnish · coat 1",
            "Varnish · coat 2",
            "Varnish · coat 3"
        ]
    );
    assert_eq!(
        tasks[0].1,
        Duration {
            work: 32.,
            wait: 0.
        }
    );
    assert_eq!(
        tasks[2].1,
        Duration {
            work: 10.,
            wait: 480.
        }
    );
    assert_eq!(
        tasks[3].1,
        Duration {
            work: 12.,
            wait: 360.
        }
    );
    let combined = tasks
        .iter()
        .fold(Duration::default(), |total, (_, task)| Duration {
            work: total.work + task.work,
            wait: total.wait + task.wait,
        });
    assert_eq!(combined, settings.estimate(2.));
    settings.stain.enabled = false;
    assert_eq!(settings.task_estimates(2.).count(), 5);
    assert!(
        settings
            .task_estimates(0.)
            .all(|(_, duration)| duration == Duration::default())
    );
}

#[test]
fn after_varnish_grits_repeat_only_between_enabled_varnish_coats() {
    let mut settings = Settings::default();
    settings.sanding[0].enabled = true;
    settings.sanding[0].minutes_per_m2 = 4.;
    settings.sanding[4].enabled = true;
    settings.sanding[4].minutes_per_m2 = 2.;
    settings.sanding_after_varnish[4] = true;
    // Without varnish, only the initial sanding is scheduled.
    assert_eq!(settings.estimate(2.), Duration { work: 8., wait: 0. });
    settings.varnish.enabled = true;
    settings.varnish.coats = 3;
    // 8 initial sanding + 36 varnish + 8 sanding between the first two coats.
    assert_eq!(
        settings.estimate(2.),
        Duration {
            work: 52.,
            wait: 2160.
        }
    );
    let tasks: Vec<_> = settings.task_estimates(2.).collect();
    assert_eq!(tasks.len(), 6);
    assert_eq!(
        tasks
            .iter()
            .map(|(title, _)| title.as_str())
            .collect::<Vec<_>>(),
        [
            "Sanding · 80 grit",
            "Varnish · coat 1",
            "Sanding · 220 grit (after coat 1)",
            "Varnish · coat 2",
            "Sanding · 220 grit (after coat 2)",
            "Varnish · coat 3",
        ]
    );
    assert_eq!(tasks[2].1, Duration { work: 4., wait: 0. });
    assert_eq!(tasks[4].1, Duration { work: 4., wait: 0. });
    assert_eq!(
        tasks[5].1,
        Duration {
            work: 12.,
            wait: 1440.
        }
    );
    settings.varnish.coats = 2;
    assert_eq!(
        settings.estimate(2.),
        Duration {
            work: 36.,
            wait: 1800.
        }
    );
    settings.varnish.coats = 1;
    assert_eq!(
        settings.estimate(2.),
        Duration {
            work: 20.,
            wait: 1440.
        }
    );
    assert_eq!(settings.task_estimates(2.).count(), 2);
    settings.varnish.coats = 3;
    settings.sanding[4].enabled = false;
    assert_eq!(
        settings.estimate(2.),
        Duration {
            work: 44.,
            wait: 2160.
        }
    );
    assert!(
        settings
            .task_estimates(0.)
            .all(|(_, duration)| duration == Duration::default())
    );
}

#[test]
fn after_varnish_flags_default_off_and_persist_without_affecting_initial_sanding() {
    let mut settings: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.sanding_after_varnish, [false; 6]);
    settings.sanding[5].enabled = true;
    settings.sanding_after_varnish[5] = true;
    // A selected between-coat grit alone creates no work until varnish is enabled.
    assert!(!settings.any_enabled());
    assert_eq!(settings.estimate(2.), Duration::default());
    settings.varnish.enabled = true;
    settings.varnish.coats = 4;
    settings.sanding_after_varnish[4] = true; // Disabled grit stays excluded.
    let restored: Settings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(restored, settings);
    assert_eq!(restored.task_estimates(2.).count(), 7);
    assert_eq!(restored.estimate(2.).work, 48. + 96.);
    settings.sanding_after_varnish[5] = false;
    assert_eq!(settings.estimate(2.).work, 48. + 32.);
}

#[test]
fn every_finish_coat_is_separate_with_final_wait_only_on_its_last_coat() {
    let mut settings = Settings::default();
    for step in [
        &mut settings.pre_stain,
        &mut settings.stain,
        &mut settings.varnish,
    ] {
        step.enabled = true;
        step.coats = 2;
        step.minutes_per_m2 = 3.;
        step.drying_hours = 1.;
        step.final_drying_hours = Some(4.);
    }
    let tasks: Vec<_> = settings.task_estimates(2.).collect();
    assert_eq!(
        tasks
            .iter()
            .map(|(title, _)| title.as_str())
            .collect::<Vec<_>>(),
        [
            "Pre-stain · coat 1",
            "Pre-stain · coat 2",
            "Stain · coat 1",
            "Stain · coat 2",
            "Varnish · coat 1",
            "Varnish · coat 2",
        ]
    );
    for (index, (_, duration)) in tasks.iter().enumerate() {
        assert_eq!(duration.work, 6.);
        assert_eq!(duration.wait, if index % 2 == 0 { 60. } else { 240. });
    }
    assert_eq!(
        settings.estimate(2.),
        Duration {
            work: 36.,
            wait: 900.
        }
    );
}

#[test]
fn material_estimate_prices_purchased_stock_and_flags_missing_prices() {
    fn labels(app: &mut KetchupApp) -> String {
        fn collect(shape: &egui::Shape, text: &mut String) {
            match shape {
                egui::Shape::Text(label) => {
                    text.push_str(&label.galley.job.text);
                    text.push('\n');
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, text);
                    }
                }
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.material_cost_estimate_contents(ui);
        });
        let mut text = String::new();
        for shape in output.shapes {
            collect(&shape.shape, &mut text);
        }
        text
    }
    let mut doc = Document::default();
    let face = rectangle(&mut doc.model, DVec3::ZERO);
    doc.model.push_pull(face, 0.019).unwrap();
    let stock = stock_settings::Stock {
        name: "Panel stock".into(),
        thickness_mm: 19.,
        width_mm: 3000.,
        length_mm: 4000.,
        price: 42.5,
        ..Default::default()
    };
    let group = doc
        .model
        .make_object(&doc.model.root_entities(), false, "Panel")
        .unwrap();
    doc.model.objects.get_mut(&group).unwrap().stock_material_id = Some(stock.id);
    doc.model
        .stock_materials
        .insert(stock.id, stock.definition());
    let mut app = KetchupApp::with_document(doc, None, true);
    app.general_settings.stock.currency = "CAD ".into();
    app.general_settings.stock.wood = vec![stock];
    // Unassigned solids and loose faces must not make the estimate incomplete.
    let unassigned = rectangle(&mut app.doc.model, DVec3::X * 10.);
    app.doc.model.push_pull(unassigned, 0.019).unwrap();
    rectangle(&mut app.doc.model, DVec3::X * 20.);
    let before = app.doc.model.clone();
    let text = labels(&mut app);
    assert_eq!(app.estimate_cache.material_recomputations, 1);
    assert_eq!(labels(&mut app), text);
    assert_eq!(app.estimate_cache.material_recomputations, 1);
    for label in ["Quantity", "Unit price (CAD)", "Amount (CAD)"] {
        assert!(text.contains(label), "Missing {label}: {text}");
    }
    assert!(!text.contains("Estimate incomplete"), "{text}");
    assert!(!text.contains("Material total"), "{text}");
    assert_eq!(
        text.matches("CAD").count(),
        2,
        "Currency belongs in table headers: {text}"
    );
    for (currency, price, expected) in [
        ("EUR", 42.5, "42.50"),
        ("JPY", 1234.6, "1235"),
        ("GBP", 42.5, "42.50"),
    ] {
        app.general_settings.stock.currency = currency.into();
        app.general_settings.stock.wood[0].price = price;
        let text = labels(&mut app);
        assert!(text.contains(&format!("Unit price ({currency})")), "{text}");
        assert!(text.contains(&format!("Amount ({currency})")), "{text}");
        assert_eq!(
            text.matches(currency).count(),
            2,
            "Currency belongs in table headers: {text}"
        );
        assert_eq!(
            text.lines()
                .filter(|line| *line == format!("{currency} {expected}"))
                .count(),
            0,
            "The detail must not repeat the KPI total: {text}"
        );
        assert_eq!(
            text.lines().filter(|line| *line == expected).count(),
            2,
            "Unit price and amount must stay numeric and use the selected currency's precision: {text}"
        );
        assert!(!text.contains("CAD"), "{text}");
        assert_eq!(app.estimate_cache.material_recomputations, 1);
    }
    app.general_settings.stock.wood.clear();
    let text = labels(&mut app);
    assert_eq!(app.estimate_cache.material_recomputations, 1);
    assert!(text.contains("Price unavailable"), "{text}");
    assert!(!text.contains("Priced subtotal"), "{text}");
    assert!(!text.lines().any(|line| line == "GBP 0.00"), "{text}");
    assert_eq!(
        text.matches("GBP").count(),
        2,
        "Currency belongs in table headers: {text}"
    );
    assert!(text.contains("Estimate incomplete"), "{text}");
    assert_eq!(app.doc.model, before);
    assert!(!app.project.modified);
}

#[test]
fn estimate_cache_tracks_geometry_selection_visibility_and_document_replacement() {
    let mut model = Model::default();
    rectangle(&mut model, DVec3::ZERO);
    rectangle(&mut model, DVec3::X * 10.);
    let object = model
        .make_object(&model.root_entities(), false, "Wood")
        .unwrap();
    assign_wood(&mut model, object);
    let first = model
        .faces
        .values()
        .find(|face| model.vertices[&face.vertices[0]].position.x < 5.)
        .unwrap()
        .id;
    let second = model
        .faces
        .values()
        .find(|face| model.vertices[&face.vertices[0]].position.x >= 5.)
        .unwrap()
        .id;
    let mut cache = EstimateCache::default();
    let all = BTreeSet::new();
    assert_eq!(cache.area(&model, &all, &wood_catalog()), 12.);
    assert_eq!(
        cache.area(&model, &BTreeSet::from([first]), &wood_catalog()),
        6.
    );
    model.hidden.insert(first);
    assert_eq!(
        cache.area(&model, &BTreeSet::from([first]), &wood_catalog()),
        0.
    );
    assert_eq!(cache.area(&model, &all, &wood_catalog()), 6.);
    let vertices = model.faces[&second].vertices.clone();
    for id in vertices {
        model.vertices.get_mut(&id).unwrap().position.y *= 2.;
    }
    assert_eq!(cache.area(&model, &all, &wood_catalog()), 12.);
    assert_eq!(cache.area(&Model::default(), &all, &wood_catalog()), 0.);
}

#[test]
fn material_cache_reuses_failures_and_invalidates_for_layout_selection_and_model_edits() {
    let mut cache = EstimateCache::default();
    let mut model = Model::default();
    let all = BTreeSet::new();
    let mut layout = crate::cutting_layout::Settings::default();
    assert!(cache.materials(&model, &all, layout).is_err());
    assert!(cache.materials(&model, &all, layout).is_err());
    assert_eq!(cache.material_recomputations, 1);
    layout.kerf_mm += 1.;
    let _ = cache.materials(&model, &all, layout);
    assert_eq!(cache.material_recomputations, 2);
    let face = rectangle(&mut model, DVec3::ZERO);
    let _ = cache.materials(&model, &all, layout);
    assert_eq!(cache.material_recomputations, 3);
    let _ = cache.materials(&model, &BTreeSet::from([face]), layout);
    assert_eq!(cache.material_recomputations, 4);
    model.hidden.insert(face);
    let _ = cache.materials(&model, &BTreeSet::from([face]), layout);
    assert_eq!(cache.material_recomputations, 5);
    let _ = cache.materials(&Model::default(), &all, layout);
    assert_eq!(cache.material_recomputations, 6);
}

#[test]
fn finishing_requires_wood_and_respects_child_overrides_and_catalog_changes() {
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO);
    let child = model
        .make_object(&BTreeSet::from([face]), false, "Child")
        .unwrap();
    let parent = model
        .make_object(&BTreeSet::from([child]), false, "Assembly")
        .unwrap();
    let child = model
        .objects
        .values()
        .find(|object| object.name == "Child")
        .unwrap()
        .id;
    let loose = rectangle(&mut model, DVec3::X * 10.);
    let mut cache = EstimateCache::default();
    let mut catalog = wood_catalog();
    let all = BTreeSet::new();
    assert_eq!(cache.area(&model, &all, &catalog), 0.);
    assign_wood(&mut model, parent);
    assert_eq!(cache.area(&model, &all, &catalog), 6.);
    assert_eq!(cache.area(&model, &BTreeSet::from([loose]), &catalog), 0.);
    // A valid purchasing material outside the Wood category is not finishable,
    // even when its parent is assigned wood.
    let other = ketchup_core::MaterialId::new_v4();
    model
        .stock_materials
        .insert(other, catalog.wood[0].definition());
    model.objects.get_mut(&child).unwrap().stock_material_id = Some(other);
    assert_eq!(cache.area(&model, &all, &catalog), 0.);
    model.objects.get_mut(&child).unwrap().stock_material_id = None;
    assert_eq!(cache.area(&model, &all, &catalog), 6.);
    catalog.wood.clear();
    assert_eq!(cache.area(&model, &all, &catalog), 0.);
    catalog = wood_catalog();
    assert_eq!(cache.area(&model, &all, &catalog), 6.);
    // Legacy inline wood assignments follow the same rules before migration.
    let object = model.objects.get_mut(&parent).unwrap();
    object.stock_material_id = None;
    object.stock_material = Some(catalog.wood[0].definition());
    assert_eq!(cache.area(&model, &all, &catalog), 6.);
}

#[test]
fn material_estimate_ignores_unassigned_items_but_reports_invalid_assigned_parts() {
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO);
    model.push_pull(face, 0.019).unwrap();
    let object = model
        .make_object(&model.root_entities(), false, "Wood")
        .unwrap();
    assign_wood(&mut model, object);
    let unassigned = rectangle(&mut model, DVec3::X * 10.);
    model.push_pull(unassigned, 0.019).unwrap();
    let flat = rectangle(&mut model, DVec3::X * 20.);
    let mut cache = EstimateCache::default();
    let layout = crate::cutting_layout::Settings::default();
    let estimate = cache
        .materials(&model, &BTreeSet::new(), layout)
        .as_ref()
        .unwrap();
    assert_eq!(estimate.skipped, 0);
    assert_eq!(estimate.requirements.len(), 1);
    assert!(
        cache
            .materials(&model, &BTreeSet::from([unassigned]), layout)
            .is_err()
    );
    let object = model
        .make_object(&BTreeSet::from([flat]), false, "Assigned flat")
        .unwrap();
    assign_wood(&mut model, object);
    assert_eq!(
        cache
            .materials(&model, &BTreeSet::new(), layout)
            .as_ref()
            .unwrap()
            .skipped,
        1
    );
}

#[test]
fn per_object_treatments_use_individual_areas_and_survive_document_roundtrip() {
    let mut model = Model::default();
    let first_face = rectangle(&mut model, DVec3::ZERO);
    let first = model
        .make_object(&BTreeSet::from([first_face]), false, "Sanded")
        .unwrap();
    let second_face = rectangle(&mut model, DVec3::X * 10.);
    let second = model
        .make_object(&BTreeSet::from([second_face]), false, "Finished")
        .unwrap();
    assign_wood(&mut model, first);
    assign_wood(&mut model, second);
    model
        .objects
        .get_mut(&first)
        .unwrap()
        .surface_treatment
        .sanding[0]
        .enabled = true;
    model
        .objects
        .get_mut(&first)
        .unwrap()
        .surface_treatment
        .sanding[0]
        .minutes_per_m2 = 2.;
    model
        .objects
        .get_mut(&second)
        .unwrap()
        .surface_treatment
        .stain
        .enabled = true;
    let restored: Model = serde_json::from_str(&serde_json::to_string(&model).unwrap()).unwrap();
    assert_eq!(restored, model);
    let mut cache = EstimateCache::default();
    assert_eq!(cache.area(&model, &BTreeSet::new(), &wood_catalog()), 12.);
    let tasks = object_tasks(
        &model,
        &cache.area.as_ref().unwrap().1,
        &Settings::default(),
    );
    assert_eq!(tasks.len(), 2);
    assert_eq!(
        tasks[0].1,
        Duration {
            work: 12.,
            wait: 0.
        }
    );
    assert_eq!(
        tasks[1].1,
        Duration {
            work: 30.,
            wait: 480.
        }
    );
    cache.area(
        &model,
        &BTreeSet::from([first, first_face]),
        &wood_catalog(),
    );
    assert_eq!(
        object_tasks(
            &model,
            &cache.area.as_ref().unwrap().1,
            &Settings::default()
        )
        .len(),
        1
    );
    model.hidden.insert(first);
    assert_eq!(
        cache.area(&model, &BTreeSet::from([first]), &wood_catalog()),
        0.
    );
}

#[test]
fn object_defaults_apply_to_existing_objects_and_explicit_overrides_survive_reload() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let face = rectangle(&mut app.doc.model, DVec3::ZERO);
    app.selected = BTreeSet::from([face]);
    let before = app.doc.model.clone();
    app.create_object(&ctx, false, "Part");
    let id = *app.selected.first().unwrap();
    app.time_estimates.stain.enabled = true;
    let areas = std::collections::BTreeMap::from([(id, 6.)]);
    assert!(
        app.doc.model.objects[&id]
            .surface_treatment_override()
            .is_none()
    );
    let tasks = object_tasks(&app.doc.model, &areas, &app.time_estimates);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].0, "Stain · coat 1");
    // Defaults are live for unconfigured objects, including ones created earlier.
    app.time_estimates.stain.enabled = false;
    assert!(object_tasks(&app.doc.model, &areas, &app.time_estimates).is_empty());
    app.time_estimates.stain.enabled = true;
    // Explicitly disabling every treatment must override enabled defaults.
    app.doc
        .model
        .objects
        .get_mut(&id)
        .unwrap()
        .surface_treatment_customized = true;
    let restored: Model =
        serde_json::from_str(&serde_json::to_string(&app.doc.model).unwrap()).unwrap();
    assert!(object_tasks(&restored, &areas, &app.time_estimates).is_empty());
    app.undo(&ctx, false);
    assert_eq!(app.doc.model, before);
}

#[test]
fn older_objects_default_to_no_treatment() {
    let object: ketchup_core::Object = serde_json::from_value(serde_json::json!({
        "id": 1, "name": "Old part", "kind": "Group", "entities": [], "definition_entities": {}
    }))
    .unwrap();
    assert_eq!(object.surface_treatment, Settings::default());
    assert!(object.surface_treatment_override().is_none());
}

#[test]
fn bulk_treatment_edits_preserve_unedited_object_choices() {
    let before = Settings::default();
    let mut after = before.clone();
    after.sanding[0].enabled = true;
    let product_id = ketchup_core::MaterialId::new_v4();
    after.stain.product_id = Some(product_id);
    let mut target = before.clone();
    target.varnish.enabled = true;
    target.varnish.coats = 5;
    let varnish_id = ketchup_core::MaterialId::new_v4();
    target.varnish.product_id = Some(varnish_id);
    apply_treatment_changes(&mut target, &before, &after);
    assert!(target.sanding[0].enabled);
    assert!(target.varnish.enabled);
    assert_eq!(target.varnish.coats, 5);
    assert_eq!(target.stain.product_id, Some(product_id));
    assert_eq!(target.varnish.product_id, Some(varnish_id));
    // Clearing one product is also a bulk edit, without resetting other steps.
    let mut cleared = after.clone();
    cleared.stain.product_id = None;
    apply_treatment_changes(&mut target, &after, &cleared);
    assert_eq!(target.stain.product_id, None);
    assert_eq!(target.varnish.product_id, Some(varnish_id));
}

#[test]
fn task_groups_sum_work_and_share_the_longest_wait_per_coat() {
    let mut model = Model::default();
    let face = rectangle(&mut model, DVec3::ZERO);
    let first = model
        .make_object(&BTreeSet::from([face]), false, "First")
        .unwrap();
    let face = rectangle(&mut model, DVec3::X * 10.);
    let second = model
        .make_object(&BTreeSet::from([face]), false, "Second")
        .unwrap();
    for (id, rate, wait, final_wait) in [(first, 2., 3., 4.), (second, 5., 8., 12.)] {
        let settings = &mut model.objects.get_mut(&id).unwrap().surface_treatment;
        settings.stain.enabled = true;
        settings.stain.minutes_per_m2 = rate;
        settings.stain.coats = 2;
        settings.stain.drying_hours = wait;
        settings.stain.final_drying_hours = Some(final_wait);
    }
    model
        .objects
        .get_mut(&second)
        .unwrap()
        .surface_treatment
        .sanding[0]
        .enabled = true;
    let areas = std::collections::BTreeMap::from([(first, 2.), (second, 3.)]);
    let tasks = object_tasks(&model, &areas, &Settings::default());
    assert_eq!(tasks.len(), 3);
    assert_eq!(tasks[0].0, "Sanding · 80 grit");
    assert_eq!(tasks[0].2, 1);
    assert_eq!(
        tasks[0].1,
        Duration {
            work: 48.,
            wait: 0.
        }
    );
    assert_eq!(tasks[1].0, "Stain · coat 1");
    assert_eq!(tasks[1].2, 2);
    assert_eq!(
        tasks[1].1,
        Duration {
            work: 19.,
            wait: 480.
        }
    );
    assert_eq!(tasks[2].0, "Stain · coat 2");
    assert_eq!(tasks[2].2, 2);
    assert_eq!(
        tasks[2].1,
        Duration {
            work: 19.,
            wait: 720.
        }
    );
    let total = tasks
        .iter()
        .fold(Duration::default(), |total, (_, task, _)| Duration {
            work: total.work + task.work,
            wait: total.wait + task.wait,
        });
    assert_eq!(
        total,
        Duration {
            work: 86.,
            wait: 1200.
        }
    );
    // Adding an object with the same waits adds work without extending drying.
    let first_stain = model.objects[&first].surface_treatment.stain.clone();
    model
        .objects
        .get_mut(&second)
        .unwrap()
        .surface_treatment
        .stain = first_stain;
    let single = object_tasks(
        &model,
        &std::collections::BTreeMap::from([(first, 2.)]),
        &Settings::default(),
    );
    let multiple = object_tasks(&model, &areas, &Settings::default());
    for (title, duration, _) in single {
        let (_, combined, count) = multiple.iter().find(|(name, _, _)| *name == title).unwrap();
        assert_eq!(*count, 2);
        assert!(combined.work > duration.work);
        assert_eq!(combined.wait, duration.wait);
    }
}

#[test]
fn finishing_costs_pool_coats_and_objects_before_buying_containers() {
    use finish_settings::{ContainerUnit, Finish, FinishType};
    let mut model = Model::default();
    rectangle(&mut model, DVec3::ZERO);
    let first = model
        .make_object(&model.root_entities(), false, "First")
        .unwrap();
    rectangle(&mut model, DVec3::X * 10.);
    let roots = model
        .root_entities()
        .into_iter()
        .filter(|id| *id != first)
        .collect();
    let second = model.make_object(&roots, false, "Second").unwrap();
    let mut products = vec![
        Finish {
            finish_type: FinishType::PreStain,
            price: 10.,
            coverage_m2_per_container: Some(10.),
            ..Default::default()
        },
        Finish {
            finish_type: FinishType::Paint,
            price: 25.,
            coverage_m2_per_container: Some(10.),
            ..Default::default()
        },
        Finish {
            finish_type: FinishType::Varnish,
            price: 30.,
            coverage_m2_per_container: Some(10.),
            ..Default::default()
        },
    ];
    let mut settings = Settings::default();
    for (step, product) in [
        &mut settings.pre_stain,
        &mut settings.stain,
        &mut settings.varnish,
    ]
    .into_iter()
    .zip(&products)
    {
        step.enabled = true;
        step.product_id = Some(product.id);
        step.coats = 1;
    }
    settings.stain.coats = 2;
    let areas = [(first, 3.), (second, 3.)].into_iter().collect();
    for unit in [
        ContainerUnit::Milliliters,
        ContainerUnit::Liters,
        ContainerUnit::UsFluidOunces,
        ContainerUnit::UsQuarts,
        ContainerUnit::UsGallons,
        ContainerUnit::Grams,
        ContainerUnit::Kilograms,
    ] {
        for product in &mut products {
            product.container_unit = unit;
        }
        let costs = finishing_costs(&model, &areas, &settings, &products);
        assert_eq!(costs.len(), 3);
        assert_eq!(
            costs.iter().filter_map(|cost| cost.amount).sum::<f64>(),
            90.
        );
        assert!(costs.iter().all(|cost| cost.issue.is_none()));
    }
    // One product used in two stages is purchased once from the combined demand.
    settings.pre_stain.product_id = Some(products[1].id);
    let costs = finishing_costs(&model, &areas, &settings, &products);
    assert_eq!(costs.len(), 2);
    assert_eq!(
        costs.iter().filter_map(|cost| cost.amount).sum::<f64>(),
        80.
    );
    // Object overrides replace defaults and disabled steps consume no product.
    model
        .objects
        .get_mut(&second)
        .unwrap()
        .surface_treatment_customized = true;
    let costs = finishing_costs(&model, &areas, &settings, &products);
    assert_eq!(
        costs.iter().filter_map(|cost| cost.amount).sum::<f64>(),
        55.
    );
    for kind in [
        FinishType::PreStain,
        FinishType::Stain,
        FinishType::Paint,
        FinishType::Varnish,
        FinishType::Primer,
        FinishType::Sealer,
        FinishType::Lacquer,
        FinishType::Oil,
        FinishType::Wax,
        FinishType::Other,
    ] {
        for product in &mut products {
            product.finish_type = kind;
        }
        assert_eq!(
            finishing_costs(&model, &areas, &settings, &products)
                .iter()
                .filter_map(|cost| cost.amount)
                .sum::<f64>(),
            55.
        );
    }
}

#[test]
fn finishing_costs_report_unknown_products_coverage_and_prices() {
    let mut model = Model::default();
    rectangle(&mut model, DVec3::ZERO);
    let object = model
        .make_object(&model.root_entities(), false, "Panel")
        .unwrap();
    let areas = [(object, 6.)].into_iter().collect();
    let mut settings = Settings::default();
    settings.stain.enabled = true;
    assert!(
        finishing_costs(&model, &areas, &settings, &[])[0]
            .amount
            .is_none()
    );
    let mut product = finish_settings::Finish {
        price: 20.,
        ..Default::default()
    };
    settings.stain.product_id = Some(product.id);
    for coverage in [None, Some(0.), Some(f64::NAN)] {
        product.coverage_m2_per_container = coverage;
        let costs = finishing_costs(&model, &areas, &settings, &[product.clone()]);
        assert!(costs[0].containers.is_none());
        assert!(costs[0].issue.unwrap().starts_with("Coverage missing"));
    }
    product.coverage_m2_per_container = Some(10.);
    for price in [0., -1., f64::NAN] {
        product.price = price;
        let costs = finishing_costs(&model, &areas, &settings, &[product.clone()]);
        assert_eq!(costs[0].containers, Some(1.));
        assert!(costs[0].amount.is_none());
        assert!(costs[0].issue.unwrap().starts_with("Price missing"));
    }
    assert!(
        finishing_costs(&model, &areas, &settings, &[])[0]
            .issue
            .is_some()
    );
    settings.stain.enabled = false;
    assert!(finishing_costs(&model, &areas, &settings, &[product]).is_empty());
}

#[test]
fn unified_cost_table_groups_stock_and_finishes_with_one_header() {
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
    let mut doc = Document::default();
    let face = rectangle(&mut doc.model, DVec3::ZERO);
    doc.model.push_pull(face, 0.019).unwrap();
    let stock = stock_settings::Stock {
        name: "Panel stock".into(),
        thickness_mm: 19.,
        width_mm: 3000.,
        length_mm: 4000.,
        price: 42.5,
        ..Default::default()
    };
    let object = doc
        .model
        .make_object(&doc.model.root_entities(), false, "Panel")
        .unwrap();
    doc.model
        .objects
        .get_mut(&object)
        .unwrap()
        .stock_material_id = Some(stock.id);
    doc.model
        .stock_materials
        .insert(stock.id, stock.definition());
    let paint = finish_settings::Finish {
        name: "White paint".into(),
        finish_type: finish_settings::FinishType::Paint,
        price: 25.,
        coverage_m2_per_container: Some(100.),
        ..Default::default()
    };
    let mut app = KetchupApp::with_document(doc, None, true);
    app.general_settings.stock.wood = vec![stock];
    app.time_estimates.stain.enabled = true;
    app.time_estimates.stain.product_id = Some(paint.id);
    app.general_settings.stock.finishes = vec![paint];
    let ctx = egui::Context::default();
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.set_width(600.);
        app.material_cost_estimate_contents(ui);
    });
    let mut labels = Vec::new();
    for shape in output.shapes {
        collect(&shape.shape, &mut labels);
    }
    for label in [
        "Cost estimate",
        "Material / Product",
        "Quantity",
        "Wood stock",
        "Paint",
    ] {
        assert_eq!(
            labels.iter().filter(|text| text.as_str() == label).count(),
            1,
            "{labels:?}"
        );
    }
    let wood = labels.iter().position(|text| text == "Wood stock").unwrap();
    let paint = labels.iter().position(|text| text == "Paint").unwrap();
    assert!(wood < paint);
    assert!(labels.iter().any(|text| text == "White paint"));
    assert_eq!(labels.iter().filter(|text| text.as_str() == "1").count(), 2);
    assert!(
        !labels
            .iter()
            .any(|text| text.contains("1 container") || text.contains("1 piece"))
    );

    assert!(
        !labels
            .iter()
            .any(|text| text == "Finishing products" || text == "Material total")
    );
}
