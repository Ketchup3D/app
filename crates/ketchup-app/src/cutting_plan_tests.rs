use super::*;

fn material() -> ketchup_core::StockMaterial {
    ketchup_core::StockMaterial {
        name: "Catalog oak".into(),
        dimensions: [1000, 12200, 24400],
    }
}

fn assign_raw(model: &mut Model) {
    let roots = model.root_entities();
    let id = model.make_object(&roots, false, "").unwrap();
    model.objects.get_mut(&id).unwrap().stock_material = Some(material());
}

fn blank(size: DVec3) -> Model {
    let mut model = Model::default();
    let corners = [
        DVec3::ZERO,
        DVec3::X * size.x,
        DVec3::new(size.x, size.y, 0.),
        DVec3::Y * size.y,
    ];
    for i in 0..4 {
        model.add_edge(corners[i], corners[(i + 1) % 4]).unwrap();
    }
    let face = *model.faces.keys().next().unwrap();
    let normal = ketchup_core::faces::plane(&model, &model.faces[&face].vertices)
        .unwrap()
        .normal;
    model.push_pull(face, size.z / normal.z).unwrap();
    model
}

fn add_board(model: &mut Model, size: DVec3, offset: DVec3, name: &str, component: bool) -> u64 {
    let entities = model.paste_entities(&blank(size), offset);
    let id = model.make_object(&entities, component, name).unwrap();
    model.objects.get_mut(&id).unwrap().stock_material = Some(material());
    id
}

#[test]
fn groups_twenty_rails_and_two_posts_with_nominal_stock_and_real_cut_lengths() {
    let mut model = Model::default();
    for i in 0..20 {
        add_board(
            &mut model,
            DVec3::new(0.038, 0.089, 0.6),
            DVec3::X * i as f64,
            "Rail",
            true,
        );
    }
    for i in 0..2 {
        add_board(
            &mut model,
            DVec3::new(0.089, 0.089, 2.1),
            DVec3::Y * (i + 1) as f64,
            "Post",
            false,
        );
    }
    let before = model.clone();
    let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
    assert_eq!(plan.groups.len(), 2);
    assert_eq!(plan.groups[0].dimensions, [380, 890, 6000]);
    assert_eq!(plan.groups[0].parts.len(), 20);
    assert_eq!(plan.groups[1].dimensions, [890, 890, 21000]);
    assert_eq!(plan.groups[1].parts.len(), 2);
    assert!(plan.groups.iter().all(|group| group.rectangular));
    assert_eq!(plan.skipped, 0);
    let pdf = String::from_utf8(plan.pdf("Workbench", false, false)).unwrap();
    for text in [
        "22 pieces",
        "2x4 nominal",
        "4x4 nominal",
        "38 mm x 89 mm",
        "89 mm x 89 mm",
        "60 cm",
        "210 cm",
    ] {
        assert!(pdf.contains(text), "Missing {text}");
    }
    assert!(!pdf.contains("TOTAL LENGTH"));
    assert_eq!(model, before, "Planning must never edit the document");
}

#[test]
fn nested_assembly_and_part_selection_count_each_instance_once() {
    let mut model = Model::default();
    let a = add_board(
        &mut model,
        DVec3::new(0.038, 0.089, 0.6),
        DVec3::ZERO,
        "Rail",
        true,
    );
    let source = model.copy_entities(&BTreeSet::from([a]));
    let b = *model.paste_entities(&source, DVec3::X).first().unwrap();
    let assembly = model
        .make_object(&BTreeSet::from([a, b]), false, "Frame")
        .unwrap();
    // Grouping remaps child entity IDs; use the resulting instances.
    let children: Vec<_> = model.objects[&assembly]
        .entities
        .iter()
        .copied()
        .filter(|id| model.objects.contains_key(id))
        .collect();
    let (a, b) = (children[0], children[1]);
    add_board(
        &mut model,
        DVec3::new(0.1, 0.1, 1.),
        DVec3::Y,
        "Other project part",
        false,
    );
    let plan = CuttingPlan::collect(&model, &BTreeSet::from([assembly, a])).unwrap();
    assert_eq!(plan.groups.len(), 1);
    assert_eq!(plan.groups[0].parts.len(), 2);
    assert!(
        plan.groups[0]
            .parts
            .iter()
            .all(|name| name.contains("Frame") && name.contains("Rail"))
    );
    let plan = CuttingPlan::collect(&model, &BTreeSet::from([b])).unwrap();
    assert_eq!(plan.groups[0].parts.len(), 1);
    let face = *model
        .expand_entities(&BTreeSet::from([a]))
        .iter()
        .find(|id| model.faces.contains_key(id))
        .unwrap();
    let plan = CuttingPlan::collect(&model, &BTreeSet::from([face])).unwrap();
    assert_eq!(plan.groups[0].parts.len(), 1);
    assert_eq!(plan.groups[0].dimensions, [380, 890, 6000]);
    model.hidden.insert(b);
    let plan = CuttingPlan::collect(&model, &BTreeSet::from([assembly])).unwrap();
    assert_eq!(
        plan.groups[0].parts.len(),
        1,
        "Selected assemblies exclude hidden children"
    );
    assert!(CuttingPlan::collect(&model, &BTreeSet::from([b])).is_err());
}

