//! Independent feature/option checks, in addition to the end-to-end workflows.
use super::*;

pub(super) struct RenderCase {
    pub name: String,
    key: &'static str,
    value: serde_json::Value,
}

pub(super) fn render_cases() -> Vec<RenderCase> {
    let mut cases = Vec::new();
    let mut pair = |name: &str, key, a, b| {
        for (index, value) in [a, b].into_iter().enumerate() {
            cases.push(RenderCase {
                name: format!("audit-{name}-{index}"),
                key,
                value,
            });
        }
    };
    let defaults = serde_json::to_value(ketchup_core::DisplaySettings::default()).unwrap();
    for (key, value) in defaults.as_object().unwrap() {
        if value.is_boolean() {
            // Keys come from a static schema below to retain useful report names.
            let key = match key.as_str() {
                "axes" => "axes",
                "grid" => "grid",
                "guides" => "guides",
                "dimensions" => "dimensions",
                "hidden_geometry" => "hidden_geometry",
                "hidden_objects" => "hidden_objects",
                "group_emphasis" => "group_emphasis",
                "x_ray" => "x_ray",
                "edges" => "edges",
                "back_edges" => "back_edges",
                "profiles" => "profiles",
                "depth_cue" => "depth_cue",
                "endpoints" => "endpoints",
                "extensions" => "extensions",
                "color_by_tag" => "color_by_tag",
                "section_planes" => "section_planes",
                "section_cuts" => "section_cuts",
                "section_fill" => "section_fill",
                "shadows" => "shadows",
                "fog" => "fog",
                "sky" => "sky",
                "ground" => "ground",
                key => panic!("Uncovered display option: {key}"),
            };
            pair(key, key, false.into(), true.into());
        }
    }
    for style in ketchup_core::FaceStyle::ALL {
        let baseline = if style == ketchup_core::FaceStyle::Materials {
            ketchup_core::FaceStyle::Shaded
        } else {
            ketchup_core::FaceStyle::Materials
        };
        pair(
            &format!("style-{style:?}"),
            "face_style",
            serde_json::to_value(baseline).unwrap(),
            serde_json::to_value(style).unwrap(),
        );
    }
    for (key, a, b) in [
        ("profile_width", 1., 8.),
        ("section_height", 0.5, 1.5),
        ("shadow_darkness", 0., 1.),
        ("sun_angle", 0., 180.),
        ("sun_elevation", 5., 85.),
        ("fog_start", 0., 10.),
        ("fog_end", 10., 50.),
    ] {
        pair(key, key, a.into(), b.into());
    }
    for key in ["background", "sky_color", "ground_color", "horizon_color"] {
        pair(
            key,
            key,
            defaults[key].clone(),
            serde_json::json!([240, 20, 150]),
        );
    }
    pair(
        "units",
        "units",
        defaults["units"].clone(),
        serde_json::json!({"length": "feet"}),
    );
    cases
}

