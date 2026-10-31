use super::*;

fn catalog() -> Vec<Stock> {
    vec![
        Stock {
            name: "Red oak board".into(),
            ..Default::default()
        },
        Stock {
            name: "Pine sheet".into(),
            thickness_mm: 12.,
            width_mm: 1220.,
            ..Default::default()
        },
    ]
}

#[test]
fn search_matches_names_and_sizes_and_keeps_project_only_materials() {
    let wood = catalog();
    let project_id = ketchup_core::MaterialId::new_v4();
    let project = std::collections::BTreeMap::from([
        (
            wood[0].id,
            ketchup_core::StockMaterial {
                name: "Old catalog name".into(),
                dimensions: [190, 890, 24400],
            },
        ),
        (
            project_id,
            ketchup_core::StockMaterial {
                name: "Walnut offcut".into(),
                dimensions: [190, 890, 24400],
            },
        ),
    ]);
    for (query, ids) in [
        ("", vec![wood[0].id, wood[1].id, project_id]),
        (" OaK RED ", vec![wood[0].id]),
        ("pine 1220", vec![wood[1].id]),
        ("walnut PROJECT", vec![project_id]),
        ("Old catalog name", vec![]),
        ("no such stock", vec![]),
    ] {
        let actual: Vec<_> = material_choices(&wood, &project, query)
            .iter()
            .map(|choice| choice.id)
            .collect();
        assert_eq!(actual, ids, "query: {query}");
    }
}

fn frame(
    ctx: &egui::Context,
    wood: &[Stock],
    selected: &mut Option<ketchup_core::MaterialId>,
    filter: &mut String,
    events: Vec<egui::Event>,
) -> (egui::Pos2, bool, Vec<(String, egui::Pos2)>) {
    fn collect(shape: &egui::Shape, labels: &mut Vec<(String, egui::Pos2)>) {
        match shape {
            egui::Shape::Text(text) => labels.push((
                text.galley.job.text.clone(),
                text.pos + text.galley.rect.center().to_vec2(),
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, labels);
                }
            }
            _ => {}
        }
    }
    let mut center = egui::Pos2::ZERO;
    let mut assigned = false;
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600., 500.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            ui.set_width(300.);
            let picker = material_selector(
                ui,
                "Stock material",
                selected,
                wood,
                &Default::default(),
                filter,
            );
            center = picker.response.rect.center();
            assigned = picker.inner;
        },
    );
    let mut labels = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, &mut labels);
    }
    (center, assigned, labels)
}

fn click(
    ctx: &egui::Context,
    wood: &[Stock],
    selected: &mut Option<ketchup_core::MaterialId>,
    filter: &mut String,
    pos: egui::Pos2,
) -> bool {
    let mut assigned = false;
    for pressed in [true, false] {
        assigned |= frame(
            ctx,
            wood,
            selected,
            filter,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        )
        .1;
    }
    assigned
}

#[test]
fn typing_only_filters_and_enter_assigns_the_single_result() {
    let ctx = egui::Context::default();
    ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
    let wood = catalog();
    let mut selected = Some(wood[1].id);
    let mut filter = String::new();
    frame(&ctx, &wood, &mut selected, &mut filter, vec![]);
    let (center, _, _) = frame(&ctx, &wood, &mut selected, &mut filter, vec![]);
    click(&ctx, &wood, &mut selected, &mut filter, center);
    frame(&ctx, &wood, &mut selected, &mut filter, vec![]);
    assert!(
        !frame(
            &ctx,
            &wood,
            &mut selected,
            &mut filter,
            vec![egui::Event::Text(" RED oak ".into())]
        )
        .1
    );
    let (_, _, labels) = frame(&ctx, &wood, &mut selected, &mut filter, vec![]);
    assert_eq!(selected, Some(wood[1].id));
    assert!(
        labels
            .iter()
            .any(|(text, _)| text == "Red oak board (19 × 89 × 2440 mm)")
    );
    assert!(
        !labels
            .iter()
            .any(|(text, _)| text.starts_with("Pine sheet"))
    );
    assert!(
        frame(
            &ctx,
            &wood,
            &mut selected,
            &mut filter,
            vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]
        )
        .1
    );
    assert_eq!(selected, Some(wood[0].id));
    assert_eq!(filter, " RED oak ");
}

#[test]
fn clear_keeps_picker_open_and_unassigned_remains_available_with_no_matches() {
    let wood = catalog();
    for clear in [true, false] {
        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        let mut selected = Some(wood[0].id);
        let mut filter = "unmatched query".to_owned();
        frame(&ctx, &wood, &mut selected, &mut filter, vec![]);
        let (center, _, _) = frame(&ctx, &wood, &mut selected, &mut filter, vec![]);
        click(&ctx, &wood, &mut selected, &mut filter, center);
        let (_, _, labels) = frame(&ctx, &wood, &mut selected, &mut filter, vec![]);
        assert!(
            labels
                .iter()
                .any(|(text, _)| text == "No stock materials match your search.")
        );
        let target = labels
            .iter()
            .find(|(text, _)| text == if clear { "Clear" } else { "Unassigned" })
            .unwrap()
            .1;
        let assigned = click(&ctx, &wood, &mut selected, &mut filter, target);
        if clear {
            assert!(!assigned);
            assert_eq!(selected, Some(wood[0].id));
            assert!(filter.is_empty());
            let (_, _, labels) = frame(&ctx, &wood, &mut selected, &mut filter, vec![]);
            assert!(
                labels
                    .iter()
                    .any(|(text, _)| text.starts_with("Pine sheet"))
            );
            assert!(ctx.memory(|memory| memory.focused()).is_some());
        } else {
            assert!(assigned);
            assert_eq!(selected, None);
            assert_eq!(filter, "unmatched query");
        }
    }
}