#[test]
fn dimensions_follow_rotation_translation_scaling_and_axis_permutations() {
    let mut original = blank(DVec3::new(0.038, 0.089, 0.6));
    assign_raw(&mut original);
    for rotation in [
        glam::DQuat::IDENTITY,
        glam::DQuat::from_rotation_y(0.71) * glam::DQuat::from_rotation_z(0.31),
        glam::DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2),
    ] {
        let mut model = original.clone();
        for vertex in model.vertices.values_mut() {
            vertex.position = rotation * (vertex.position * 2.) + DVec3::new(1000., -1000., 2000.);
        }
        let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
        assert_eq!(plan.groups[0].dimensions, [760, 1780, 12000]);
        assert!(plan.groups[0].rectangular);
    }
}

#[test]
fn short_steps_keep_the_group_width_and_cut_direction_even_when_selected_alone() {
    let mut model = Model::default();
    let mut entities = BTreeSet::new();
    for (index, length) in [1.2, 1.1, 1., 0.9, 0.8, 0.2].into_iter().enumerate() {
        entities.extend(model.paste_entities(
            &blank(DVec3::new(0.038, 0.3, length)),
            DVec3::X * (index * 2) as f64,
        ));
    }
    let steps = model.make_object(&entities, false, "Steps").unwrap();
    model.objects.get_mut(&steps).unwrap().stock_material = Some(material());
    let short_face = *model.objects[&steps]
        .entities
        .iter()
        .find(|id| {
            model.faces.get(id).is_some_and(|face| {
                face.vertices
                    .iter()
                    .all(|vertex| model.vertices[vertex].position.x > 9.9)
            })
        })
        .unwrap();
    // A different hierarchy with the same leaf name has its own orientation.
    let other = add_board(
        &mut model,
        DVec3::new(0.038, 0.3, 0.2),
        DVec3::X * 20.,
        "Steps",
        false,
    );
    model
        .make_object(&BTreeSet::from([other]), false, "Other")
        .unwrap();
    // Hidden geometry must not determine the group's cutting direction.
    let hidden = add_board(
        &mut model,
        DVec3::new(0.038, 2., 0.2),
        DVec3::X * 30.,
        "Steps",
        false,
    );
    model.hidden.insert(hidden);

    for rotation in [
        glam::DQuat::IDENTITY,
        glam::DQuat::from_rotation_y(0.71) * glam::DQuat::from_rotation_z(0.31),
    ] {
        let mut rotated = model.clone();
        for vertex in rotated.vertices.values_mut() {
            vertex.position = rotation * vertex.position;
        }
        let before = rotated.clone();
        let plan = CuttingPlan::collect(&rotated, &BTreeSet::new()).unwrap();
        let steps: Vec<_> = plan
            .groups
            .iter()
            .filter(|group| group.parts == ["Steps"])
            .map(|group| group.dimensions)
            .collect();
        assert_eq!(
            steps,
            [2000, 8000, 9000, 10000, 11000, 12000].map(|length| [380, 3000, length])
        );
        let other = plan
            .groups
            .iter()
            .find(|group| group.parts == ["Other/Steps"])
            .unwrap();
        assert_eq!(other.dimensions, [380, 2000, 3000]);
        let selected = CuttingPlan::collect(&rotated, &BTreeSet::from([short_face])).unwrap();
        assert_eq!(selected.groups.len(), 1);
        assert_eq!(selected.groups[0].dimensions, [380, 3000, 2000]);
        assert_eq!(selected.groups[0].parts, ["Steps"]);
        assert_eq!(rotated, before, "Planning must never edit the document");
        for group_stock in [false, true] {
            let pdf = String::from_utf8(plan.pdf("Steps", false, group_stock)).unwrap();
            assert!(pdf.contains("(38 mm x 30 cm)"));
            assert!(pdf.contains("(20 cm)"));
            if group_stock {
                assert!(pdf.contains("Custom stock: 38 mm x 30 cm"));
                assert!(pdf.contains("- 30 cm x 20 cm"));
            }
        }
    }
}