pub(super) fn render_workspace(name: &str) -> KetchupApp {
    let case = render_cases().into_iter().find(|c| c.name == name).unwrap();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    app.inspector = false;
    let first = ui_solid_box(
        &mut app.doc.model,
        DVec3::new(-2., -1., 0.),
        DVec3::new(0., 1., 2.),
    );
    let second = ui_solid_box(
        &mut app.doc.model,
        DVec3::new(1., -1., 0.),
        DVec3::new(3., 1., 3.),
    );
    let hidden = ui_solid_box(
        &mut app.doc.model,
        DVec3::new(4., 0., 0.),
        DVec3::new(5., 1., 2.),
    );
    app.doc.model.hidden.insert(hidden);
    let edge = app
        .doc
        .model
        .add_edge(DVec3::new(-3., 0., 2.), DVec3::new(-3., 2., 2.))
        .unwrap();
    app.doc.model.hidden.insert(edge);
    app.doc
        .model
        .add_dimension(DVec3::new(-2., -2., 0.), DVec3::new(3., -2., 0.), -DVec3::Y)
        .unwrap();
    for (id, color, name) in [
        (first, [195, 96, 67], "Structure"),
        (second, [95, 143, 177], "Furniture"),
    ] {
        let tag = app.doc.model.add_tag(name).unwrap();
        app.doc
            .model
            .assign_tag(&BTreeSet::from([id]), Some(tag))
            .unwrap();
        for face in app.doc.model.expand_entities(&BTreeSet::from([id])) {
            if app.doc.model.faces.contains_key(&face) {
                app.doc.model.face_colors.insert(face, color);
            }
        }
    }
    app.doc.camera.target = DVec3::new(1., 0., 1.);
    app.doc.camera.distance = 22.;
    match case.key {
        "group_emphasis" => assert!(app.doc.model.enter_object(first)),
        "guides" => {
            app.set_tool(Tool::Line);
            app.line_start = Some(DVec3::new(-2., -2., 0.));
            app.snap = Some(Snap {
                point: DVec3::new(-2., -2., 3.),
                kind: Inference::Axis(Axis::Z),
            });
        }
        "section_fill" | "section_height" => app.doc.display.section_cuts = true,
        "profile_width" => app.doc.display.profiles = true,
        "shadow_darkness" | "sun_angle" | "sun_elevation" => {
            app.doc.display.shadows = true;
            // Shadows only fall on model faces. A receiving floor makes sun
            // elevation visibly change the shadow instead of merely leaving
            // the same box walls fully shaded at both tested elevations.
            let floor = [
                DVec3::new(-5., -4., -0.1),
                DVec3::new(6., -4., -0.1),
                DVec3::new(6., 5., -0.1),
                DVec3::new(-5., 5., -0.1),
            ];
            for i in 0..4 {
                app.doc
                    .model
                    .add_edge(floor[i], floor[(i + 1) % 4])
                    .unwrap();
            }
        }
        "fog_start" | "fog_end" => {
            app.doc.display.fog = true;
            app.doc.display.fog_start = 0.;
        }
        "background" => {
            app.doc.display.sky = false;
            app.doc.display.ground = false;
        }
        "sky" | "ground" | "sky_color" | "ground_color" | "horizon_color" => {
            app.doc.camera.pitch = 0.15
        }
        _ => {}
    }
    let mut display = serde_json::to_value(&app.doc.display).unwrap();
    display[case.key] = case.value;
    app.doc.display = serde_json::from_value(display).unwrap();
    app.doc.model.validate().unwrap();
    app
}

#[test]
fn every_display_setting_has_an_independent_render_case() {
    let defaults = serde_json::to_value(ketchup_core::DisplaySettings::default()).unwrap();
    let fields: BTreeSet<_> = defaults
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let cases = render_cases();
    let covered: BTreeSet<_> = cases.iter().map(|c| c.key).collect();
    assert_eq!(covered, fields);
    for case in cases {
        render_workspace(&case.name);
    }
}

