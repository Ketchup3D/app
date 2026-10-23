use super::*;
use ketchup_core::DVec3;

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn skp_import_preserves_nested_geometry_holes_colors_and_metres() {
    let model = import_skp(
        include_bytes!("../tests/fixtures/placed-window.skp"),
        "Window",
    )
    .unwrap();
    model.validate().unwrap();
    assert_eq!(model.faces.len(), 2);
    assert_eq!(model.objects.len(), 4);
    assert!(model.faces.values().all(|face| face.holes.len() == 1));
    assert!(model.face_colors.values().all(|c| *c == [210, 45, 30]));
    assert_eq!(model.objects.last_key_value().unwrap().1.name, "Window");
    assert!(
        model
            .vertices
            .values()
            .any(|v| { v.position.distance(DVec3::new(0.254, 5.08, 0.508)) < 1e-9 })
    );
    assert!(import_skp(b"not a SketchUp file", "Bad").is_err());
    let modern = import_skp(
        include_bytes!("../tests/fixtures/modern-triangle.skp"),
        "Triangle",
    )
    .unwrap();
    assert_eq!(modern.faces.len(), 1);
    assert_eq!(modern.edges.len(), 3);
    modern.validate().unwrap();
}

#[test]
fn obj_export_preserves_hole_area_and_omits_hidden_geometry() {
    let mut model = Model::default();
    for corners in [
        [
            DVec3::ZERO,
            DVec3::X * 4.,
            DVec3::new(4., 4., 0.),
            DVec3::Y * 4.,
        ],
        [
            DVec3::new(1., 1., 0.),
            DVec3::new(3., 1., 0.),
            DVec3::new(3., 3., 0.),
            DVec3::new(1., 3., 0.),
        ],
    ] {
        for i in 0..4 {
            model.add_edge(corners[i], corners[(i + 1) % 4]).unwrap();
        }
    }
    let inner = model
        .faces
        .values()
        .find(|f| f.holes.is_empty())
        .unwrap()
        .id;
    model.erase(&std::collections::BTreeSet::from([inner]));
    let hidden = model.add_edge(DVec3::X * 10., DVec3::X * 11.).unwrap();
    model.hidden.insert(hidden);
    let obj = export_obj(&model).unwrap();
    let vertices: Vec<DVec3> = obj
        .lines()
        .filter_map(|line| line.strip_prefix("v "))
        .map(|line| {
            let xyz: Vec<f64> = line
                .split_whitespace()
                .map(|s| s.parse().unwrap())
                .collect();
            DVec3::new(xyz[0], xyz[1], xyz[2])
        })
        .collect();
    assert!(vertices.iter().all(|p| p.x <= 4.));
    let area: f64 = obj
        .lines()
        .filter_map(|line| line.strip_prefix("f "))
        .map(|line| {
            let ids: Vec<usize> = line
                .split_whitespace()
                .map(|s| s.parse::<usize>().unwrap() - 1)
                .collect();
            let [a, b, c] = [vertices[ids[0]], vertices[ids[1]], vertices[ids[2]]];
            (b - a).cross(c - a).length() * 0.5
        })
        .sum();
    assert!((area - 12.).abs() < 1e-9);
    assert!(export_obj(&Model::default()).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_skp_rejects_truncated_containers_without_panicking() {
    for bytes in [
        include_bytes!("../tests/fixtures/placed-window.skp").as_slice(),
        include_bytes!("../tests/fixtures/modern-triangle.skp").as_slice(),
    ] {
        for length in [0, 1, 2, 10, 24, 64, 100, bytes.len() / 2] {
            assert!(
                import_skp(&bytes[..length.min(bytes.len())], "Truncated").is_err(),
                "length={length}"
            );
        }
    }
}