#[test]
fn whole_project_excludes_hidden_parts_but_keeps_visible_locked_parts_outside_the_edit_context() {
    let mut model = Model::default();
    let a = add_board(
        &mut model,
        DVec3::new(0.038, 0.089, 0.6),
        DVec3::ZERO,
        "A",
        false,
    );
    let b = add_board(
        &mut model,
        DVec3::new(0.038, 0.089, 0.6),
        DVec3::X,
        "B",
        false,
    );
    let c = add_board(
        &mut model,
        DVec3::new(0.038, 0.089, 0.6),
        DVec3::Y,
        "Visible locked board",
        false,
    );
    model.hidden.insert(b);
    model.locked.insert(c);
    model.edit_context = Some(a);
    let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
    assert_eq!(plan.groups[0].parts.len(), 2);
    assert!(plan.groups[0].parts.iter().all(|part| part.as_str() != "B"));
    let pdf = String::from_utf8(plan.pdf("Visible boards", false, false)).unwrap();
    assert!(!pdf.contains("Whole project"));
    assert!(!pdf.contains("KETCHUP / WOODWORKING"));
    assert!(!pdf.contains("includes hidden"));
}

#[test]
fn disconnected_raw_geometry_is_split_and_edge_selection_expands_only_its_body() {
    let mut model = blank(DVec3::new(0.02, 0.1, 0.8));
    model.paste_entities(&blank(DVec3::new(0.02, 0.1, 0.8)), DVec3::X);
    assign_raw(&mut model);
    let edge = *model.edges.keys().next().unwrap();
    assert_eq!(
        CuttingPlan::collect(&model, &BTreeSet::new())
            .unwrap()
            .groups[0]
            .parts
            .len(),
        2
    );
    let plan = CuttingPlan::collect(&model, &BTreeSet::from([edge])).unwrap();
    assert_eq!(plan.groups[0].parts.len(), 1);
    assert_eq!(plan.groups[0].dimensions, [200, 1000, 8000]);
}

#[test]
fn empty_flat_and_line_only_selections_report_no_parts_instead_of_exporting_the_project() {
    assert!(CuttingPlan::collect(&Model::default(), &BTreeSet::new()).is_err());
    let mut model = Model::default();
    let line = model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    assert!(CuttingPlan::collect(&model, &BTreeSet::new()).is_err());
    add_board(
        &mut model,
        DVec3::new(0.02, 0.1, 0.8),
        DVec3::Y,
        "Board",
        false,
    );
    assert!(CuttingPlan::collect(&model, &BTreeSet::from([line])).is_err());
    let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
    assert_eq!(plan.groups[0].parts.len(), 1);
    assert_eq!(plan.skipped, 1);
    let mut flat = blank(DVec3::new(0.02, 0.1, 0.8));
    for vertex in flat.vertices.values_mut() {
        vertex.position.z = 0.;
    }
    assert!(CuttingPlan::collect(&flat, &BTreeSet::new()).is_err());
    // A slanted triangle has nonzero world bounds but no material thickness.
    let mut triangle = Model::default();
    let corners = [DVec3::ZERO, DVec3::new(1., 0., 1.), DVec3::new(0., 1., 1.)];
    for i in 0..3 {
        triangle.add_edge(corners[i], corners[(i + 1) % 3]).unwrap();
    }
    assert!(CuttingPlan::collect(&triangle, &BTreeSet::new()).is_err());
}

#[test]
fn open_or_irregular_geometry_is_separated_and_marked_as_a_bounding_blank() {
    let mut model = blank(DVec3::new(0.02, 0.1, 0.8));
    let mut open = model.clone();
    open.faces.pop_first();
    model.paste_entities(&open, DVec3::X);
    assign_raw(&mut model);
    let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
    assert_eq!(plan.groups.len(), 2);
    assert_eq!(plan.groups[0].dimensions, plan.groups[1].dimensions);
    assert_ne!(plan.groups[0].rectangular, plan.groups[1].rectangular);
    let pdf = String::from_utf8(plan.pdf("Open shape", true, false)).unwrap();
    assert!(pdf.contains("CHECK SHAPE"));
    assert!(pdf.contains("bounding blank only"));
}