fn sidebar_position(h: &Harness, output: &egui::FullOutput, label: &str) -> Pos2 {
    fn find(shape: &egui::Shape, label: &str, left: f32) -> Option<Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label && text.pos.x > left => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            egui::Shape::Vec(shapes) => shapes.iter().rev().find_map(|s| find(s, label, left)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .rev()
        .find_map(|s| find(&s.shape, label, h.size.x - 310.))
        .unwrap_or_else(|| panic!("Missing inspector control: {label}"))
}

fn options_workspace() -> Harness {
    let mut h = Harness::new();
    h.ctx.style_mut_of(h.ctx.theme(), |s| s.animation_time = 0.);
    h.size.y = 5000.;
    for _ in 0..2 {
        h.frame(vec![], egui::Modifiers::NONE);
    }
    for title in [
        "Entity Info",
        "Edge style",
        "Sections",
        "Shadows",
        "Fog",
        "Environment",
    ] {
        let output = h.frame(vec![], egui::Modifiers::NONE);
        h.click(sidebar_position(&h, &output, title));
        h.time += 1.;
        h.frame(vec![], egui::Modifiers::NONE);
    }
    h
}

fn check_option(key: &str, label: &str) {
    let mut h = options_workspace();
    let model = h.app.doc.model.clone();
    let before = serde_json::to_value(&h.app.doc.display).unwrap()[key]
        .as_bool()
        .unwrap();
    for expected in [!before, before] {
        let output = h.frame(vec![], egui::Modifiers::NONE);
        let position = sidebar_position(&h, &output, label);
        h.click(position);
        assert_eq!(
            serde_json::to_value(&h.app.doc.display).unwrap()[key],
            expected,
            "{label}"
        );
        assert!(
            h.app.project.modified,
            "{label} must mark the project modified"
        );
        assert_eq!(h.app.doc.model, model, "{label} changed model geometry");
        let saved = Document::from_json(&serde_json::to_string(&h.app.doc).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(saved.display).unwrap()[key], expected);
    }
}

macro_rules! option {
    ($name:ident, $key:literal, $label:literal) => {
        #[test]
        fn $name() {
            check_option($key, $label);
        }
    };
}
option!(option_axes, "axes", "Axes");
option!(option_guides, "guides", "Guides");
option!(option_grid, "grid", "Grid");
option!(option_dimensions, "dimensions", "Dimensions");
option!(option_hidden_geometry, "hidden_geometry", "Hidden geometry");
option!(option_hidden_objects, "hidden_objects", "Hidden objects");
option!(option_x_ray, "x_ray", "X-Ray");
option!(option_color_by_tag, "color_by_tag", "Color by tag");
option!(option_edges, "edges", "Edges");
option!(option_back_edges, "back_edges", "Back edges");
option!(option_profiles, "profiles", "Profiles");
option!(option_depth_cue, "depth_cue", "Depth cue");
option!(option_endpoints, "endpoints", "Endpoints");
option!(option_extensions, "extensions", "Extensions");
option!(option_section_planes, "section_planes", "Section planes");
option!(option_section_cuts, "section_cuts", "Section cuts");
option!(option_section_fill, "section_fill", "Section fill");
option!(option_shadows, "shadows", "Display shadows");
option!(option_fog, "fog", "Display fog");
option!(option_sky, "sky", "Sky");
option!(option_ground, "ground", "Ground");

fn check_face_style(style: ketchup_core::FaceStyle) {
    let mut h = options_workspace();
    if style == h.app.doc.display.face_style {
        h.app.doc.display.face_style = ketchup_core::FaceStyle::Wireframe;
    }
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, style.label()));
    assert_eq!(h.app.doc.display.face_style, style);
    assert!(h.app.project.modified);
    let saved = Document::from_json(&serde_json::to_string(&h.app.doc).unwrap()).unwrap();
    assert_eq!(saved.display.face_style, style);
}
macro_rules! face_style {
    ($name:ident, $style:ident) => {
        #[test]
        fn $name() {
            check_face_style(ketchup_core::FaceStyle::$style);
        }
    };
}
face_style!(style_wireframe, Wireframe);
face_style!(style_hidden_line, HiddenLine);
face_style!(style_monochrome, Monochrome);
face_style!(style_shaded, Shaded);
face_style!(style_materials, Materials);

#[test]
fn reset_display_restores_every_setting_and_preserves_geometry() {
    let mut h = options_workspace();
    let edge = h.app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    let model = h.app.doc.model.clone();
    let mut json = serde_json::to_value(&h.app.doc.display).unwrap();
    for value in json.as_object_mut().unwrap().values_mut() {
        if let Some(on) = value.as_bool() {
            *value = (!on).into();
        }
    }
    json["profile_width"] = 8.into();
    json["section_height"] = (-12).into();
    json["sun_angle"] = 0.into();
    json["sun_elevation"] = 85.into();
    json["shadow_darkness"] = 1.into();
    json["fog_start"] = 20.into();
    json["fog_end"] = 200.into();
    json["face_style"] = "Wireframe".into();
    for key in ["background", "sky_color", "ground_color", "horizon_color"] {
        json[key] = serde_json::json!([1, 2, 3]);
    }
    h.app.doc.display = serde_json::from_value(json).unwrap();
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, "Reset display"));
    assert_eq!(h.app.doc.display, ketchup_core::DisplaySettings::default());
    assert_eq!(h.app.doc.model, model);
    assert!(h.app.doc.model.edges.contains_key(&edge));
    assert!(h.app.project.modified);
}

