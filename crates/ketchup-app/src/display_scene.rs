use super::*;
use ketchup_core::DisplaySettings;

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
    let eye = camera.eye();
    let parallel =
        (camera.projection == ketchup_core::Projection::Parallel).then_some(eye - camera.target);
    model
        .edges
        .values()
        .filter(|edge| {
            // Topology is independent of camera, visibility and materials. Keep it
            // with the triangulation cache, and only classify visible neighbors.
            let mut neighbors =
                cache
                    .adjacent_faces(edge.a, edge.b)
                    .iter()
                    .filter(|id| entities.get(**id).displayed(d))
                    .map(|id| {
                        let face = &model.faces[id];
                        cache.get(*id).normal.dot(
                            parallel.unwrap_or(eye - model.vertices[&face.vertices[0]].position),
                        ) > 0.
                    });
            let Some(first) = neighbors.next() else {
                return true;
            };
            let Some(second) = neighbors.next() else {
                return true;
            };
            first != second || neighbors.any(|facing| facing != first)
        })
        .map(|edge| edge.id)
        .collect()
}

#[derive(PartialEq)]
struct SectionKey {
    geometry: u64,
    entities: u64,
    height: f64,
    fill: bool,
    hidden_objects: bool,
    hidden_geometry: bool,
}

#[derive(Default)]
pub(super) struct SectionCache {
    key: Option<SectionKey>,
    segments: Vec<(DVec3, DVec3)>,
    triangles: Vec<[DVec3; 3]>,
    #[cfg(test)]
    builds: usize,
}

impl SectionCache {
    fn update(
        &mut self,
        cache: &renderer::FaceCache,
        entities: &renderer::EntityCache,
        d: &DisplaySettings,
    ) {
        let key = SectionKey {
            geometry: cache.revision(),
            entities: entities.revision(),
            height: d.section_height,
            fill: d.section_fill && d.face_style != ketchup_core::FaceStyle::Wireframe,
            hidden_objects: d.hidden_objects,
            hidden_geometry: d.hidden_geometry,
        };
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.segments.clear();
        self.triangles.clear();
        let mut section = Model::default();
        for (_, face) in cache
            .iter()
            .filter(|(id, _)| entities.get(*id).displayed(d))
        {
            for triangle in &face.triangles {
                let mut hits = [DVec3::ZERO; 2];
                let mut count = 0;
                for i in 0..3 {
                    let a = triangle[i];
                    let b = triangle[(i + 1) % 3];
                    if (a.z <= key.height) != (b.z <= key.height) {
                        hits[count] = a.lerp(b, (key.height - a.z) / (b.z - a.z));
                        count += 1;
                    }
                }
                if count == 2 && hits[0].distance_squared(hits[1]) > 1e-12 {
                    self.segments.push((hits[0], hits[1]));
                    if key.fill {
                        let _ = section.add_edge(hits[0], hits[1]);
                    }
                }
            }
        }
        if key.fill {
            for face in section.faces.values() {
                self.triangles.extend(section.face_triangles(face));
            }
        }
        self.key = Some(key);
        #[cfg(test)]
        {
            self.builds += 1;
        }
    }
}