#[test]
fn notches_and_holes_purchase_the_same_full_blank_as_an_uncut_panel() {
    for cutout in ["none", "corner", "hole"] {
        let mut model = Model::default();
        let outline = if cutout == "corner" {
            vec![
                (0., 0.), (2., 0.), (2., 1.9), (1.9, 1.9), (1.9, 2.), (0., 2.),
            ]
        } else {
            vec![(0., 0.), (2., 0.), (2., 2.), (0., 2.)]
        };
        let mut ring = |points: &[(f64, f64)]| {
            for i in 0..points.len() {
                let (x, y) = points[i];
                let (next_x, next_y) = points[(i + 1) % points.len()];
                model
                    .add_edge(DVec3::new(x, y, 0.), DVec3::new(next_x, next_y, 0.))
                    .unwrap();
            }
        };
        ring(&outline);
        if cutout == "hole" {
            ring(&[(0.9, 0.9), (1., 0.9), (1., 1.), (0.9, 1.)]);
            let inner = model
                .faces
                .values()
                .find(|face| model.face_area(face) < 0.02)
                .unwrap()
                .id;
            model.erase(&BTreeSet::from([inner]));
        }
        let face = *model.faces.keys().next().unwrap();
        model.push_pull(face, 0.018).unwrap();
        assign_raw(&mut model);
        let object = model.objects.values_mut().next().unwrap();
        object.stock_material.as_mut().unwrap().dimensions = [180, 20000, 20000];
        let before = model.clone();
        // Whole-project and object selections must each count one full blank.
        for selected in [BTreeSet::new(), model.objects.keys().copied().collect()] {
            let plan = CuttingPlan::collect(&model, &selected).unwrap();
            assert_eq!(plan.groups.len(), 1, "{cutout}");
            assert_eq!(plan.groups[0].dimensions, [180, 20000, 20000], "{cutout}");
            assert_eq!(plan.groups[0].rectangular, cutout == "none");
            let required = plan.material_requirements(LayoutSettings::default());
            assert_eq!(required[0].quantity, 1, "{cutout}");
            assert_eq!(required[0].excluded, 0, "{cutout}");
            let pdf = String::from_utf8(plan.pdf("Panel", false, true)).unwrap();
            assert!(pdf.contains("Stock required: 1 stock piece"), "{cutout}");
            assert!(!pdf.contains("DOES NOT FIT SELECTED STOCK"), "{cutout}");
        }
        assert_eq!(model, before);
    }
}

#[test]
fn fractional_dimensions_are_retained_and_nominal_labels_never_replace_actual_sizes() {
    for (tenths, expected) in [
        (0, "0 mm"),
        (381, "38.1 mm"),
        (999, "99.9 mm"),
        (1000, "100 mm"),
        (1001, "10.01 cm"),
        (1010, "10.1 cm"),
        (1100, "11 cm"),
        (6000, "60 cm"),
        (6004, "60.04 cm"),
        (10000, "100 cm"),
    ] {
        assert_eq!(metric_length(tenths), expected);
    }
    assert_eq!(nominal_stock(381, 889), Some("2x4"));
    assert_eq!(nominal_stock(889, 889), Some("4x4"));
    assert_eq!(
        nominal_stock(508, 1016),
        None,
        "Full two by four inches is not dressed 2x4"
    );
    assert_eq!(nominal_stock(400, 900), None);
    let mut model = blank(DVec3::new(0.0381, 0.0889, 0.6004));
    assign_raw(&mut model);
    let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
    assert_eq!(plan.groups[0].dimensions, [381, 889, 6004]);
    for group_stock in [false, true] {
        let pdf = String::from_utf8(plan.pdf("Fractional cuts", false, group_stock)).unwrap();
        assert!(pdf.contains("Dimensions in mm or cm"));
        assert!(pdf.contains("(38.1 mm x 88.9 mm)"));
        assert!(pdf.contains("(60.04 cm)"));
        if group_stock {
            assert!(pdf.contains("Stock: 244 cm x 122 cm"));
            assert!(pdf.contains("- 88.9 mm x 60.04 cm"));
        }
    }
}