fn numeric_option(key: &str, values: &[f64], invalid: &[f64]) {
    let mut h = Harness::new();
    ui_solid_pair(&mut h);
    let model = h.app.doc.model.clone();
    for value in values {
        let mut json = serde_json::to_value(ketchup_core::DisplaySettings::default()).unwrap();
        json[key] = (*value).into();
        h.app.doc.display = serde_json::from_value(json).unwrap();
        // Enable dependencies so each value exercises rendering as well as persistence.
        h.app.doc.display.profiles = true;
        h.app.doc.display.shadows = true;
        h.app.doc.display.section_cuts = true;
        h.app.doc.display.section_planes = true;
        h.app.doc.display.fog = true;
        if key == "fog_start" {
            h.app.doc.display.fog_end = value + 1.;
        }
        if key == "fog_end" {
            h.app.doc.display.fog_start = 0.;
        }
        h.frame(vec![], egui::Modifiers::NONE);
        let saved = Document::from_json(&serde_json::to_string(&h.app.doc).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(saved.display).unwrap()[key].as_f64(),
            Some(*value)
        );
        assert_eq!(h.app.doc.model, model);
    }
    for value in invalid {
        let mut json = serde_json::to_value(Document::default()).unwrap();
        json["display"][key] = (*value).into();
        assert!(
            Document::from_json(&json.to_string()).is_err(),
            "accepted {key}={value}"
        );
    }
}
macro_rules! numeric {
    ($name:ident, $key:literal, $valid:expr, $invalid:expr) => {
        #[test]
        fn $name() {
            numeric_option($key, &$valid, &$invalid);
        }
    };
}
numeric!(
    range_profile_width,
    "profile_width",
    [1., 4., 8.],
    [0., 8.1]
);
numeric!(
    range_section_height,
    "section_height",
    [-1e8, 0., 1.5, 1e8],
    [-1e11, 1e11]
);
numeric!(
    range_shadow_darkness,
    "shadow_darkness",
    [0., 0.5, 1.],
    [-0.1, 1.1]
);
numeric!(
    range_sun_direction,
    "sun_angle",
    [0., 180., 360.],
    [-1., 361.]
);
numeric!(
    range_sun_elevation,
    "sun_elevation",
    [5., 45., 85.],
    [4.9, 85.1]
);
numeric!(range_fog_start, "fog_start", [0., 10., 1e8], [-1.]);
numeric!(range_fog_end, "fog_end", [0.1, 50., 1e9], [0., -1.]);

#[test]
fn polygon_minimum_and_maximum_sides_commit_valid_faces_and_undo() {
    for sides in [3, 64] {
        let mut h = Harness::new();
        h.app.polygon_sides = sides;
        h.app.set_tool(Tool::Polygon);
        h.click(h.at(DVec3::ZERO));
        h.click(h.at(DVec3::X * 2.));
        assert_eq!(h.app.doc.model.edges.len(), sides);
        assert_eq!(h.app.doc.model.faces.len(), 1);
        h.app.doc.model.validate().unwrap();
        h.app.undo(&h.ctx, false);
        assert!(h.app.doc.model.edges.is_empty());
        h.app.undo(&h.ctx, true);
        assert_eq!(h.app.doc.model.edges.len(), sides);
    }
}

