use super::*;
use ketchup_core::DisplaySettings;
use std::collections::BTreeMap;

/// A soft upper-left key light travels with the view, keeping depth legible as
/// the camera orbits. Prepare its basis once per frame rather than per face.
pub(super) struct FaceLighting {
    eye: DVec3,
    direction: DVec3,
    parallel: Option<DVec3>,
}

impl FaceLighting {
    pub(super) fn new(camera: &Camera) -> Self {
        let eye = camera.eye();
        let toward_eye = (eye - camera.target).normalize();
        let right = camera.right();
        let up = camera.up();
        Self {
            eye,
            parallel: (camera.projection == ketchup_core::Projection::Parallel)
                .then_some(toward_eye),
            direction: (toward_eye * 0.8 - right * 0.45 + up * 0.65).normalize(),
        }
    }

    pub(super) fn shade(&self, color: Color32, normal: DVec3, point: DVec3) -> Color32 {
        // Light the visible side of open faces and imported geometry, even
        // when their winding differs. Monochrome keeps its distinct back color.
        let normal = if normal.dot(self.parallel.unwrap_or(self.eye - point)) < 0. {
            -normal
        } else {
            normal
        };
        let brightness = 0.62 + 0.38 * normal.dot(self.direction).max(0.);
        let [r, g, b, alpha] = color.to_srgba_unmultiplied();
        let channel = |value| (f64::from(value) * brightness).round() as u8;
        Color32::from_rgba_unmultiplied(channel(r), channel(g), channel(b), alpha)
    }
}

/// Darken the face's own material while preserving X-Ray/hidden-face opacity.
pub(super) fn shadow_color(color: Color32, darkness: f64) -> Color32 {
    let [r, g, b, alpha] = color.to_srgba_unmultiplied();
    let channel = |value| (f64::from(value) * (1. - darkness)).round() as u8;
    Color32::from_rgba_unmultiplied(channel(r), channel(g), channel(b), alpha)
}

pub(super) fn profile_edges(
    model: &Model,
    camera: &Camera,
    cache: &renderer::FaceCache,
    entities: &renderer::EntityCache,
    d: &DisplaySettings,
) -> BTreeSet<u64> {
    let mut adjacent: BTreeMap<(u64, u64), Vec<bool>> = BTreeMap::new();
    for face in model
        .faces
        .values()
        .filter(|f| entities.get(f.id).displayed(d))
    {
        let facing = cache
            .get(face.id)
            .normal
            .dot(camera.toward_eye(model.vertices[&face.vertices[0]].position))
            > 0.;
        for ring in std::iter::once(&face.vertices).chain(face.holes.iter()) {
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                adjacent
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push(facing);
            }
        }
    }
    model
        .edges
        .values()
        .filter(|e| {
            adjacent
                .get(&(e.a.min(e.b), e.a.max(e.b)))
                .is_none_or(|faces| faces.len() == 1 || faces.iter().any(|f| *f != faces[0]))
        })
        .map(|e| e.id)
        .collect()
}

pub(super) fn paint_sections(
    mesh: &mut renderer::Mesh,
    camera: &Camera,
    model: &Model,
    cache: &renderer::FaceCache,
    entities: &renderer::EntityCache,
    d: &DisplaySettings,
    faces: &FacePalette,
) {
    if !d.section_cuts && !d.section_planes {
        return;
    }
    let z = d.section_height;
    if d.section_cuts {
        let mut section = Model::default();
        for face in model
            .faces
            .values()
            .filter(|f| entities.get(f.id).displayed(d))
        {
            for triangle in &cache.get(face.id).triangles {
                let mut hits = Vec::new();
                for i in 0..3 {
                    let a = triangle[i];
                    let b = triangle[(i + 1) % 3];
                    if (a.z <= z) != (b.z <= z) {
                        hits.push(a.lerp(b, (z - a.z) / (b.z - a.z)));
                    }
                }
                if hits.len() == 2 && hits[0].distance_squared(hits[1]) > 1e-12 {
                    let _ = section.add_edge(hits[0], hits[1]);
                    mesh.world_line(camera, hits[0], hits[1], 3., ACCENT);
                }
            }
        }
        if d.section_fill && d.face_style != ketchup_core::FaceStyle::Wireframe {
            for face in section.faces.values() {
                for triangle in section.face_triangles(face) {
                    mesh.world_triangle(camera, triangle, faces.section);
                }
            }
        }
    }
    if d.section_planes {
        let (lo, hi) = model
            .bounds()
            .unwrap_or((DVec3::splat(-2.), DVec3::splat(2.)));
        let pad = (hi - lo).max_element().max(1.) * 0.1;
        let corners = [
            DVec3::new(lo.x - pad, lo.y - pad, z),
            DVec3::new(hi.x + pad, lo.y - pad, z),
            DVec3::new(hi.x + pad, hi.y + pad, z),
            DVec3::new(lo.x - pad, hi.y + pad, z),
        ];
        for i in 0..4 {
            mesh.overlay_line(corners[i], corners[(i + 1) % 4], 1.5, ACCENT, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadows_darken_materials_without_changing_face_opacity() {
        assert_eq!(
            shadow_color(Color32::from_rgb(200, 120, 80), 0.5),
            Color32::from_rgb(100, 60, 40)
        );
        let translucent = Color32::from_rgba_unmultiplied(200, 120, 80, 64);
        assert_eq!(shadow_color(translucent, 0.5).a(), translucent.a());
        assert_eq!(shadow_color(Color32::WHITE, 0.), Color32::WHITE);
    }
}
