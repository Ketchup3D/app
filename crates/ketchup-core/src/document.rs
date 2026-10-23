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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub vertices: BTreeMap<u64, Vertex>,
    pub edges: BTreeMap<u64, Edge>,
    next_id: u64,
}

impl Model {
    fn id(&mut self) -> u64 {
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
    fn vertex_at(&mut self, position: DVec3) -> u64 {
        if let Some(v) = self
            .vertices
            .values()
            .find(|v| v.position.distance(position) < EPS)
        {
            return v.id;
        }
        let id = self.id();
        self.vertices.insert(id, Vertex { id, position });
        id
    }
    fn insert_edge(&mut self, a: u64, b: u64) -> u64 {
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
    fn split_at(&mut self, vertex: u64) {
        let point = self.vertices[&vertex].position;
        let splits: Vec<_> = self
            .edges
            .values()
            .filter(|e| e.a != vertex && e.b != vertex)
            .filter_map(|e| {
                let (a, b) = self.points(e);
                let t = (point - a).dot(b - a) / a.distance_squared(b);
                (t > EPS && t < 1.0 - EPS && (a + (b - a) * t).distance(point) < EPS)
                    .then_some(e.clone())
            })
            .collect();
        for edge in splits {
            self.edges.remove(&edge.id);
            self.insert_edge(edge.a, vertex);
            self.insert_edge(vertex, edge.b);
        }
    }
    pub fn add_edge(&mut self, start: DVec3, end: DVec3) -> Result<u64, &'static str> {
        if !start.is_finite() || !end.is_finite() {
            return Err("Coordinates must be finite");
        }
        if start.distance(end) < EPS {
            return Err("Give your line a little length");
        }
        if self.edges.values().any(|e| {
            let (a, b) = self.points(e);
            (a.distance(start) < EPS && b.distance(end) < EPS)
                || (a.distance(end) < EPS && b.distance(start) < EPS)
        }) {
            return Err("That edge already exists");
        }
        let a = self.vertex_at(start);
        let b = self.vertex_at(end);
        self.split_at(a);
        self.split_at(b);
        Ok(self.insert_edge(a, b))
    }
    pub fn erase(&mut self, ids: &BTreeSet<u64>) {
        self.edges.retain(|id, _| !ids.contains(id));
        let used: BTreeSet<_> = self.edges.values().flat_map(|e| [e.a, e.b]).collect();
        self.vertices.retain(|id, _| used.contains(id));
    }
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut vertices = self.vertices.values();
        let first = vertices.next()?.position;
        Some(vertices.fold((first, first), |(lo, hi), v| {
            (lo.min(v.position), hi.max(v.position))
        }))
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.vertices.len() > 1_000_000 || self.edges.len() > 1_000_000 {
            return Err("Model is too large");
        }
        let max_id = self
            .vertices
            .keys()
            .chain(self.edges.keys())
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
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub version: u32,
    pub name: String,
    pub model: Model,
    pub camera: Camera,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            name: "Untitled model".into(),
            model: Model::default(),
            camera: Camera::default(),
        }
    }
}
impl Document {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let doc: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if doc.version != 1 {
            return Err("Unsupported document version".into());
        }
        doc.model.validate().map_err(str::to_owned)?;
        if !doc.camera.valid() {
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