#[test]
fn invalid_precision_inputs_preserve_model_history_and_active_preview() {
    for tool in [
        Tool::Line,
        Tool::Move,
        Tool::Scale,
        Tool::PushPull,
        Tool::Offset,
    ] {
        let mut h = Harness::new();
        let face = modeling_rectangle(&mut h);
        h.app.selected = if tool == Tool::PushPull || tool == Tool::Offset {
            BTreeSet::from([face])
        } else {
            h.app.doc.model.selectable_entities()
        };
        h.app.set_tool(tool);
        let start = h.at(DVec3::ZERO);
        h.click(start);
        let end = h.at(DVec3::new(1., 0.5, 1.));
        h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
        let before = h.app.doc.model.clone();
        let undo = h.app.history.undo_label().map(str::to_owned);
        for input in ["bad", "NaN", "inf", "1e999", "-", "1.2.3"] {
            h.app.length_input = input.into();
            h.key(Key::Enter);
            assert_eq!(h.app.doc.model, before, "{tool:?}: {input}");
            assert_eq!(h.app.history.undo_label(), undo.as_deref());
            h.app.doc.model.validate().unwrap();
        }
        h.key(Key::Escape);
        assert!(h.app.modeling.is_none());
        assert!(h.app.line_start.is_none());
    }
}

#[test]
fn all_keyboard_tool_shortcuts_select_the_expected_tool() {
    let mut h = Harness::new();
    for (key, tool) in [
        (Key::Space, Tool::Select),
        (Key::L, Tool::Line),
        (Key::R, Tool::Rectangle),
        (Key::C, Tool::Circle),
        (Key::G, Tool::Polygon),
        (Key::D, Tool::Freehand),
        (Key::P, Tool::PushPull),
        (Key::S, Tool::Scale),
        (Key::F, Tool::Offset),
        (Key::M, Tool::Move),
        (Key::B, Tool::Paint),
        (Key::T, Tool::TapeMeasure),
        (Key::A, Tool::Protractor),
        (Key::I, Tool::Dimensions),
        (Key::E, Tool::Erase),
        (Key::O, Tool::Orbit),
    ] {
        let camera = h.app.doc.camera.clone();
        h.key(key);
        assert_eq!(h.app.tool, tool);
        if key == Key::F {
            assert_eq!(Tool::Offset.shortcut(), "F");
            assert_eq!(h.app.doc.camera.target, camera.target);
            assert_eq!(h.app.doc.camera.distance, camera.distance);
        }
    }
    h.key(Key::F1);
    assert!(h.app.help);
    h.key(Key::Escape);
    assert!(!h.app.help);
}

#[test]
fn tag_ui_add_rename_cancel_delete_and_undo_preserve_geometry() {
    let mut h = options_workspace();
    let edge = h.app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    h.app.new_tag_name = "Structure".into();
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, "+"));
    assert_eq!(h.app.doc.model.tags.len(), 1);
    let tag = *h.app.doc.model.tags.keys().next().unwrap();
    h.app
        .doc
        .model
        .assign_tag(&BTreeSet::from([edge]), Some(tag))
        .unwrap();
    h.app.tag_rename = Some((tag, "Renamed".into()));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, "Save"));
    assert_eq!(h.app.doc.model.tags[&tag].name, "Renamed");
    h.app.tag_rename = Some((tag, "Cancelled".into()));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, "Cancel"));
    assert_eq!(h.app.doc.model.tags[&tag].name, "Renamed");
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, "×"));
    assert!(h.app.doc.model.tags.is_empty());
    assert!(h.app.doc.model.entity_tags.is_empty());
    assert!(h.app.doc.model.edges.contains_key(&edge));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.tags[&tag].name, "Renamed");
    assert_eq!(h.app.doc.model.entity_tags[&edge], tag);
}

