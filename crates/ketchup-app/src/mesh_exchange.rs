//! Strict, bounded interchange subset. Coordinates returned in metres, Z up.
use ketchup_core::{DVec3, Model};
use std::collections::{BTreeMap, BTreeSet};
const LIMIT: usize = 200_000;
fn number(s: &str) -> Result<f64, String> {
    let n: f64 = s.parse().map_err(|_| format!("Invalid number: {s}"))?;
    if !n.is_finite() || n.abs() > 1e9 {
        return Err("Non-finite or excessive coordinate".into());
    }
    Ok(n)
}
#[derive(Default)]
struct Mesh {
    points: Vec<DVec3>,
    faces: Vec<Vec<usize>>,
    lines: Vec<Vec<usize>>,
}
impl Mesh {
    fn point(&mut self, p: DVec3) -> Result<usize, String> {
        if self.points.len() >= LIMIT || !p.is_finite() || p.abs().max_element() > 1e9 {
            return Err("Import exceeds coordinate or 200,000 vertex limit".into());
        }
        self.points.push(p);
        Ok(self.points.len() - 1)
    }
    fn finish(self, name: &str) -> Result<Model, String> {
        let mut vertices = BTreeMap::new();
        let mut edges = BTreeMap::new();
        let mut faces = BTreeMap::new();
        let mut positions = BTreeMap::new();
        let mut pairs = BTreeSet::new();
        let mut next = 0u64;
        let mut ids = Vec::new();
        for p in self.points {
            let key = p.to_array().map(|v| if v == 0. { 0 } else { v.to_bits() });
            let id = *positions.entry(key).or_insert_with(|| {
                next += 1;
                vertices.insert(next, serde_json::json!({"id":next,"position":p.to_array()}));
                next
            });
            ids.push(id);
        }
        let mut entities = Vec::new();
        for (face, rings) in [(true, self.faces), (false, self.lines)] {
            for ring in rings {
                if next > 1_000_000 {
                    return Err("Import exceeds one million entities".into());
                }
                let ring: Vec<u64> = ring.into_iter().map(|i| ids[i]).collect();
                if ring.len() < if face { 3 } else { 2 } || ring.windows(2).any(|p| p[0] == p[1]) {
                    return Err("Degenerate geometry".into());
                }
                let count = if face { ring.len() } else { ring.len() - 1 };
                for i in 0..count {
                    let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                    let key = (a.min(b), a.max(b));
                    if a == b {
                        return Err("Degenerate edge".into());
                    }
                    if pairs.insert(key) {
                        next += 1;
                        entities.push(next);
                        edges.insert(next, serde_json::json!({"id":next,"a":a,"b":b}));
                    }
                }
                if face {
                    next += 1;
                    entities.push(next);
                    faces.insert(
                        next,
                        serde_json::json!({"id":next,"vertices":ring,"holes":[]}),
                    );
                }
            }
        }
        if next >= 1_000_000 {
            return Err("Import exceeds one million entities".into());
        }
        if entities.is_empty() {
            return Err("No supported geometry in file".into());
        }
        next += 1;
        let mut objects = BTreeMap::new();
        objects.insert(
            next,
            serde_json::json!({"id":next,"name":name,"kind":"Group","entities":entities}),
        );
        let model: Model = serde_json::from_value(serde_json::json!({"vertices":vertices,"edges":edges,"faces":faces,"objects":objects,"next_id":next})).map_err(|e|e.to_string())?;
        model.validate().map_err(str::to_owned)?;
        Ok(model)
    }
}
pub fn import(bytes: &[u8], extension: &str, name: &str) -> Result<Model, String> {
    if bytes.len() > super::interchange::MAX_SKP_BYTES {
        return Err("Import is limited to 64 MiB".into());
    }
    match extension {
        "stl" => stl(bytes)?.finish(name),
        "obj" => {
            obj(std::str::from_utf8(bytes).map_err(|_| "OBJ must be UTF-8 text")?)?.finish(name)
        }
        "dxf" => {
            dxf(std::str::from_utf8(bytes).map_err(|_| "Only text DXF is supported")?)?.finish(name)
        }
        _ => Err("Unsupported import format".into()),
    }
}
fn stl(bytes: &[u8]) -> Result<Mesh, String> {
    let mut mesh = Mesh::default();
    if bytes.len() >= 84 {
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        if count <= LIMIT / 3 && 84 + count * 50 == bytes.len() {
            for triangle in bytes[84..].chunks_exact(50) {
                let mut face = Vec::new();
                for p in triangle[12..48].chunks_exact(12) {
                    let mut xyz = [0.; 3];
                    for (i, c) in p.chunks_exact(4).enumerate() {
                        xyz[i] = f32::from_le_bytes(c.try_into().unwrap()) as f64 * 0.001;
                    }
                    face.push(mesh.point(DVec3::from_array(xyz))?);
                }
                mesh.faces.push(face);
            }
            return Ok(mesh);
        }
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Invalid binary STL length")?;
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if !lines
        .next()
        .is_some_and(|l| l == "solid" || l.starts_with("solid "))
    {
        return Err("Expected ASCII STL solid".into());
    }
    let mut ended = false;
    while let Some(line) = lines.next() {
        if line == "endsolid" || line.starts_with("endsolid ") {
            ended = true;
            break;
        }
        let normal: Vec<_> = line.split_whitespace().collect();
        if normal.len() != 5 || normal[..2] != ["facet", "normal"] {
            return Err("Expected STL facet normal".into());
        }
        for n in &normal[2..] {
            number(n)?;
        }
        if lines.next() != Some("outer loop") {
            return Err("Expected STL outer loop".into());
        }
        let mut face = Vec::new();
        for _ in 0..3 {
            let v: Vec<_> = lines
                .next()
                .ok_or("Truncated STL")?
                .split_whitespace()
                .collect();
            if v.len() != 4 || v[0] != "vertex" {
                return Err("Expected STL vertex".into());
            }
            face.push(mesh.point(DVec3::new(number(v[1])?, number(v[2])?, number(v[3])?) * 0.001)?);
        }
        if lines.next() != Some("endloop") || lines.next() != Some("endfacet") {
            return Err("Invalid STL facet ending".into());
        }
        mesh.faces.push(face);
    }
    if !ended || lines.next().is_some() {
        return Err("Invalid STL ending".into());
    }
    Ok(mesh)
}
fn obj(text: &str) -> Result<Mesh, String> {
    let mut mesh = Mesh::default();
    for line in text.lines() {
        let words: Vec<_> = line
            .split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        if words.is_empty() {
            continue;
        }
        match words[0] {
            "v" => {
                if words.len() != 4 {
                    return Err("OBJ vertices must have exactly XYZ coordinates".into());
                }
                mesh.point(DVec3::new(
                    number(words[1])?,
                    number(words[2])?,
                    number(words[3])?,
                ))?;
            }
            "f" | "l" => {
                if words.len() > 4097 {
                    return Err("OBJ polygon exceeds 4096 vertices".into());
                }
                let mut ring = Vec::new();
                for w in &words[1..] {
                    let raw = w.split('/').next().unwrap_or("");
                    let n: i64 = raw.parse().map_err(|_| "Invalid OBJ vertex index")?;
                    let i = if n < 0 {
                        mesh.points.len() as i64 + n
                    } else {
                        n - 1
                    };
                    if n == 0 || i < 0 || i >= mesh.points.len() as i64 {
                        return Err(
                            "OBJ vertex index out of range (forward references unsupported)".into(),
                        );
                    }
                    ring.push(i as usize);
                }
                if mesh.faces.len() + mesh.lines.len() >= LIMIT {
                    return Err("Too many OBJ primitives".into());
                }
                if words[0] == "f" {
                    mesh.faces.push(ring);
                } else {
                    mesh.lines.push(ring);
                }
            }
            "vt" | "vn" | "o" | "g" | "s" | "usemtl" | "mtllib" => {}
            _ => return Err(format!("Unsupported OBJ statement: {}", words[0])),
        }
    }
    Ok(mesh)
}
fn dxf(text: &str) -> Result<Mesh, String> {
    let lines: Vec<_> = text.lines().collect();
    if !lines.len().is_multiple_of(2) {
        return Err("Truncated DXF code/value pair".into());
    }
    let pairs: Vec<(i32, &str)> = lines
        .chunks_exact(2)
        .map(|p| {
            Ok((
                p[0].trim().parse().map_err(|_| "Invalid DXF group code")?,
                p[1].trim(),
            ))
        })
        .collect::<Result<_, String>>()?;
    if pairs.last() != Some(&(0, "EOF")) {
        return Err("DXF must end with EOF".into());
    }
    let mut scale = 0.001;
    for i in 0..pairs.len().saturating_sub(1) {
        if pairs[i] == (9, "$INSUNITS") {
            if pairs[i + 1].0 != 70 {
                return Err("Invalid DXF units".into());
            }
            scale = match pairs[i + 1].1 {
                "0" | "4" => 0.001,
                "1" => 0.0254,
                "2" => 0.3048,
                "5" => 0.01,
                "6" => 1.,
                _ => return Err("DXF units unsupported (use mm, cm, m, inches or feet)".into()),
            };
        }
    }
    let mut mesh = Mesh::default();
    let mut section = "";
    let mut i = 0;
    let mut found = false;
    while i < pairs.len() {
        if pairs[i] == (0, "SECTION") {
            if section != "" || pairs.get(i + 1).map(|p| p.0) != Some(2) {
                return Err("Invalid DXF section".into());
            }
            section = pairs[i + 1].1;
            i += 2;
            continue;
        }
        if pairs[i] == (0, "ENDSEC") {
            section = "";
            i += 1;
            continue;
        }
        if section != "ENTITIES" {
            i += 1;
            continue;
        }
        found = true;
        if pairs[i].0 != 0 {
            return Err("Expected DXF entity".into());
        }
        let kind = pairs[i].1;
        let start = i + 1;
        i = start;
        while i < pairs.len() && pairs[i].0 != 0 {
            i += 1;
        }
        let fields = &pairs[start..i];
        let get = |code: i32, default: Option<f64>| -> Result<f64, String> {
            match fields.iter().find(|p| p.0 == code) {
                Some(p) => number(p.1),
                None => default.ok_or_else(|| format!("Missing DXF field {code}")),
            }
        };
        if get(30, Some(0.))? != 0.
            || get(31, Some(0.))? != 0.
            || get(38, Some(0.))? != 0.
            || get(39, Some(0.))? != 0.
            || get(210, Some(0.))? != 0.
            || get(220, Some(0.))? != 0.
            || get(230, Some(1.))? != 1.
        {
            return Err(
                "Only DXF geometry in the XY plane with default extrusion is supported".into(),
            );
        }
        let mut points = Vec::new();
        match kind {
            "LINE" => {
                points.push(DVec3::new(get(10, None)?, get(20, None)?, 0.));
                points.push(DVec3::new(get(11, None)?, get(21, None)?, 0.));
            }
            "CIRCLE" | "ARC" => {
                let center = DVec3::new(get(10, None)?, get(20, None)?, 0.);
                let r = get(40, None)?;
                if r <= 0. {
                    return Err("DXF radius must be positive".into());
                }
                let a = if kind == "ARC" {
                    get(50, None)?.to_radians()
                } else {
                    0.
                };
                let mut span = if kind == "ARC" {
                    (get(51, None)?.to_radians() - a).rem_euclid(std::f64::consts::TAU)
                } else {
                    std::f64::consts::TAU
                };
                if span == 0. {
                    span = std::f64::consts::TAU;
                }
                let steps = (span / std::f64::consts::TAU * 96.).ceil().max(1.) as usize;
                for j in 0..=steps {
                    let t = a + span * j as f64 / steps as f64;
                    points.push(center + DVec3::new(t.cos() * r, t.sin() * r, 0.));
                }
                if kind == "CIRCLE" {
                    let first = points[0];
                    *points.last_mut().unwrap() = first;
                }
            }
            "LWPOLYLINE" => {
                let flags = get(70, Some(0.))?;
                if flags != 0. && flags != 1. {
                    return Err("Unsupported DXF polyline flags".into());
                }
                if fields
                    .iter()
                    .any(|p| matches!(p.0, 40..=42) && p.1.parse::<f64>().ok() != Some(0.))
                    || get(43, Some(0.))? != 0.
                {
                    return Err("DXF bulges and polyline widths are unsupported".into());
                }
                let mut x = None;
                for &(code, value) in fields {
                    match code {
                        10 => {
                            if x.is_some() {
                                return Err("Missing DXF polyline Y".into());
                            }
                            x = Some(number(value)?);
                        }
                        20 => {
                            points.push(DVec3::new(
                                x.take().ok_or("Missing DXF polyline X")?,
                                number(value)?,
                                0.,
                            ));
                        }
                        _ => {}
                    }
                }
                if x.is_some() || get(90, None)? != points.len() as f64 || points.len() < 2 {
                    return Err("Invalid DXF polyline vertex count".into());
                }
                if flags == 1. {
                    points.push(points[0]);
                }
            }
            _ => {
                return Err(format!(
                    "Unsupported DXF entity {kind}. Supported: LINE, LWPOLYLINE without bulges/widths, CIRCLE, ARC; XY plane only."
                ));
            }
        }
        let mut ring = Vec::new();
        for p in points {
            ring.push(mesh.point(p * scale)?);
        }
        mesh.lines.push(ring);
    }
    if section != "" || !found {
        return Err("Missing or incomplete DXF ENTITIES section".into());
    }
    Ok(mesh)
}
/// STL has no unit metadata: write millimetres, matching our import convention.
pub fn export_stl(model: &Model) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; 84];
    let header = b"KetchUp3D STL; units: millimetres";
    out[..header.len()].copy_from_slice(header);
    let mut count = 0u32;
    for face in model.faces.values().filter(|f| !model.is_hidden(f.id)) {
        for triangle in model.face_triangles(face) {
            if count >= LIMIT as u32 {
                return Err("STL export exceeds 200,000 triangles".into());
            }
            let normal = (triangle[1] - triangle[0]).cross(triangle[2] - triangle[0]);
            if !normal.is_finite() || normal.length_squared() <= 1e-24 {
                return Err("Cannot export degenerate STL triangle".into());
            }
            for p in
                std::iter::once(normal.normalize()).chain(triangle.into_iter().map(|p| p * 1000.))
            {
                for n in p.to_array() {
                    let value = n as f32;
                    if !value.is_finite() {
                        return Err("STL coordinate overflow".into());
                    }
                    out.extend_from_slice(&value.to_le_bytes());
                }
            }
            out.extend_from_slice(&[0, 0]);
            count += 1;
        }
    }
    if count == 0 {
        return Err("There are no visible faces to export as STL".into());
    }
    out[80..84].copy_from_slice(&count.to_le_bytes());
    Ok(out)
}
#[cfg(test)]
mod tests {
    #[test]
    fn obj_file_fixture_preserves_polygon_size_and_rejects_bad_references() {
        let bytes = include_bytes!("../tests/fixtures/quad-negative.obj");
        let model = super::import(bytes, "obj", "Panel").unwrap();
        assert_eq!(model.faces.len(), 1);
        let (lo, hi) = model.bounds().unwrap();
        assert_eq!(lo, DVec3::ZERO);
        assert_eq!(hi, DVec3::new(1., 1., 0.));
        assert!(model.validate().is_ok());
        let corrupt = String::from_utf8_lossy(bytes).replace("f -4 -3 -2 -1", "f 1 2 3 999");
        assert!(super::import(corrupt.as_bytes(), "obj", "Panel").is_err());
    }
    use super::*;
    #[test]
    fn obj_indices_and_rejections() {
        let m = import(
            b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3/-1 -2/-1 -1/-1\n",
            "obj",
            "Part",
        )
        .unwrap();
        assert_eq!(m.faces.len(), 1);
        for bytes in [
            b"v NaN 0 0".as_slice(),
            b"v 0 0 0\nf 0 1 2",
            b"v 0 0 0\nf 1 2 3",
            b"curv 0 1 1 2",
        ] {
            assert!(import(bytes, "obj", "Bad").is_err());
        }
    }
    #[test]
    fn stl_ascii_binary_roundtrip_and_truncation() {
        let m = import(
            include_bytes!("../tests/fixtures/triangle-mm.stl"),
            "stl",
            "Part",
        )
        .unwrap();
        assert!(m.vertices.values().any(|v| v.position.x == 1.));
        let bytes = export_stl(&m).unwrap();
        assert_eq!(bytes.len(), 134);
        let restored = import(&bytes, "stl", "Part").unwrap();
        assert_eq!(restored.faces.len(), 1);
        for n in [0, 20, 83, 100, 133] {
            assert!(import(&bytes[..n], "stl", "Bad").is_err());
        }
        assert!(export_stl(&Model::default()).is_err());
    }
    #[test]
    fn stl_solid_binary_header_and_hidden_faces() {
        let m = import(
            include_bytes!("../tests/fixtures/triangle-mm.stl"),
            "stl",
            "Part",
        )
        .unwrap();
        let mut bytes = export_stl(&m).unwrap();
        bytes[..5].copy_from_slice(b"solid");
        assert_eq!(import(&bytes, "stl", "Part").unwrap().faces.len(), 1);
        let mut hidden = m.clone();
        hidden.hidden.extend(hidden.faces.keys().copied());
        assert!(export_stl(&hidden).is_err());
        bytes[96..100].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(import(&bytes, "stl", "Bad").is_err());
    }

