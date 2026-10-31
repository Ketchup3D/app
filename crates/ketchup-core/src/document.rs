use crate::Camera;
use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const EPS: f64 = 1.0e-7;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub id: u64,
    pub position: DVec3,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: u64,
    pub a: u64,
    pub b: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub id: u64,
    pub vertices: Vec<u64>,
    #[serde(default)]
    pub holes: Vec<Vec<u64>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub vertices: BTreeMap<u64, Vertex>,
    pub edges: BTreeMap<u64, Edge>,
    #[serde(default)]
    pub faces: BTreeMap<u64, Face>,
    #[serde(default)]
    pub face_colors: BTreeMap<u64, [u8; 3]>,
    #[serde(default)]
    pub dimensions: BTreeMap<u64, crate::Dimension>,
    #[serde(default)]
    pub texts: BTreeMap<u64, crate::TextAnnotation>,
    #[serde(default)]
    pub objects: BTreeMap<u64, crate::Object>,
    #[serde(default)]
    pub stock_materials: BTreeMap<crate::MaterialId, crate::StockMaterial>,
    #[serde(default)]
    pub hidden: BTreeSet<u64>,
    #[serde(default)]
    pub locked: BTreeSet<u64>,
    #[serde(skip)]
    pub edit_context: Option<u64>,
    #[serde(default)]
    pub tags: BTreeMap<u64, crate::Tag>,
    #[serde(default)]
    pub entity_tags: BTreeMap<u64, u64>,
    pub(crate) next_id: u64,
}