pub(super) fn paint_sections(
    mesh: &mut renderer::Mesh,
    section: &mut SectionCache,
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
        section.update(cache, entities, d);
        for &(a, b) in &section.segments {
            mesh.world_line(camera, a, b, 3., ACCENT);
        }
        for &triangle in &section.triangles {
            mesh.world_triangle(camera, triangle, faces.section);
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

    fn box_model() -> Model {
        let mut model = Model::default();
        let points = [
            DVec3::ZERO,
            DVec3::X * 2.,
            DVec3::new(2., 2., 0.),
            DVec3::Y * 2.,
        ];
        for i in 0..4 {
            model.add_edge(points[i], points[(i + 1) % 4]).unwrap();
        }
        let face = *model.faces.keys().next().unwrap();
        model.push_pull(face, 2.).unwrap();
        model
    }

    #[test]
    fn section_geometry_is_reused_and_tracks_geometry_visibility_and_fill() {
        let mut model = box_model();
        let mut faces = renderer::FaceCache::default();
        let mut entities = renderer::EntityCache::default();
        let mut section = SectionCache::default();
        let mut display = DisplaySettings {
            section_cuts: true,
            section_fill: true,
            section_height: 1.,
            ..Default::default()
        };
        faces.update(&model);
        entities.update(&model);
        section.update(&faces, &entities, &display);
        let area: f64 = section
            .triangles
            .iter()
            .map(|[a, b, c]| (*b - *a).cross(*c - *a).length() * 0.5)
            .sum();
        assert!((area - 4.).abs() < 1e-6);
        assert!(section.segments.iter().all(|(a, b)| a.z == 1. && b.z == 1.));
        section.update(&faces, &entities, &display);
        assert_eq!(section.builds, 1);
        for vertex in model.vertices.values_mut() {
            vertex.position.x *= 2.;
        }
        faces.update(&model);
        section.update(&faces, &entities, &display);
        let area: f64 = section
            .triangles
            .iter()
            .map(|[a, b, c]| (*b - *a).cross(*c - *a).length() * 0.5)
            .sum();
        assert!((area - 8.).abs() < 1e-6);
        assert_eq!(section.builds, 2);
        model.hidden.extend(model.faces.keys().copied());
        entities.update(&model);
        section.update(&faces, &entities, &display);
        assert!(section.segments.is_empty() && section.triangles.is_empty());
        display.hidden_geometry = true;
        section.update(&faces, &entities, &display);
        assert!(!section.segments.is_empty() && !section.triangles.is_empty());
        display.section_fill = false;
        section.update(&faces, &entities, &display);
        assert!(!section.segments.is_empty() && section.triangles.is_empty());
        display.section_height = 10.;
        section.update(&faces, &entities, &display);
        assert!(section.segments.is_empty());
        assert_eq!(section.builds, 6);
    }

    #[test]
    fn cached_adjacency_preserves_profiles_across_views_and_visibility_changes() {
        let mut model = box_model();
        let mut faces = renderer::FaceCache::default();
        let mut entities = renderer::EntityCache::default();
        faces.update(&model);
        let mut camera = Camera::default();
        for hidden in [false, true] {
            if hidden {
                model.hidden.insert(*model.faces.keys().next().unwrap());
            }
            entities.update(&model);
            for show_hidden in [false, true] {
                let display = DisplaySettings {
                    hidden_geometry: show_hidden,
                    ..Default::default()
                };
                for view in ketchup_core::StandardView::ALL {
                    camera.set_standard_view(view);
                    // Original per-frame adjacency provides an independent
                    // reference for the retained topology classification.
                    let mut adjacent = std::collections::BTreeMap::<_, Vec<bool>>::new();
                    for face in model
                        .faces
                        .values()
                        .filter(|f| entities.get(f.id).displayed(&display))
                    {
                        let facing = faces
                            .get(face.id)
                            .normal
                            .dot(camera.toward_eye(model.vertices[&face.vertices[0]].position))
                            > 0.;
                        for ring in std::iter::once(&face.vertices).chain(face.holes.iter()) {
                            for i in 0..ring.len() {
                                let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                                adjacent
                                    .entry((a.min(b), a.max(b)))
                                    .or_default()
                                    .push(facing);
                            }
                        }
                    }
                    let expected: BTreeSet<_> = model
                        .edges
                        .values()
                        .filter(|edge| {
                            adjacent
                                .get(&(edge.a.min(edge.b), edge.a.max(edge.b)))
                                .is_none_or(|neighbors| {
                                    neighbors.len() == 1
                                        || neighbors.iter().any(|f| *f != neighbors[0])
                                })
                        })
                        .map(|edge| edge.id)
                        .collect();
                    assert_eq!(
                        profile_edges(&model, &camera, &faces, &entities, &display),
                        expected
                    );
                }
            }
        }
    }

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