fn boolean_menu_choice(tool: Tool, count: usize, volume: f64) {
    let mut h = Harness::new();
    let (a, b) = ui_solid_pair(&mut h);
    h.app.selected = BTreeSet::from([a, b]);
    let before = h.app.doc.model.clone();
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "Union"));
    let menu = h.frame(vec![], egui::Modifiers::NONE);
    h.click(menu_text_position(&menu, tool.label()));
    if tool == Tool::Trim {
        // Trim deliberately requires an ordered cutter/target choice.
        h.click(h.at(DVec3::new(0.5, 0., 1.)));
        h.click(h.at(DVec3::new(2.5, 0., 1.)));
        assert!(h.app.doc.model.objects.contains_key(&a));
    }
    assert_eq!(h.app.tool, tool);
    assert_eq!(h.app.solid_tool, tool);
    assert_eq!(h.app.selected.len(), count);
    let actual: f64 = h
        .app
        .selected
        .iter()
        .map(|id| h.app.doc.model.solid_volume(*id).unwrap())
        .sum();
    assert!((actual - volume).abs() < 1e-6, "{tool:?}: {actual}");
    let result = h.app.doc.model.clone();
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
    h.app.undo(&h.ctx, true);
    assert_eq!(h.app.doc.model, result);
    Document::from_json(&serde_json::to_string(&h.app.doc).unwrap()).unwrap();
}
macro_rules! boolean {
    ($name:ident, $tool:ident, $count:literal, $volume:literal) => {
        #[test]
        fn $name() {
            boolean_menu_choice(Tool::$tool, $count, $volume);
        }
    };
}
boolean!(tool_outer_shell_menu, OuterShell, 1, 12.);
boolean!(tool_union_menu, Union, 1, 12.);
boolean!(tool_intersect_menu, Intersect, 1, 4.);
boolean!(tool_trim_menu, Trim, 1, 4.);
boolean!(tool_split_menu, Split, 3, 12.);

#[test]
fn new_project_save_discard_and_cancel_buttons_preserve_the_expected_work() {
    for choice in ["Save", "Discard changes", "Cancel"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("before-new.ketchup");
        let mut h = Harness::new();
        h.app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        let before = h.app.doc.model.clone();
        h.app.project.path = Some(path.clone());
        h.app.changed(&h.ctx);
        h.app.request_project(projects::Action::New, &h.ctx);
        h.frame(vec![], egui::Modifiers::NONE);
        let output = h.frame(vec![], egui::Modifiers::NONE);
        h.click(text_position(&output, choice));
        if choice == "Save" {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while h.app.project.pending.is_some() {
                assert!(std::time::Instant::now() < deadline, "Save did not finish");
                std::thread::sleep(Duration::from_millis(2));
                h.frame(vec![], egui::Modifiers::NONE);
            }
            assert_eq!(
                crate::project_files::decode(&std::fs::read(path).unwrap())
                    .unwrap()
                    .model,
                before
            );
        } else {
            assert!(!path.exists());
        }
        assert!(h.app.project.confirm.is_none());
        assert!(h.app.project.error.is_none());
        if choice == "Cancel" {
            assert_eq!(h.app.doc.model, before);
        } else {
            assert!(h.app.doc.model.edges.is_empty());
        }
    }
}

#[test]
fn malformed_clipboard_payloads_do_not_change_the_model_or_history() {
    let mut h = Harness::new();
    modeling_rectangle(&mut h);
    let before = h.app.doc.model.clone();
    let undo = h.app.history.undo_label().map(str::to_owned);
    for payload in [
        "ketchup-geometry-v1\n{",
        "ketchup-geometry-v1\nnull",
        "ketchup-geometry-v1\n{\"model\":{},\"anchor\":[0,0,0]}",
    ] {
        h.frame(
            vec![egui::Event::Paste(payload.into())],
            egui::Modifiers::NONE,
        );
        assert_eq!(h.app.doc.model, before);
        assert_eq!(h.app.history.undo_label(), undo.as_deref());
        assert!(h.app.paste_preview.is_none());
    }
}