#[test]
fn pdf_escapes_names_paginates_the_summary_and_has_valid_cross_reference_offsets() {
    assert_eq!(
        pdf_string("Café (A) \\ new\nline"),
        "Caf\\351 \\050A\\051 \\134 new line"
    );
    let plan = CuttingPlan {
        groups: (0..70)
            .map(|i| CutGroup {
                material_id: None,
                material: material(),
                dimensions: [380, 890, 6000 + i * 10],
                rectangular: i % 2 == 0,
                parts: vec![format!(
                    "{} (Part {}) \\ Café",
                    "LongAssemblyName".repeat(30),
                    i
                )],
            })
            .collect(),
        skipped: 3,
    };
    let bytes = plan.pdf("Bench (rev 2) \\ Shop", true, false);
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.starts_with("%PDF-1.4"));
    assert!(text.ends_with("%%EOF\n"));
    assert!(text.contains("Bench \\050rev 2\\051 \\134 Shop"));
    assert!(text.contains("Selected parts"));
    let pages = text.matches("/Type /Page /").count();
    assert!(pages > 1);
    assert!(!text.contains("PART REFERENCES"));
    assert!(text.contains("LongAssemblyName"));
    assert!(!text.contains("KETCHUP / WOODWORKING"));
    assert_eq!(
        text.matches("Finished sizes - verify before cutting")
            .count(),
        pages
    );
    let xref: usize = text
        .split("startxref\n")
        .nth(1)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(&bytes[xref..xref + 4], b"xref");
    let xref_table = &text[xref..];
    let size: usize = xref_table
        .lines()
        .nth(1)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    for (i, line) in xref_table.lines().skip(3).take(size - 1).enumerate() {
        let offset: usize = line[..10].parse().unwrap();
        assert!(text[offset..].starts_with(&format!("{} 0 obj\n", i + 1)));
    }
    // Content stream lengths must count exact bytes, including trailing newlines.
    for stream in text.split("<< /Length ").skip(1) {
        let length: usize = stream.split_whitespace().next().unwrap().parse().unwrap();
        let data = stream.split_once("stream\n").unwrap().1;
        assert!(data[length..].starts_with("endstream"));
    }
}

#[test]
fn visibility_respects_hidden_ancestors_and_tags_for_both_export_scopes() {
    for mode in [
        "object",
        "ancestor",
        "object_tag",
        "ancestor_tag",
        "geometry",
        "geometry_tag",
    ] {
        let mut model = Model::default();
        let board = add_board(
            &mut model,
            DVec3::new(0.038, 0.089, 0.6),
            DVec3::ZERO,
            "Excluded board",
            false,
        );
        let assembly = model
            .make_object(&BTreeSet::from([board]), false, "Assembly")
            .unwrap();
        let board = *model.objects[&assembly]
            .entities
            .iter()
            .find(|id| model.objects.contains_key(id))
            .unwrap();
        add_board(
            &mut model,
            DVec3::new(0.038, 0.089, 0.6),
            DVec3::Y,
            "Visible board",
            false,
        );
        let targets = match mode {
            "ancestor" | "ancestor_tag" => BTreeSet::from([assembly]),
            "geometry" | "geometry_tag" => model
                .expand_entities(&BTreeSet::from([board]))
                .into_iter()
                .filter(|id| !model.objects.contains_key(id))
                .collect(),
            _ => BTreeSet::from([board]),
        };
        if mode.ends_with("_tag") {
            let tag = model.add_tag("Hidden stock").unwrap();
            model.assign_tag(&targets, Some(tag)).unwrap();
            model.tags.get_mut(&tag).unwrap().visible = false;
        } else {
            model.hidden.extend(targets);
        }
        let before = model.clone();
        let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
        assert_eq!(plan.groups[0].parts.len(), 1, "{mode}");
        assert!(plan.groups[0].parts[0].contains("Visible board"), "{mode}");
        assert_eq!(
            plan.skipped, 0,
            "Hidden items should not count as unusable parts: {mode}"
        );
        assert!(
            CuttingPlan::collect(&model, &BTreeSet::from([assembly])).is_err(),
            "{mode}"
        );
        assert_eq!(model, before);
    }
}

