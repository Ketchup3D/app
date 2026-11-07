use super::tests::board_at;
use super::*;
use crate::{
    Document,
    connections::{Connection, ConnectionOperation},
};

fn crossing(axis: DVec3, degrees: f64) -> (Model, [u64; 2]) {
    let mut model = Model::default();
    let lo = DVec3::new(-0.4, -0.06, -0.02);
    let hi = -lo;
    let members = [
        board_at(&mut model, lo, hi, false),
        board_at(&mut model, lo, hi, true),
    ];
    model
        .rotate_selection(&BTreeSet::from([members[1]]), DVec3::ZERO, axis, degrees)
        .unwrap();
    (model, members)
}

#[test]
fn arbitrary_relative_angles_cut_complementary_profiles_without_removing_other_stock() {
    for (axis, degrees) in [
        (DVec3::Z, 30.),
        (DVec3::Z, -125.),
        (DVec3::new(0.3, 0.8, 1.), 57.),
    ] {
        for reverse in [false, true] {
            for kind in JointKind::ALL {
                let (mut model, mut members) = crossing(axis, degrees);
                if reverse {
                    members.swap(0, 1);
                }
                let stock = JointStock::new(&model, members).unwrap();
                let scale =
                    model.solid_volume(members[0]).unwrap() / csg::volume(&stock.solids[0]).abs();
                let expected = members
                    .iter()
                    .map(|id| model.solid_volume(*id).unwrap())
                    .sum::<f64>()
                    - csg::volume(&stock.overlap).abs() * scale;
                let mut settings = JointSettings {
                    kind,
                    clearance: 0.,
                    taper_degrees: 1.,
                    ..Default::default()
                };
                let supported = model.supported_joinery_axes(members, &settings);
                assert!(!supported.is_empty(), "{kind:?}, angle={degrees}");
                (settings.depth_axis, settings.width_axis) = supported[0];
                model
                    .test_cut_joint(members, &settings)
                    .unwrap_or_else(|error| {
                        panic!("{kind:?}, angle={degrees}, reverse={reverse}: {error}")
                    });
                model.validate().unwrap();
                let result = members.map(|id| {
                    solids::import(&model.copy_entities(&BTreeSet::from([id])), stock.frame)
                        .unwrap()
                });
                let total = members
                    .iter()
                    .map(|id| model.solid_volume(*id).unwrap())
                    .sum::<f64>();
                assert!(
                    (total - expected).abs() < 1e-8,
                    "{kind:?} must partition only the true overlap"
                );
                let remaining_overlap = csg::apply(
                    result[0].clone(),
                    result[1].clone(),
                    Boolean::Intersect,
                    stock.frame.epsilon,
                )
                .unwrap();
                assert!(csg::volume(&remaining_overlap).abs() * scale < 1e-9);
                for i in 0..2 {
                    let outside = csg::apply(
                        stock.solids[i].clone(),
                        stock.solids[1 - i].clone(),
                        Boolean::Difference,
                        stock.frame.epsilon,
                    )
                    .unwrap();
                    let lost = csg::apply(
                        outside,
                        result[i].clone(),
                        Boolean::Difference,
                        stock.frame.epsilon,
                    )
                    .unwrap();
                    assert!(
                        csg::volume(&lost).abs() * scale < 1e-9,
                        "non-overlapping stock must remain intact"
                    );
                }
            }
        }
    }
}

#[test]
fn non_rectangular_stock_is_machined_using_its_real_surfaces() {
    let (mut model, members) = crossing(DVec3::Z, 35.);
    for id in model.selection_vertices(&BTreeSet::from([members[0]])) {
        let vertex = model.vertices.get_mut(&id).unwrap();
        vertex.position.x += vertex.position.z * 0.7;
    }
    let before = model.clone();
    let settings = JointSettings {
        kind: JointKind::HalfLap,
        clearance: 0.,
        ..Default::default()
    };
    assert!(!model.supported_joinery_axes(members, &settings).is_empty());
    model.test_cut_joint(members, &settings).unwrap();
    model.validate().unwrap();
    for id in members {
        assert!(model.solid_volume(id).unwrap() < before.solid_volume(id).unwrap());
    }
}