#[test]
#[ignore = "Explicit per-tool CPU timings on a 4096-face import"]
fn every_tool_pointer_workload_benchmark() {
    let mut h = Harness::new();
    h.app.doc.model = crate::picking::tests::triangle_grid(4096);
    h.app.fit(&h.ctx);
    h.frame(vec![], egui::Modifiers::NONE);
    for tool in Tool::ALL {
        h.app.set_tool(tool);
        let mut total = Duration::ZERO;
        let mut maximum = Duration::ZERO;
        for i in 0..20 {
            let point = h.app.viewport.min
                + h.app.viewport.size() * egui::vec2(0.2 + i as f32 * 0.025, 0.47);
            let start = std::time::Instant::now();
            std::hint::black_box(h.frame(
                vec![egui::Event::PointerMoved(point)],
                egui::Modifiers::NONE,
            ));
            let elapsed = start.elapsed();
            total += elapsed;
            maximum = maximum.max(elapsed);
        }
        eprintln!(
            "TOOL_CPU {tool:?} mean={:.3} max={:.3} ms",
            (total / 20).as_secs_f64() * 1000.,
            maximum.as_secs_f64() * 1000.
        );
    }
}

fn color_button_position(h: &Harness, output: &egui::FullOutput, rgb: [u8; 3], below: f32) -> Pos2 {
    fn find(shape: &egui::Shape, color: Color32, left: f32, below: f32) -> Option<Pos2> {
        match shape {
            egui::Shape::Rect(r)
                if r.fill == color
                    && r.rect.left() > left
                    && r.rect.top() > below
                    && (20. ..=40.).contains(&r.rect.width())
                    && (20. ..=40.).contains(&r.rect.height()) =>
            {
                Some(r.rect.center())
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, color, left, below)),
            _ => None,
        }
    }
    let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    output
        .shapes
        .iter()
        .find_map(|s| find(&s.shape, color, h.size.x - 310., below))
        .unwrap_or_else(|| panic!("Missing color button {rgb:?}"))
}

fn edit_environment_color(key: &str) {
    let mut h = options_workspace();
    let before = serde_json::to_value(&h.app.doc.display).unwrap()[key].clone();
    let rgb = [
        before[0].as_u64().unwrap() as u8,
        before[1].as_u64().unwrap() as u8,
        before[2].as_u64().unwrap() as u8,
    ];
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(color_button_position(&h, &output, rgb, 0.));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    assert!(egui::Popup::is_any_open(&h.ctx));
    fn red_channel(shape: &egui::Shape) -> Option<Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with("R ") => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(red_channel),
            _ => None,
        }
    }
    let channel = output
        .shapes
        .iter()
        .find_map(|s| red_channel(&s.shape))
        .expect("Red channel input");
    h.pointer(channel, egui::PointerButton::Primary, true, false);
    let changed = channel + egui::vec2(30., 0.);
    h.frame(
        vec![egui::Event::PointerMoved(changed)],
        egui::Modifiers::NONE,
    );
    h.pointer(changed, egui::PointerButton::Primary, false, false);
    assert_ne!(
        serde_json::to_value(&h.app.doc.display).unwrap()[key],
        before,
        "{key}"
    );
    assert!(h.app.project.modified);
    h.key(Key::Escape);
    let saved = Document::from_json(&serde_json::to_string(&h.app.doc).unwrap()).unwrap();
    assert_eq!(saved.display, h.app.doc.display);
}
macro_rules! color {
    ($name:ident, $key:literal) => {
        #[test]
        fn $name() {
            edit_environment_color($key);
        }
    };
}
color!(color_background_picker, "background");
color!(color_sky_picker, "sky_color");
color!(color_ground_picker, "ground_color");
color!(color_horizon_picker, "horizon_color");