impl Model {
    pub(crate) fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
    pub fn points(&self, edge: &Edge) -> (DVec3, DVec3) {
        (
            self.vertices[&edge.a].position,
            self.vertices[&edge.b].position,
        )
    }
    pub fn edge_length(&self, edge: &Edge) -> f64 {
        let (a, b) = self.points(edge);
        a.distance(b)
    }
    fn vertex_at(&mut self, position: DVec3, drawing_vertices: &BTreeSet<u64>) -> u64 {
        if let Some(v) = self
            .vertices
            .values()
            .find(|v| drawing_vertices.contains(&v.id) && v.position.distance(position) < EPS)
        {
            return v.id;
        }
        let id = self.id();
        self.vertices.insert(id, Vertex { id, position });
        id
    }
    pub(crate) fn insert_edge(&mut self, a: u64, b: u64) -> u64 {
        if let Some(e) = self
            .edges
            .values()
            .find(|e| (e.a == a && e.b == b) || (e.a == b && e.b == a))
        {
            return e.id;
        }
        let id = self.id();
        self.edges.insert(id, Edge { id, a, b });
        id
    }
    fn split_at(&mut self, vertex: u64, drawing_edges: &mut BTreeSet<u64>) {
        let point = self.vertices[&vertex].position;
        let splits: Vec<_> = self
            .edges
            .values()
            .filter(|e| drawing_edges.contains(&e.id))
            .filter(|e| e.a != vertex && e.b != vertex)
            .filter_map(|e| {
                let (a, b) = self.points(e);
                let t = (point - a).dot(b - a) / a.distance_squared(b);
                (t > EPS && t < 1.0 - EPS && (a + (b - a) * t).distance(point) < EPS)
                    .then_some(e.clone())
            })
            .collect();
        for edge in splits {
            for face in self.faces.values_mut() {
                for ring in std::iter::once(&mut face.vertices).chain(face.holes.iter_mut()) {
                    if let Some(i) = (0..ring.len()).find(|&i| {
                        let a = ring[i];
                        let b = ring[(i + 1) % ring.len()];
                        (a == edge.a && b == edge.b) || (a == edge.b && b == edge.a)
                    }) {
                        ring.insert(i + 1, vertex);
                    }
                }
            }
            self.edges.remove(&edge.id);
            drawing_edges.remove(&edge.id);
            drawing_edges.insert(self.insert_edge(edge.a, vertex));
            drawing_edges.insert(self.insert_edge(vertex, edge.b));
        }
    }
    pub fn add_edge(&mut self, start: DVec3, end: DVec3) -> Result<u64, &'static str> {
        if self.edit_context.is_some() {
            return self.edit_in_context(|model| model.add_edge(start, end));
        }
        if !start.is_finite() || !end.is_finite() {
            return Err("Coordinates must be finite");
        }
        // Keep newly drawn geometry within the same limits enforced on reload.
        // Reject before allocating vertices or splitting an existing edge.
        if start.abs().max_element() > 1e10 || end.abs().max_element() > 1e10 {
            return Err("Coordinates must be within ten billion meters of the origin");
        }
        if start.distance(end) < EPS {
            return Err("Give your line a little length");
        }
        let mut drawing_edges = self.drawing_edges();
        if let Some(edge) = self
            .edges
            .values()
            .filter(|e| drawing_edges.contains(&e.id))
            .find(|e| {
                let (a, b) = self.points(e);
                (a.distance(start) < EPS && b.distance(end) < EPS)
                    || (a.distance(end) < EPS && b.distance(start) < EPS)
            })
            .cloned()
        {
            // Retracing an existing boundary heals an intentionally erased face.
            if self.create_faces_with_edges(edge.a, edge.b, &drawing_edges) {
                return Ok(edge.id);
            }
            return Err("That edge already exists");
        }
        let drawing_vertices = drawing_edges
            .iter()
            .flat_map(|id| {
                let edge = &self.edges[id];
                [edge.a, edge.b]
            })
            .collect();
        let a = self.vertex_at(start, &drawing_vertices);
        let b = self.vertex_at(end, &drawing_vertices);
        self.split_at(a, &mut drawing_edges);
        self.split_at(b, &mut drawing_edges);
        let id = self.insert_edge(a, b);
        drawing_edges.insert(id);
        self.create_faces_with_edges(a, b, &drawing_edges);
        Ok(id)
    }
    pub fn erase(&mut self, ids: &BTreeSet<u64>) {
        if self.edit_context.is_some() {
            let _ = self.edit_in_context(|model| {
                model.erase(ids);
                Ok(())
            });
            return;
        }
        self.sync_dimensions();
        let ids = self.erasable_entities(ids);
        self.dimensions.retain(|id, _| !ids.contains(id));
        self.texts.retain(|id, _| !ids.contains(id));
        let removed: BTreeSet<_> = self
            .edges
            .values()
            .filter(|e| ids.contains(&e.id))
            .map(|e| (e.a.min(e.b), e.a.max(e.b)))
            .collect();
        self.faces.retain(|id, face| {
            !ids.contains(id)
                && !std::iter::once(&face.vertices)
                    .chain(face.holes.iter())
                    .any(|ring| {
                        (0..ring.len()).any(|i| {
                            let a = ring[i];
                            let b = ring[(i + 1) % ring.len()];
                            removed.contains(&(a.min(b), a.max(b)))
                        })
                    })
        });
        self.face_colors.retain(|id, _| self.faces.contains_key(id));
        self.edges.retain(|id, _| !ids.contains(id));
        let used: BTreeSet<_> = self.edges.values().flat_map(|e| [e.a, e.b]).collect();
        self.vertices.retain(|id, _| used.contains(id));
        self.sync_dimensions();
        self.objects.retain(|id, _| !ids.contains(id));
        self.clean_entity_state();
    }
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        // Expand inherited visibility once; walking every object's membership
        // for each edge becomes quadratic on imported assemblies.
        let mut hidden_roots = self.hidden.clone();
        hidden_roots.extend(self.entity_tags.iter().filter_map(|(entity, tag)| {
            self.tags
                .get(tag)
                .is_some_and(|tag| !tag.visible)
                .then_some(*entity)
        }));
        let hidden = self.expand_entities(&hidden_roots);
        let used: BTreeSet<_> = self
            .edges
            .values()
            .filter(|e| !hidden.contains(&e.id))
            .flat_map(|e| [e.a, e.b])
            .chain(
                self.faces
                    .values()
                    .filter(|f| !hidden.contains(&f.id))
                    .flat_map(|f| f.vertices.iter().chain(f.holes.iter().flatten()).copied()),
            )
            .collect();
        let vertices = used.iter().map(|id| self.vertices[id].position).chain(
            self.dimensions
                .values()
                .filter(|d| !hidden.contains(&d.id))
                .flat_map(|d| d.layout(self)),
        );
        let mut vertices = vertices.chain(
            self.texts
                .values()
                .filter(|t| !hidden.contains(&t.id))
                .flat_map(|t| t.layout(self)),
        );
        let first = vertices.next()?;
        Some(vertices.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.vertices.len() > 1_000_000
            || self.edges.len() > 1_000_000
            || self.faces.len() > 1_000_000
            || self.objects.len() > 1_000_000
            || self.dimensions.len() > 1_000_000
            || self.texts.len() > 1_000_000
        {
            return Err("Model is too large");
        }
        let max_id = self
            .vertices
            .keys()
            .chain(self.edges.keys())
            .chain(self.faces.keys())
            .chain(self.objects.keys())
            .chain(self.dimensions.keys())
            .chain(self.texts.keys())
            .max()
            .copied()
            .unwrap_or(0);
        if self.next_id < max_id || self.next_id > u64::MAX - 2_000_001 {
            return Err("Invalid model IDs");
        }
        for (id, v) in &self.vertices {
            if *id != v.id || !v.position.is_finite() || v.position.abs().max_element() > 1e10 {
                return Err("Invalid vertex");
            }
        }
        let mut pairs = BTreeSet::new();
        for (id, e) in &self.edges {
            if *id != e.id
                || e.a == e.b
                || !self.vertices.contains_key(&e.a)
                || !self.vertices.contains_key(&e.b)
            {
                return Err("Invalid edge");
            }
            if self.edge_length(e) < EPS || !pairs.insert((e.a.min(e.b), e.a.max(e.b))) {
                return Err("Invalid edge");
            }
        }
        let mut boundaries = BTreeSet::new();
        for (id, face) in &self.faces {
            if *id != face.id
                || self.vertices.contains_key(id)
                || self.edges.contains_key(id)
                || !boundaries.insert(crate::faces::canonical_ring(&face.vertices))
            {
                return Err("Invalid face ID");
            }
            for ring in std::iter::once(&face.vertices).chain(face.holes.iter()) {
                if ring.len() < 3
                    || ring.iter().any(|v| !self.vertices.contains_key(v))
                    || crate::faces::plane(self, ring).is_none()
                {
                    return Err("Invalid face boundary");
                }
                for i in 0..ring.len() {
                    let a = ring[i];
                    let b = ring[(i + 1) % ring.len()];
                    if !pairs.contains(&(a.min(b), a.max(b))) {
                        return Err("Face boundary is missing an edge");
                    }
                }
            }
            let triangles = self.face_triangles(face);
            let area: f64 = triangles
                .iter()
                .map(|[a, b, c]| (*b - *a).cross(*c - *a).length() * 0.5)
                .sum();
            if triangles.is_empty() || area <= EPS {
                return Err("Invalid face surface");
            }
        }
        self.validate_tags()?;
        self.validate_objects()?;
        self.validate_dimensions()?;
        self.validate_texts()?;
        if self
            .face_colors
            .keys()
            .any(|id| !self.faces.contains_key(id))
        {
            return Err("Paint refers to a missing face");
        }
        Ok(())
    }
    pub fn contains_entity(&self, id: &u64) -> bool {
        self.edges.contains_key(id)
            || self.faces.contains_key(id)
            || self.objects.contains_key(id)
            || self.dimensions.contains_key(id)
            || self.texts.contains_key(id)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// Assigned when tracking shared material usage; absent in older projects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<uuid::Uuid>,
    pub version: u32,
    pub name: String,
    pub model: Model,
    pub camera: Camera,
    /// Per-viewport framing belongs to this project, unlike reusable layout preferences.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub viewport_cameras: BTreeMap<u64, Camera>,
    /// Selection belongs to this project, including the object open for editing.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub selection: BTreeSet<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_context: Option<u64>,
    #[serde(default = "enabled")]
    pub snap_to_geometry: bool,
    #[serde(default = "enabled")]
    pub snap_to_axes: bool,
    #[serde(default)]
    pub display: crate::DisplaySettings,
}
fn enabled() -> bool {
    true
}
impl Default for Document {
    fn default() -> Self {
        Self {
            project_id: None,
            version: 2,
            name: "Untitled model".into(),
            model: Model::default(),
            camera: Camera::default(),
            viewport_cameras: BTreeMap::new(),
            selection: BTreeSet::new(),
            selection_context: None,
            snap_to_geometry: true,
            snap_to_axes: true,
            display: crate::DisplaySettings::default(),
        }
    }
}
impl Document {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let mut doc: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if doc.version != 1 && doc.version != 2 {
            return Err("Unsupported document version".into());
        }
        doc.model.validate().map_err(str::to_owned)?;
        if doc.version == 1 {
            let edges: Vec<_> = doc.model.edges.values().map(|e| (e.a, e.b)).collect();
            for (a, b) in edges {
                doc.model.create_faces(a, b);
            }
            doc.version = 2;
        }
        doc.model.initialize_instance_transforms();
        if !doc.display.valid() {
            return Err("Invalid display settings".into());
        }
        if !doc.camera.valid() || doc.viewport_cameras.values().any(|camera| !camera.valid()) {
            return Err("Invalid camera".into());
        }
        Ok(doc)
    }
}

#[derive(Default)]
pub struct History {
    undo: Vec<(String, Model)>,
    redo: Vec<(String, Model)>,
}
impl History {
    pub fn record(&mut self, label: impl Into<String>, before: Model, after: &Model) {
        if before == *after {
            return;
        }
        self.undo.push((label.into(), before));
        self.redo.clear();
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
    }
    pub fn undo(&mut self, model: &mut Model) -> bool {
        let Some((label, old)) = self.undo.pop() else {
            return false;
        };
        self.redo.push((label, std::mem::replace(model, old)));
        true
    }
    pub fn redo(&mut self, model: &mut Model) -> bool {
        let Some((label, new)) = self.redo.pop() else {
            return false;
        };
        self.undo.push((label, std::mem::replace(model, new)));
        true
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|v| v.0.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|v| v.0.as_str())
    }
}