#[test]
fn only_assigned_objects_are_included_and_materials_keep_equal_cuts_separate() {
    let mut model = Model::default();
    let a = add_board(
        &mut model,
        DVec3::new(0.038, 0.089, 0.6),
        DVec3::ZERO,
        "Assigned",
        false,
    );
    let b = add_board(
        &mut model,
        DVec3::new(0.038, 0.089, 0.6),
        DVec3::X,
        "Unassigned",
        false,
    );
    model.objects.get_mut(&b).unwrap().stock_material = None;
    assert!(
        CuttingPlan::collect(&model, &BTreeSet::from([b]))
            .unwrap_err()
            .contains("Assign a material")
    );
    let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
    assert_eq!(plan.groups.len(), 1);
    assert_eq!(plan.skipped, 1);
    let mut other = material();
    other.name = "Catalog pine".into();
    model.objects.get_mut(&b).unwrap().stock_material = Some(other);
    let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
    assert_eq!(plan.groups.len(), 2);
    let pdf = String::from_utf8(plan.pdf("Materials", false, true)).unwrap();
    assert!(pdf.contains("Material: Catalog oak"));
    assert!(pdf.contains("Material: Catalog pine"));
    assert!(!pdf.contains("Suggested stock"));
    // Assignment survives document round-trips and copies, and defaults for old data.
    let saved = serde_json::to_string(&model).unwrap();
    let restored: Model = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored, model);
    let copy = model.copy_entities(&BTreeSet::from([a]));
    assert!(
        copy.objects
            .values()
            .any(|o| o.stock_material == Some(material()))
    );
    let mut legacy = serde_json::to_value(&model.objects[&a]).unwrap();
    legacy.as_object_mut().unwrap().remove("stock_material");
    let legacy: ketchup_core::Object = serde_json::from_value(legacy).unwrap();
    assert!(legacy.stock_material.is_none());
}

#[test]
fn assembly_material_is_inherited_and_child_assignment_overrides_it() {
    let mut model = blank(DVec3::new(0.038, 0.089, 0.6));
    assign_raw(&mut model);
    let roots = model.root_entities();
    let assembly = model.make_object(&roots, false, "Assembly").unwrap();
    let mut parent_material = material();
    parent_material.name = "Parent".into();
    model.objects.get_mut(&assembly).unwrap().stock_material = Some(parent_material.clone());
    let child = *model.objects[&assembly].entities.first().unwrap();
    assert_eq!(
        CuttingPlan::collect(&model, &BTreeSet::new())
            .unwrap()
            .groups[0]
            .material,
        material()
    );
    model.objects.get_mut(&child).unwrap().stock_material = None;
    assert_eq!(
        CuttingPlan::collect(&model, &BTreeSet::new())
            .unwrap()
            .groups[0]
            .material,
        parent_material
    );
}

#[test]
fn cut_list_sorts_by_group_then_descending_quantity_and_cut_length() {
    let plan = CuttingPlan {
        groups: [
            ("Zeta", 4, 8000),
            ("Beta", 2, 6000),
            ("Alpha", 1, 7000),
            ("Beta", 2, 9000),
            ("Beta", 3, 5000),
        ]
        .into_iter()
        .map(|(name, quantity, length)| CutGroup {
            material_id: None,
            material: material(),
            dimensions: [380, 890, length],
            rectangular: true,
            parts: vec![name.into(); quantity],
        })
        .collect(),
        skipped: 0,
    };
    for group_stock in [false, true] {
        let pdf = String::from_utf8(plan.pdf_with_layout(
            "Sorted cuts",
            false,
            group_stock,
            LayoutSettings {
                board_length_mm: 1000.,
                ..LayoutSettings::default()
            },
        ))
        .unwrap();
        let mut remaining = pdf.as_str();
        for (index, length) in [70, 50, 90, 60, 80].into_iter().enumerate() {
            remaining = remaining
                .split_once(&format!("(P{:03})", index + 1))
                .expect("Part labels follow the sorted rows")
                .1;
            let (row, tail) = remaining.split_once(&format!("({length} cm)")).unwrap();
            assert!(!row.contains("(P00"), "Cut lengths follow the sorted rows");
            remaining = tail;
        }
        if group_stock {
            for (index, length) in [70, 50, 90, 60, 80].into_iter().enumerate() {
                assert!(pdf.contains(&format!("P{:03} x1 - 89 mm x {length} cm", index + 1)));
            }
        }
    }
}