#[test]
fn either_board_and_either_end_trim_flush_without_changing_shared_material() {
    let (source, members) = crossing(DVec3::Z, 35.);
    let settings = JointSettings {
        kind: JointKind::HalfLap,
        clearance: 0.,
        ..Default::default()
    };
    let stock = JointStock::new(&source, members).unwrap();
    let mut untrimmed = source.clone();
    untrimmed.test_cut_joint(members, &settings).unwrap();
    for trim in [TrimBoard::First, TrimBoard::Second] {
        let index = trim.member().unwrap();
        for keep_end in [KeepEnd::Lower, KeepEnd::Upper] {
            let mut model = source.clone();
            model
                .test_cut_joint(
                    members,
                    &JointSettings {
                        trim,
                        keep_end,
                        ..settings.clone()
                    },
                )
                .unwrap();
            model.validate().unwrap();
            assert!(
                model.solid_volume(members[index]).unwrap()
                    < untrimmed.solid_volume(members[index]).unwrap()
            );
            assert!(
                (model.solid_volume(members[1 - index]).unwrap()
                    - untrimmed.solid_volume(members[1 - index]).unwrap())
                .abs()
                    < 1e-10
            );
            let imported = |model: &Model| {
                solids::import(
                    &model.copy_entities(&BTreeSet::from([members[index]])),
                    stock.frame,
                )
                .unwrap()
            };
            let lost = csg::apply(
                imported(&untrimmed),
                imported(&model),
                Boolean::Difference,
                stock.frame.epsilon,
            )
            .unwrap();
            assert!(!lost.is_empty(), "the selected excess must be removed");
            let lost_joint = csg::apply(
                lost,
                stock.overlap.clone(),
                Boolean::Intersect,
                stock.frame.epsilon,
            )
            .unwrap();
            assert!(
                csg::volume(&lost_joint).abs() < 1e-10,
                "trimming must preserve the joint material"
            );
            let surfaces = imported(&model);
            let flush = surfaces.iter().any(|face| {
                stock.solids[1 - index].iter().any(|other| {
                    face.normal.dot(other.normal) > 1. - 1e-8
                        && (face.w - other.w).abs() < 1e-8
                        && other.normal.x.abs() > 0.1
                        && other.normal.y.abs() > 0.1
                })
            });
            if trim == TrimBoard::First {
                assert!(flush, "trimmed end must follow the angled receiving face");
            }
        }
    }
}

#[test]
fn trim_defaults_roundtrip_and_cache_invalidate_without_baking_source() {
    let legacy: JointSettings = serde_json::from_str(r#"{"kind":"HalfLap"}"#).unwrap();
    assert_eq!(
        (legacy.trim, legacy.keep_end),
        (TrimBoard::None, KeepEnd::Lower)
    );
    assert_eq!(legacy.trim_axis, None);
    assert!(
        !JointSettings {
            trim_axis: Some(3),
            ..legacy.clone()
        }
        .valid()
    );
    let (mut source, members) = crossing(DVec3::Z, 35.);
    let id = source
        .add_connection(Connection {
            members,
            entry_face: None,
            operation: ConnectionOperation::Joinery(legacy.clone()),
        })
        .unwrap();
    let mut cache = EvaluationCache::default();
    assert!(cache.evaluate(&source).errors.is_empty());
    for (trim, keep_end, trim_axis) in [
        (TrimBoard::First, KeepEnd::Lower, None),
        (TrimBoard::First, KeepEnd::Upper, Some(0)),
        (TrimBoard::Second, KeepEnd::Upper, Some(1)),
    ] {
        source.connections.get_mut(&id).unwrap().operation =
            ConnectionOperation::Joinery(JointSettings {
                trim,
                keep_end,
                trim_axis,
                ..legacy.clone()
            });
        let saved = source.clone();
        let evaluations = cache.evaluations;
        let result = cache.evaluate(&source);
        let fresh = source.evaluate_joinery();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(cache.evaluations, evaluations + 1);
        for member in members {
            assert!(
                (result.model.solid_volume(member).unwrap()
                    - fresh.model.solid_volume(member).unwrap())
                .abs()
                    < 1e-10
            );
        }
        assert_eq!(source, saved);
        let restored = Document::from_json(
            &serde_json::to_string(&Document {
                model: source.clone(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(restored.model.connections, source.connections);
        assert!(restored.model.evaluate_joinery().errors.is_empty());
    }
    for offset in [1., -1.] {
        source
            .move_selection(&BTreeSet::from([members[1]]), DVec3::Z * offset)
            .unwrap();
        let saved = source.clone();
        let result = cache.evaluate(&source);
        let fresh = source.evaluate_joinery();
        assert_eq!(result.errors, fresh.errors);
        assert_eq!(result.errors.is_empty(), offset < 0.);
        assert_eq!(source, saved);
    }
    source
        .rotate_selection(
            &BTreeSet::from([members[1]]),
            DVec3::ZERO,
            DVec3::new(0.3, 0.8, 1.),
            17.,
        )
        .unwrap();
    let result = cache.evaluate(&source);
    let fresh = source.evaluate_joinery();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.errors, fresh.errors);
    for member in members {
        assert!(
            (result.model.solid_volume(member).unwrap()
                - fresh.model.solid_volume(member).unwrap())
            .abs()
                < 1e-9
        );
    }
}

#[test]
fn projected_boxes_that_overlap_without_shared_stock_do_not_create_a_joint() {
    let (mut source, members) = crossing(DVec3::Z, -45.);
    source
        .move_selection(&BTreeSet::from([members[1]]), DVec3::new(0.6, 0.32, 0.))
        .unwrap();
    let axes = source.joinery_frame(members[0]).unwrap();
    let a = axes.bounds(&source, members[0]).unwrap();
    let b = axes.bounds(&source, members[1]).unwrap();
    assert!((a.1.min(b.1) - a.0.max(b.0)).min_element() > 0.);
    assert!(JointStock::new(&source, members).is_err());
    let before = source.clone();
    assert!(
        source
            .test_cut_joint(members, &JointSettings::default())
            .is_err()
    );
    assert_eq!(source, before);
}