    #[test]
    fn dxf_curves_polylines_units_and_rejections() {
        let wrap = |body: &str| format!("0\nSECTION\n2\nENTITIES\n{body}0\nENDSEC\n0\nEOF\n");
        let circle = wrap("0\nCIRCLE\n10\n0\n20\n0\n40\n1000\n");
        assert_eq!(
            import(circle.as_bytes(), "dxf", "Circle")
                .unwrap()
                .edges
                .len(),
            96
        );
        let arc = wrap("0\nARC\n10\n0\n20\n0\n40\n1000\n50\n0\n51\n90\n");
        assert_eq!(
            import(arc.as_bytes(), "dxf", "Arc").unwrap().edges.len(),
            24
        );
        let poly =
            wrap("0\nLWPOLYLINE\n90\n3\n70\n1\n10\n0\n20\n0\n10\n1000\n20\n0\n10\n0\n20\n1000\n");
        assert_eq!(
            import(poly.as_bytes(), "dxf", "Profile")
                .unwrap()
                .edges
                .len(),
            3
        );
        assert!(import(poly.replace("70\n1", "42\n1").as_bytes(), "dxf", "Bad").is_err());
        assert!(
            import(
                circle.replace("40\n1000", "30\n1\n40\n1000").as_bytes(),
                "dxf",
                "Bad"
            )
            .is_err()
        );
        assert!(import(circle.trim_end_matches("0\nEOF\n").as_bytes(), "dxf", "Bad").is_err());
        let metres = include_str!("../tests/fixtures/profile-mm.dxf").replace("70\n4", "70\n6");
        assert!(
            import(metres.as_bytes(), "dxf", "Metres")
                .unwrap()
                .vertices
                .values()
                .any(|v| v.position.x == 1000.)
        );
    }

    #[test]
    fn dxf_units_and_subset() {
        let text = include_str!("../tests/fixtures/profile-mm.dxf");
        let m = import(text.as_bytes(), "dxf", "Profile").unwrap();
        assert_eq!(m.edges.len(), 1);
        assert!(m.vertices.values().any(|v| v.position.x == 1.));
        assert!(import(text.replace("LINE", "INSERT").as_bytes(), "dxf", "Bad").is_err());
        assert!(import(text.replace("1000", "NaN").as_bytes(), "dxf", "Bad").is_err());
    }
}