#[test]
fn stock_grouping_preserves_visible_only_collection() {
    let mut model = Model::default();
    let size = DVec3::new(0.018, 0.3, 0.6);
    add_board(&mut model, size, DVec3::ZERO, "Visible panel", false);
    let hidden = add_board(&mut model, size, DVec3::X, "Hidden panel", false);
    model.hidden.insert(hidden);
    let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
    let pdf = String::from_utf8(plan.pdf("Cabinet", false, true)).unwrap();
    assert!(pdf.contains("Material: Catalog oak"));
    assert!(pdf.contains("1 piece | Net cut area: 0.18 m2"));
}

#[test]
fn stock_pdf_includes_colored_coded_diagrams_and_uses_the_selected_kerf() {
    let plan = CuttingPlan {
        groups: vec![CutGroup {
            material_id: None,
            material: ketchup_core::StockMaterial {
                name: "Oak board".into(),
                dimensions: [380, 890, 10000],
            },
            dimensions: [380, 890, 5000],
            rectangular: true,
            parts: vec!["A".into(), "B".into()],
        }],
        skipped: 0,
    };
    let settings = LayoutSettings {
        board_length_mm: 1000.,
        kerf_mm: 3.,
        ..LayoutSettings::default()
    };
    let with_kerf =
        String::from_utf8(plan.pdf_with_layout("Bench", false, true, settings)).unwrap();
    assert!(with_kerf.contains("Stock required: 2 stock pieces"));
    let required = plan.material_requirements(settings);
    assert_eq!(required.len(), 1);
    assert_eq!(required[0].quantity, 2);
    assert_eq!(required[0].excluded, 0);
    assert_eq!(plan.material_requirements(LayoutSettings { kerf_mm: 0., ..settings })[0].quantity, 1);
    assert!(with_kerf.contains("STOCK 1 of 2"));
    assert!(with_kerf.contains("STOCK 2 of 2"));
    assert!(with_kerf.contains("Kerf: 3 mm"));
    assert!(with_kerf.contains("P001 x1 - 89 mm x 50 cm"));
    assert!(
        with_kerf.contains(" re B Q"),
        "Diagrams and color swatches use vector rectangles"
    );
    let no_kerf = String::from_utf8(plan.pdf_with_layout(
        "Bench",
        false,
        true,
        LayoutSettings {
            kerf_mm: 0.,
            ..settings
        },
    ))
    .unwrap();
    assert!(no_kerf.contains("Stock required: 1 stock piece"));
    assert!(!no_kerf.contains("STOCK 2"));
    let plain = String::from_utf8(plan.pdf_with_layout("Bench", false, false, settings)).unwrap();
    assert!(!plain.contains("STOCK 1"));
    assert!(!plain.contains(" re B Q"));
    for i in 0..100 {
        for j in 0..i {
            assert_ne!(mark_color(i), mark_color(j));
        }
    }
}

#[test]
fn oversized_cuts_are_reported_without_silently_increasing_the_stock_size() {
    let plan = CuttingPlan {
        groups: vec![CutGroup {
            material_id: None,
            material: material(),
            dimensions: [180, 13000, 30000],
            rectangular: true,
            parts: vec!["Too large".into()],
        }],
        skipped: 0,
    };
    let pdf = String::from_utf8(plan.pdf("Cabinet", false, true)).unwrap();
    assert!(pdf.contains("DOES NOT FIT SELECTED STOCK"));
    assert!(pdf.contains("P001: 1 piece, 130 cm x 300 cm"));
    assert!(!pdf.contains("STOCK 1"));
}

#[test]
fn group_column_shows_named_hierarchy_and_omits_generated_group_names() {
    let mut model = Model::default();
    let board = add_board(
        &mut model,
        DVec3::new(0.038, 0.089, 0.6),
        DVec3::ZERO,
        "Leg",
        false,
    );
    let generated = model
        .make_object(&BTreeSet::from([board]), false, "Group#42")
        .unwrap();
    let frame = model
        .make_object(&BTreeSet::from([generated]), false, "Frame")
        .unwrap();
    model
        .make_object(&BTreeSet::from([frame]), false, "Bench")
        .unwrap();
    let other = add_board(
        &mut model,
        DVec3::new(0.038, 0.089, 0.6),
        DVec3::X,
        "Side",
        false,
    );
    model
        .make_object(&BTreeSet::from([other]), false, "Cabinet")
        .unwrap();
    let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
    assert_eq!(
        plan.groups.len(),
        1,
        "Keep matching cuts grouped across hierarchies"
    );
    assert_eq!(plan.groups[0].parts.len(), 2);
    assert_eq!(
        group_lines(&plan.groups[0]),
        ["Bench/Frame/Leg", "Cabinet/Side"]
    );
    for grouped in [false, true] {
        let pdf = String::from_utf8(plan.pdf("Workshop", false, grouped)).unwrap();
        assert!(pdf.contains("(GROUP)"));
        assert!(pdf.contains("Bench/Frame/Leg"));
        assert!(pdf.contains("Cabinet/Side"));
        assert!(!pdf.contains("Group#42"));
        assert!(!pdf.contains("part #"));
        assert!(!pdf.contains("[#"));
    }
}