#[test]
fn paint_target_buttons_apply_to_a_face_or_whole_group_with_one_undo_step() {
    let mut h = Harness::new();
    let face = modeling_rectangle(&mut h);
    h.app.doc.model.push_pull(face, 2.).unwrap();
    let group = h
        .app
        .doc
        .model
        .make_object(
            &h.app.doc.model.selectable_entities(),
            false,
            "Paint target",
        )
        .unwrap();
    h.key(Key::B);
    assert_eq!(h.app.paint_target, PaintTarget::Face);
    let at = h.at(DVec3::new(2., 2., 2.));
    h.click(at);
    assert_eq!(h.app.doc.model.face_colors.len(), 1);
    h.app.undo(&h.ctx, false);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, "Group"));
    assert_eq!(h.app.paint_target, PaintTarget::Group);
    h.app.update_pointer(at);
    assert_eq!(h.app.hover, Some(group));
    h.click(at);
    assert_eq!(h.app.doc.model.face_colors.len(), 6);
    assert!(
        h.app
            .doc
            .model
            .face_colors
            .values()
            .all(|color| *color == h.app.paint_color)
    );
    assert_eq!(h.app.history.undo_label(), Some("Paint group"));
    let painted = h.app.doc.model.clone();
    h.app.undo(&h.ctx, false);
    assert!(h.app.doc.model.face_colors.is_empty());
    h.app.undo(&h.ctx, true);
    assert_eq!(h.app.doc.model, painted);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, "Face"));
    assert_eq!(h.app.paint_target, PaintTarget::Face);
}

#[test]
fn all_paint_swatches_apply_the_selected_color_to_a_face() {
    let mut h = Harness::new();
    let face = modeling_rectangle(&mut h);
    h.key(Key::B);
    for rgb in [
        [195, 96, 67],
        [225, 179, 87],
        [108, 148, 102],
        [95, 143, 177],
        [163, 121, 167],
        [236, 231, 215],
        [78, 84, 81],
    ] {
        let output = h.frame(vec![], egui::Modifiers::NONE);
        let below = sidebar_position(&h, &output, "Choose paint").y + 20.;
        h.click(color_button_position(&h, &output, rgb, below));
        assert_eq!(h.app.paint_color, rgb);
        h.click(h.at(DVec3::new(2., 2., 0.)));
        assert_eq!(h.app.doc.model.face_colors[&face], rgb);
        assert_eq!(h.app.history.undo_label(), Some("Paint face"));
    }
}

#[test]
fn freehand_return_to_start_closes_a_filled_face_and_is_one_undo_step() {
    let mut h = Harness::new();
    h.app.set_tool(Tool::Freehand);
    let start = h.at(DVec3::ZERO);
    h.pointer(start, egui::PointerButton::Primary, true, false);
    for point in [
        DVec3::X * 2.,
        DVec3::new(2., 2., 0.),
        DVec3::Y * 2.,
        DVec3::ZERO,
    ] {
        h.frame(
            vec![egui::Event::PointerMoved(h.at(point))],
            egui::Modifiers::NONE,
        );
    }
    h.pointer(start, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.doc.model.faces.len(), 1);
    assert_eq!(h.app.doc.model.edges.len(), 4);
    h.app.doc.model.validate().unwrap();
    h.app.undo(&h.ctx, false);
    assert!(h.app.doc.model.edges.is_empty());
    assert!(h.app.history.undo_label().is_none());
}

#[test]
fn move_copy_array_maximum_count_stays_reloadable_and_undoes_once() {
    let mut h = Harness::new();
    let before = begin_move_copy(&mut h, false);
    modeling_number(&mut h, "6");
    for input in ["1000x", "/1000"] {
        h.app.length_input = input.into();
        h.app.commit_move_array(&h.ctx);
        assert_eq!(h.app.doc.model.edges.len(), 1001, "{input}");
        h.app.doc.model.validate().unwrap();
        Document::from_json(&serde_json::to_string(&h.app.doc).unwrap()).unwrap();
    }
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
    assert!(h.app.history.undo_label().is_none());
}

#[test]
fn tag_assignment_and_untagged_choices_work_through_the_dropdown() {
    let mut h = options_workspace();
    let edge = h.app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    let tag = h.app.doc.model.add_tag("Structure").unwrap();
    h.app.selected.insert(edge);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, "Untagged"));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, "Structure"));
    assert_eq!(h.app.doc.model.entity_tags[&edge], tag);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, "Structure"));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(sidebar_position(&h, &output, "Untagged"));
    assert!(!h.app.doc.model.entity_tags.contains_key(&edge));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.entity_tags[&edge], tag);
    h.app.doc.model.validate().unwrap();
}