#[test]
fn group_column_deduplicates_paths_wraps_long_names_and_handles_unnamed_parts() {
    for name in ["Group#1", "Group#12345", "Group #42"] {
        assert!(automatic_group_name(name));
    }
    for name in ["Group", "Group#", "Group#Leg", "MyGroup#42"] {
        assert!(!automatic_group_name(name));
    }
    let group = CutGroup {
        material_id: None,
        material: material(),
        dimensions: [180, 3000, 6000],
        rectangular: true,
        parts: vec!["Bench/Frame/Leg".into(), "Bench/Frame/Leg".into()],
    };
    assert_eq!(group_lines(&group), ["Bench/Frame/Leg"]);
    let group = CutGroup {
        material_id: None,
        parts: vec!["".into()],
        ..group
    };
    assert_eq!(group_lines(&group), ["-"]);
    let long = "LongParent/".repeat(200);
    let group = CutGroup {
        material_id: None,
        parts: vec![long.clone()],
        ..group
    };
    let lines = group_lines(&group);
    assert!(lines.iter().all(|line| line.chars().count() <= 32));
    assert_eq!(lines.join(""), long);
    let plan = CuttingPlan {
        groups: vec![group],
        skipped: 0,
    };
    let pdf = String::from_utf8(plan.pdf("Long hierarchy", false, false)).unwrap();
    assert!(pdf.matches("/Type /Page /").count() > 1);
    assert!(pdf.contains("continued"));
}

#[test]
fn assigned_stock_rejects_cuts_that_exceed_thickness_or_width() {
    for (dimensions, rectangular) in [
        ([400, 890, 5000], true),
        ([380, 900, 5000], true),
        ([400, 890, 5000], false),
        ([380, 900, 5000], false),
    ] {
        let plan = CuttingPlan {
            groups: vec![CutGroup {
                material_id: None,
                material: ketchup_core::StockMaterial {
                    name: "Narrow stock".into(),
                    dimensions: [380, 890, 10000],
                },
                dimensions,
                rectangular,
                parts: vec!["Too large".into()],
            }],
            skipped: 0,
        };
        let pdf = String::from_utf8(plan.pdf_with_layout(
            "Fit",
            false,
            true,
            LayoutSettings {
                allow_rotation: false,
                ..Default::default()
            },
        ))
        .unwrap();
        let required = plan.material_requirements(LayoutSettings {
            allow_rotation: false,
            ..Default::default()
        });
        assert_eq!(required[0].quantity, 0);
        assert_eq!(required[0].excluded, 1);
        assert!(pdf.contains("DOES NOT FIT SELECTED STOCK"));
        assert!(!pdf.contains("STOCK 1"));
    }
}

#[test]
fn material_ids_keep_identical_catalog_entries_in_separate_cut_groups() {
    let mut model = Model::default();
    let a = add_board(
        &mut model,
        DVec3::new(0.019, 0.089, 0.6),
        DVec3::ZERO,
        "A",
        false,
    );
    let b = add_board(
        &mut model,
        DVec3::new(0.019, 0.089, 0.6),
        DVec3::X,
        "B",
        false,
    );
    let first = ketchup_core::MaterialId::new_v4();
    let second = ketchup_core::MaterialId::new_v4();
    for (object, id) in [(a, first), (b, second)] {
        model.objects.get_mut(&object).unwrap().stock_material = None;
        model.objects.get_mut(&object).unwrap().stock_material_id = Some(id);
        model.stock_materials.insert(id, material());
    }
    let plan = CuttingPlan::collect(&model, &BTreeSet::new()).unwrap();
    assert_eq!(plan.groups.len(), 2);
    assert_ne!(plan.groups[0].material_id, plan.groups[1].material_id);
    let required = plan.material_requirements(LayoutSettings::default());
    assert_eq!(required.len(), 2);
    assert_ne!(required[0].material_id, required[1].material_id);
    assert!(required.iter().all(|item| item.quantity == 1));
}
