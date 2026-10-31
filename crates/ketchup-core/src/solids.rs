//! Group/component operands, BSP booleans, surface intersections, and planar output.
use crate::{
    DVec2, DVec3, Face, Model, Object, ObjectKind, Vertex,
    csg::{self, Boolean, Polygon},
    faces,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolidOperation {
    OuterShell,
    Union,
    Intersect,
    Trim,
    Split,
}
impl SolidOperation {
    pub fn label(self) -> &'static str {
        match self {
            Self::OuterShell => "Combine",
            Self::Union => "Union",
            Self::Intersect => "Intersect",
            Self::Trim => "Trim",
            Self::Split => "Split",
        }
    }
}
#[derive(Clone, Copy)]
struct Frame {
    center: DVec3,
    scale: f64,
    epsilon: f64,
}
impl Frame {
    fn new(models: &[&Model]) -> Result<Self, &'static str> {
        let mut points = models
            .iter()
            .flat_map(|m| m.vertices.values().map(|v| v.position));
        let first = points
            .next()
            .ok_or("Choose a closed solid with filled faces")?;
        let (lo, hi) = points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
        let scale = (hi - lo).max_element();
        if scale < 1e-7 {
            return Err("The solid is too small");
        }
        Ok(Self {
            center: (lo + hi) * 0.5,
            scale,
            epsilon: (1e-7 / scale).clamp(1e-12, 1e-8),
        })
    }
    fn local(self, p: DVec3) -> DVec3 {
        (p - self.center) / self.scale
    }
    fn world(self, p: DVec3) -> DVec3 {
        p * self.scale + self.center
    }
}
#[derive(Clone)]
struct Indexed {
    points: Vec<DVec3>,
    polygons: Vec<(Vec<usize>, Option<[u8; 3]>)>,
}
struct Pool {
    points: Vec<DVec3>,
    bins: BTreeMap<[i64; 3], Vec<usize>>,
    epsilon: f64,
}
impl Pool {
    fn key(&self, p: DVec3) -> [i64; 3] {
        p.to_array().map(|v| (v / self.epsilon).round() as i64)
    }
    fn add(&mut self, p: DVec3) -> usize {
        let key = self.key(p);
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    if let Some(ids) = self.bins.get(&[key[0] + x, key[1] + y, key[2] + z]) {
                        if let Some(&id) = ids
                            .iter()
                            .find(|&&id| self.points[id].distance(p) <= self.epsilon)
                        {
                            return id;
                        }
                    }
                }
            }
        }
        let id = self.points.len();
        self.points.push(p);
        self.bins.entry(key).or_default().push(id);
        id
    }
}
fn indexed(
    polygons: &[Polygon],
    extras: impl IntoIterator<Item = DVec3>,
    epsilon: f64,
) -> Result<Indexed, &'static str> {
    if polygons.len() > 12_000 {
        return Err("The result has too many surface fragments");
    }
    let mut pool = Pool {
        points: vec![],
        bins: BTreeMap::new(),
        epsilon,
    };
    let mut rings: Vec<_> = polygons
        .iter()
        .map(|p| {
            (
                p.vertices.iter().map(|p| pool.add(*p)).collect::<Vec<_>>(),
                p.color,
            )
        })
        .collect();
    for point in extras {
        pool.add(point);
    }
    if pool.points.len() > 4096 {
        return Err("These solids have too many vertices for this operation");
    }
    // Split T-junctions introduced by clipping or by collinear triangulation vertices.
    for (ring, _) in &mut rings {
        let original = ring.clone();
        ring.clear();
        for i in 0..original.len() {
            let (a, b) = (original[i], original[(i + 1) % original.len()]);
            if a == b {
                continue;
            }
            let delta = pool.points[b] - pool.points[a];
            let length = delta.length_squared();
            let mut splits = vec![(0., a)];
            for (id, p) in pool.points.iter().enumerate() {
                if id == a || id == b {
                    continue;
                }
                let t = (*p - pool.points[a]).dot(delta) / length;
                if t > 0. && t < 1. && (pool.points[a] + delta * t).distance(*p) <= epsilon {
                    splits.push((t, id));
                }
            }
            splits.sort_by(|a, b| a.0.total_cmp(&b.0));
            ring.extend(splits.into_iter().map(|(_, id)| id));
        }
        ring.dedup();
        if ring.first() == ring.last() {
            ring.pop();
        }
    }
    rings.retain(|(r, _)| r.len() >= 3);
    Ok(Indexed {
        points: pool.points,
        polygons: rings,
    })
}
fn polygons(mesh: &Indexed, epsilon: f64) -> Result<Vec<Polygon>, &'static str> {
    mesh.polygons
        .iter()
        .map(|(r, c)| {
            Polygon::new(r.iter().map(|&id| mesh.points[id]).collect(), *c, epsilon)
                .ok_or("The solid contains a collapsed face")
        })
        .collect()
}
type EdgeMap = BTreeMap<(usize, usize), Vec<(usize, bool)>>;
fn edges(mesh: &Indexed) -> EdgeMap {
    let mut edges: EdgeMap = BTreeMap::new();
    for (face, (ring, _)) in mesh.polygons.iter().enumerate() {
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            edges
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push((face, a < b));
        }
    }
    edges
}
fn components(mesh: &Indexed) -> Result<Vec<Vec<usize>>, &'static str> {
    let edges = edges(mesh);
    if edges.values().any(|v| v.len() != 2) {
        return Err(
            "Each body must be a closed solid; fill missing faces and remove internal or stray geometry",
        );
    }
    let mut adjacency = vec![vec![]; mesh.polygons.len()];
    for pair in edges.values() {
        adjacency[pair[0].0].push(pair[1].0);
        adjacency[pair[1].0].push(pair[0].0);
    }
    let mut visited = BTreeSet::new();
    let mut result = vec![];
    for seed in 0..mesh.polygons.len() {
        if visited.contains(&seed) {
            continue;
        }
        let (mut component, mut pending) = (vec![], vec![seed]);
        while let Some(id) = pending.pop() {
            if visited.insert(id) {
                component.push(id);
                pending.extend(&adjacency[id]);
            }
        }
        result.push(component);
    }
    Ok(result)
}
fn inside(point: DVec3, surfaces: &[Polygon]) -> bool {
    let mut angle = 0.;
    for p in surfaces {
        for i in 1..p.vertices.len() - 1 {
            let (a, b, c) = (
                p.vertices[0] - point,
                p.vertices[i] - point,
                p.vertices[i + 1] - point,
            );
            let denominator = a.length() * b.length() * c.length()
                + a.dot(b) * c.length()
                + b.dot(c) * a.length()
                + c.dot(a) * b.length();
            angle += 2. * a.dot(b.cross(c)).atan2(denominator);
        }
    }
    angle.abs() > std::f64::consts::TAU
}
fn import_surfaces(model: &Model, frame: Frame) -> Result<Vec<Polygon>, &'static str> {
    if model.faces.is_empty() {
        return Err("The body needs filled faces to form a closed solid");
    }
    if model.faces.len() > 4096 {
        return Err("This solid has too many faces for this operation");
    }
    let boundary: BTreeSet<_> = model
        .faces
        .values()
        .flat_map(|f| std::iter::once(&f.vertices).chain(f.holes.iter()))
        .flat_map(|r| {
            (0..r.len()).map(|i| {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                (a.min(b), a.max(b))
            })
        })
        .collect();
    if model
        .edges
        .values()
        .any(|e| !boundary.contains(&(e.a.min(e.b), e.a.max(e.b))))
    {
        return Err("Remove stray edges before using a solid tool");
    }
    let surfaces: Vec<_> = model
        .faces
        .values()
        .flat_map(|f| {
            model.face_triangles(f).into_iter().filter_map(move |t| {
                Polygon::new(
                    t.into_iter().map(|p| frame.local(p)).collect(),
                    model.face_colors.get(&f.id).copied(),
                    frame.epsilon,
                )
            })
        })
        .collect();
    if surfaces.is_empty() {
        return Err("Choose geometry with filled faces");
    }
    Ok(surfaces)
}
fn import(model: &Model, frame: Frame) -> Result<Vec<Polygon>, &'static str> {
    // Object boundaries isolate their geometry. Combine child volumes rather
    // than welding overlapping or touching children into a non-manifold mesh.
    let roots = model.root_entities();
    let mut surfaces = vec![];
    let loose: BTreeSet<_> = roots
        .iter()
        .copied()
        .filter(|id| !model.objects.contains_key(id))
        .collect();
    if !loose.is_empty() {
        surfaces = match import_mesh(&model.copy_entities(&loose), frame) {
            Ok(surfaces) => surfaces,
            Err(_) => return import_mesh(model, frame),
        };
    }
    for id in roots.iter().filter(|id| model.objects.contains_key(id)) {
        let contents = model
            .object_contents(*id)
            .ok_or("Missing object contents")?;
        let child = match import(&contents, frame) {
            Ok(child) => child,
            // Some models organize one shell's faces into separate objects.
            // Preserve support for a shell that closes only as a whole.
            Err(_) => return import_mesh(model, frame),
        };
        surfaces = csg::apply(surfaces, child, Boolean::Union, frame.epsilon)?;
    }
    if surfaces.is_empty() {
        return Err("The body needs filled faces to form a closed solid");
    }
    Ok(surfaces)
}
fn import_mesh(model: &Model, frame: Frame) -> Result<Vec<Polygon>, &'static str> {
    let surfaces = import_surfaces(model, frame)?;
    let mut mesh = indexed(
        &surfaces,
        model.vertices.values().map(|v| frame.local(v.position)),
        frame.epsilon,
    )?;
    let edge_map = edges(&mesh);
    let groups = components(&mesh)?;
    let mut adjacency = vec![vec![]; mesh.polygons.len()];
    for pair in edge_map.values() {
        let ((a, da), (b, db)) = (pair[0], pair[1]);
        adjacency[a].push((b, da == db));
        adjacency[b].push((a, da == db));
    }
    let mut flips = vec![None; mesh.polygons.len()];
    for group in &groups {
        flips[group[0]] = Some(false);
        let mut pending = vec![group[0]];
        while let Some(a) = pending.pop() {
            for &(b, opposite) in &adjacency[a] {
                let flip = flips[a].unwrap() ^ opposite;
                match flips[b] {
                    Some(previous) if previous != flip => {
                        return Err("The solid has inconsistent surface topology");
                    }
                    Some(_) => {}
                    None => {
                        flips[b] = Some(flip);
                        pending.push(b);
                    }
                }
            }
        }
    }
    for (i, (ring, _)) in mesh.polygons.iter_mut().enumerate() {
        if flips[i] == Some(true) {
            ring.reverse();
        }
    }
    let mut surfaces = polygons(&mesh, frame.epsilon)?;
    // Orient each shell first, then reverse nested shells to represent enclosed voids.
    for group in &groups {
        let volume = csg::volume(
            &group
                .iter()
                .map(|&i| surfaces[i].clone())
                .collect::<Vec<_>>(),
        );
        if volume.abs() < 1e-15 {
            return Err("The body has no enclosed volume");
        }
        if volume < 0. {
            for &i in group {
                surfaces[i].flip();
            }
        }
    }
    let mut nested = vec![false; groups.len()];
    for (i, group) in groups.iter().enumerate() {
        let point = surfaces[group[0]].vertices[0];
        let mut depth = 0;
        for (j, other) in groups.iter().enumerate() {
            if i != j
                && inside(
                    point,
                    &other
                        .iter()
                        .map(|&i| surfaces[i].clone())
                        .collect::<Vec<_>>(),
                )
            {
                depth += 1;
            }
        }
        nested[i] = depth % 2 == 1;
    }
    for (group, nested) in groups.iter().zip(nested) {
        if nested {
            for &i in group {
                surfaces[i].flip();
            }
        }
    }
    Ok(surfaces)
}

/// Give a planar surface a temporary thickness for bounded BSP clipping. Only
/// its original plane is emitted, so this never adds thickness to the model.
fn planar_prism(mut surfaces: Vec<Polygon>, epsilon: f64) -> Result<Vec<Polygon>, &'static str> {
    let first = surfaces
        .first()
        .ok_or("Choose geometry with filled faces")?;
    let (normal, w) = (first.normal, first.w);
    if surfaces.iter().any(|p| {
        p.vertices
            .iter()
            .any(|v| (normal.dot(*v) - w).abs() > epsilon)
    }) {
        return Err("Open bodies must lie in the same plane to intersect each other");
    }
    for p in &mut surfaces {
        if p.normal.dot(normal) < 0. {
            p.flip();
        }
    }
    let mesh = indexed(&surfaces, [], epsilon)?;
    let mut result = vec![];
    for p in surfaces {
        let mut bottom = p.clone();
        bottom.vertices.iter_mut().for_each(|v| *v -= normal);
        bottom.w -= 1.;
        bottom.flip();
        let mut top = p;
        top.vertices.iter_mut().for_each(|v| *v += normal);
        top.w += 1.;
        result.extend([bottom, top]);
    }
    for ((a, b), adjacent) in edges(&mesh) {
        if adjacent.len() == 1 {
            let (face, forward) = adjacent[0];
            let (a, b) = if forward { (a, b) } else { (b, a) };
            let (a, b) = (mesh.points[a], mesh.points[b]);
            if let Some(side) = Polygon::new(
                vec![a - normal, b - normal, b + normal, a + normal],
                mesh.polygons[face].1,
                epsilon,
            ) {
                result.push(side);
            }
        } else if adjacent.len() != 2 {
            return Err("Remove overlapping faces inside the planar body");
        }
    }
    Ok(result)
}

struct Surface {
    vertices: Vec<usize>,
    holes: Vec<Vec<usize>>,
    color: Option<[u8; 3]>,
}
fn merged_surfaces(mesh: &Indexed, epsilon: f64) -> Result<Vec<Surface>, &'static str> {
    let mut planes: Vec<(DVec3, f64, Option<[u8; 3]>, Vec<usize>)> = vec![];
    for (id, (ring, color)) in mesh.polygons.iter().enumerate() {
        let p = Polygon::new(
            ring.iter().map(|&i| mesh.points[i]).collect(),
            *color,
            epsilon,
        )
        .ok_or("The result contains a collapsed face")?;
        if let Some(group) = planes.iter_mut().find(|(n, w, c, _)| {
            *c == *color
                && n.dot(p.normal) > 1. - 1e-12
                && (w - p.w).abs() < epsilon * 2.
                && ring
                    .iter()
                    .all(|&i| (n.dot(mesh.points[i]) - *w).abs() < epsilon * 2.)
        }) {
            group.3.push(id);
        } else {
            planes.push((p.normal, p.w, *color, vec![id]));
        }
    }
    let mut result = vec![];
    for (normal, _, color, ids) in planes {
        let mut map: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
        for &id in &ids {
            let ring = &mesh.polygons[id].0;
            for i in 0..ring.len() {
                let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                map.entry((a.min(b), a.max(b))).or_default().push(id);
            }
        }
        let mut adjacency: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for shared in map.values() {
            if shared.len() == 2 {
                adjacency.entry(shared[0]).or_default().push(shared[1]);
                adjacency.entry(shared[1]).or_default().push(shared[0]);
            }
        }
        let mut visited = BTreeSet::new();
        for seed in ids {
            if visited.contains(&seed) {
                continue;
            }
            let (mut group, mut queue) = (BTreeSet::new(), vec![seed]);
            while let Some(id) = queue.pop() {
                if visited.insert(id) {
                    group.insert(id);
                    queue.extend(adjacency.get(&id).into_iter().flatten());
                }
            }
            let mut next: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
            for &id in &group {
                let ring = &mesh.polygons[id].0;
                for i in 0..ring.len() {
                    let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                    if map[&(a.min(b), a.max(b))]
                        .iter()
                        .filter(|id| group.contains(id))
                        .count()
                        == 1
                    {
                        next.entry(a).or_default().push(b);
                    }
                }
            }
            if next.values().any(|v| v.len() != 1) {
                result.extend(group.iter().map(|&id| Surface {
                    vertices: mesh.polygons[id].0.clone(),
                    holes: vec![],
                    color,
                }));
                continue;
            }
            let mut loops = vec![];
            while let Some((&start, _)) = next.first_key_value() {
                let (mut ring, mut current) = (vec![], start);
                loop {
                    ring.push(current);
                    let Some(neighbor) = next.remove(&current).and_then(|v| v.first().copied())
                    else {
                        return Err("Cannot join the clipped face boundaries");
                    };
                    current = neighbor;
                    if current == start {
                        break;
                    }
                    if ring.len() > mesh.points.len() {
                        return Err("The clipped boundary crosses itself");
                    }
                }
                if ring.len() >= 3 {
                    loops.push(ring);
                }
            }
            let signed_area = |r: &Vec<usize>| {
                (0..r.len())
                    .map(|i| mesh.points[r[i]].cross(mesh.points[r[(i + 1) % r.len()]]))
                    .sum::<DVec3>()
                    .dot(normal)
                    * 0.5
            };
            let (outers, holes): (Vec<_>, Vec<_>) =
                loops.into_iter().partition(|r| signed_area(r) > 0.);
            let u = normal
                .cross(if normal.z.abs() < 0.9 {
                    DVec3::Z
                } else {
                    DVec3::X
                })
                .normalize();
            let v = normal.cross(u);
            let project = |id: usize| DVec2::new(mesh.points[id].dot(u), mesh.points[id].dot(v));
            for outer in &outers {
                let polygon: Vec<_> = outer.iter().map(|&i| project(i)).collect();
                let holes = holes
                    .iter()
                    .filter(|hole| faces::in_polygon(project(hole[0]), &polygon, false))
                    .cloned()
                    .collect();
                result.push(Surface {
                    vertices: outer.clone(),
                    holes,
                    color,
                });
            }
            if outers.is_empty() {
                return Err("Cannot orient the clipped face boundaries");
            }
        }
    }
    Ok(result)
}
fn output(
    model: &mut Model,
    surfaces: Vec<Polygon>,
    frame: Frame,
    name: &str,
    outer_only: bool,
    closed: bool,
) -> Result<Option<u64>, &'static str> {
    if surfaces.is_empty() || (closed && csg::volume(&surfaces).abs() < 1e-15) {
        return Ok(None);
    }
    let mut mesh = indexed(&surfaces, [], frame.epsilon)?;
    let groups = if closed {
        components(&mesh).map_err(|_| "The result cannot form a closed solid at this precision")?
    } else {
        vec![]
    };
    if outer_only {
        let polys = polygons(&mesh, frame.epsilon)?;
        let shells: Vec<Vec<Polygon>> = groups
            .iter()
            .map(|group| group.iter().map(|&i| polys[i].clone()).collect())
            .collect();
        let retained: BTreeSet<_> = groups
            .iter()
            .enumerate()
            .filter(|(i, group)| {
                csg::volume(&shells[*i]) > 0.
                    && !shells
                        .iter()
                        .enumerate()
                        .any(|(j, other)| j != *i && inside(polys[group[0]].vertices[0], other))
            })
            .map(|(_, group)| group)
            .flatten()
            .copied()
            .collect();
        mesh.polygons = mesh
            .polygons
            .into_iter()
            .enumerate()
            .filter(|(i, _)| retained.contains(i))
            .map(|(_, p)| p)
            .collect();
    }
    let surfaces = merged_surfaces(&mesh, frame.epsilon)?;
    let (mut vertices, mut entities) = (BTreeMap::new(), BTreeSet::new());
    for surface in surfaces {
        let mut rings = vec![];
        for ring in std::iter::once(surface.vertices).chain(surface.holes) {
            let mut ids = vec![];
            for index in ring {
                let id = *vertices.entry(index).or_insert_with(|| {
                    let id = model.id();
                    model.vertices.insert(
                        id,
                        Vertex {
                            id,
                            position: frame.world(mesh.points[index]),
                        },
                    );
                    id
                });
                ids.push(id);
            }
            for i in 0..ids.len() {
                entities.insert(model.insert_edge(ids[i], ids[(i + 1) % ids.len()]));
            }
            rings.push(ids);
        }
        let id = model.id();
        let face = Face {
            id,
            vertices: rings.remove(0),
            holes: rings,
        };
        if model.face_area(&face) <= crate::document::EPS {
            return Err("The result contains details smaller than the model precision");
        }
        model.faces.insert(id, face);
        entities.insert(id);
        if let Some(color) = surface.color {
            model.face_colors.insert(id, color);
        }
    }
    let id = model.id();
    model.objects.insert(
        id,
        Object {
            id,
            surface_treatment: Default::default(),
            surface_treatment_customized: false,
            stock_material: None,
            stock_material_id: None,
            outliner_order: None,
            name: name.into(),
            kind: ObjectKind::Group,
            transform: Some(Default::default()),
            definition_entities: BTreeMap::new(),
            entities,
        },
    );
    Ok(Some(id))
}

impl Model {
    pub fn validate_solid_operand(
        &self,
        entity: u64,
        operation: SolidOperation,
    ) -> Result<(), &'static str> {
        let ids = self.solid_entities(entity)?;
        let copy = self.copy_entities(&ids);
        let frame = Frame::new(&[&copy])?;
        if operation == SolidOperation::Intersect {
            import_surfaces(&copy, frame).map(|_| ())
        } else {
            import(&copy, frame).map(|_| ())
        }
    }
    /// Resolve an object, or all connected loose geometry, as one operand.
    pub fn solid_entities(&self, entity: u64) -> Result<BTreeSet<u64>, &'static str> {
        let entity = self
            .entity_in_context(entity)
            .ok_or("Choose a body in the current editing context")?;
        if self.objects.contains_key(&entity) {
            return Ok(BTreeSet::from([entity]));
        }
        let mut vertices = BTreeSet::new();
        if let Some(edge) = self.edges.get(&entity) {
            vertices.extend([edge.a, edge.b]);
        }
        if let Some(face) = self.faces.get(&entity) {
            vertices.extend(
                face.vertices
                    .iter()
                    .chain(face.holes.iter().flatten())
                    .copied(),
            );
        }
        if vertices.is_empty() {
            return Err("Choose a closed solid group, component, or connected body");
        }
        let mut adjacency: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        for e in self
            .edges
            .values()
            .filter(|e| self.parent(e.id) == self.edit_context)
        {
            adjacency.entry(e.a).or_default().push(e.b);
            adjacency.entry(e.b).or_default().push(e.a);
        }
        // A face joins its outer boundary to every hole boundary even though
        // no modeled edge crosses the surface between those rings.
        for face in self
            .faces
            .values()
            .filter(|face| self.parent(face.id) == self.edit_context)
        {
            if let Some(&outer) = face.vertices.first() {
                for &inner in face.holes.iter().filter_map(|ring| ring.first()) {
                    adjacency.entry(outer).or_default().push(inner);
                    adjacency.entry(inner).or_default().push(outer);
                }
            }
        }
        let mut pending: VecDeque<_> = vertices.iter().copied().collect();
        while let Some(vertex) = pending.pop_front() {
            for &other in adjacency.get(&vertex).into_iter().flatten() {
                if vertices.insert(other) {
                    pending.push_back(other);
                }
            }
        }
        let mut ids: BTreeSet<_> = self
            .edges
            .values()
            .filter(|e| self.parent(e.id) == self.edit_context && vertices.contains(&e.a))
            .map(|e| e.id)
            .collect();
        ids.extend(
            self.faces
                .values()
                .filter(|f| {
                    self.parent(f.id) == self.edit_context
                        && f.vertices.iter().any(|id| vertices.contains(id))
                })
                .map(|f| f.id),
        );
        Ok(ids)
    }
    pub fn solid_volume(&self, entity: u64) -> Result<f64, &'static str> {
        let ids = self.solid_entities(entity)?;
        let copy = self.copy_entities(&ids);
        let frame = Frame::new(&[&copy])?;
        Ok(csg::volume(&import(&copy, frame)?).abs() * frame.scale.powi(3))
    }
    pub fn boolean_solids(
        &mut self,
        first: u64,
        second: u64,
        operation: SolidOperation,
    ) -> Result<BTreeSet<u64>, &'static str> {
        if self.edit_context.is_some() {
            return self.edit_in_context(|model| model.boolean_solids(first, second, operation));
        }
        let (a_ids, b_ids) = (self.solid_entities(first)?, self.solid_entities(second)?);
        if !a_ids.is_disjoint(&b_ids) {
            return Err("Choose two different solids");
        }
        if self.selection_locked(&a_ids) || self.selection_locked(&b_ids) {
            return Err("Unlock both solids before editing them");
        }
        if a_ids.iter().chain(&b_ids).any(|id| self.is_hidden(*id)) {
            return Err("Show both solids before editing them");
        }
        let (a, b) = (self.copy_entities(&a_ids), self.copy_entities(&b_ids));
        let frame = Frame::new(&[&a, &b])?;
        let (a_solid, b_solid) = (import(&a, frame), import(&b, frame));
        let closed = a_solid.is_ok() && b_solid.is_ok();
        if operation == SolidOperation::Intersect && !closed {
            let overlap = match (a_solid, b_solid) {
                (Ok(solid), Err(_)) => {
                    csg::intersect_surface(import_surfaces(&b, frame)?, solid, frame.epsilon)?
                }
                (Err(_), Ok(solid)) => {
                    csg::intersect_surface(import_surfaces(&a, frame)?, solid, frame.epsilon)?
                }
                (Err(_), Err(_)) => {
                    let surfaces = import_surfaces(&a, frame)?;
                    let other = import_surfaces(&b, frame)?;
                    let plane = surfaces.first().unwrap();
                    if other.iter().any(|p| {
                        p.vertices
                            .iter()
                            .any(|v| (plane.normal.dot(*v) - plane.w).abs() > frame.epsilon)
                    }) {
                        return Err(
                            "Open bodies must lie in the same plane to intersect each other",
                        );
                    }
                    let prism = planar_prism(other, frame.epsilon)?;
                    csg::intersect_surface(surfaces, prism, frame.epsilon)?
                }
                _ => unreachable!(),
            };
            if overlap.is_empty() {
                return Err("The bodies have no shared surface");
            }
            let mut next = self.clone();
            next.erase(&a_ids.union(&b_ids).copied().collect());
            let result = output(&mut next, overlap, frame, "Intersection", false, false)?
                .into_iter()
                .collect();
            next.validate()
                .map_err(|_| "The result cannot be represented as valid planar geometry")?;
            *self = next;
            return Ok(result);
        }
        let (a, b) = (a_solid?, b_solid?);
        let mut pieces = vec![];
        match operation {
            SolidOperation::OuterShell | SolidOperation::Union => pieces.push((
                csg::apply(a, b, Boolean::Union, frame.epsilon)?,
                operation.label(),
            )),
            SolidOperation::Intersect => {
                let overlap = csg::apply(a, b, Boolean::Intersect, frame.epsilon)?;
                if overlap.is_empty() || csg::volume(&overlap).abs() < 1e-15 {
                    return Err("The solids have no shared volume");
                }
                pieces.push((overlap, "Intersection"));
            }
            SolidOperation::Trim | SolidOperation::Split => {
                let overlap = csg::apply(a.clone(), b.clone(), Boolean::Intersect, frame.epsilon)?;
                if overlap.is_empty() || csg::volume(&overlap).abs() < 1e-15 {
                    return Err("Choose overlapping solids for this operation");
                }
                let remainder_b =
                    csg::apply(b.clone(), a.clone(), Boolean::Difference, frame.epsilon)?;
                if operation == SolidOperation::Split {
                    pieces.push((
                        csg::apply(a, b, Boolean::Difference, frame.epsilon)?,
                        "Split A",
                    ));
                    pieces.push((overlap, "Split overlap"));
                }
                pieces.push((
                    remainder_b,
                    if operation == SolidOperation::Trim {
                        "Trimmed"
                    } else {
                        "Split B"
                    },
                ));
            }
        }
        let mut next = self.clone();
        let mut removed = b_ids;
        if operation != SolidOperation::Trim {
            removed.extend(a_ids);
        }
        next.erase(&removed);
        let mut result = BTreeSet::new();
        for (polygons, name) in pieces {
            if let Some(id) = output(
                &mut next,
                polygons,
                frame,
                name,
                operation == SolidOperation::OuterShell,
                true,
            )? {
                result.insert(id);
            }
        }
        next.validate()
            .map_err(|_| "The result cannot be represented as valid planar geometry")?;
        for &id in &result {
            next.solid_volume(id)
                .map_err(|_| "The result cannot form a valid closed solid at this precision")?;
        }
        *self = next;
        Ok(result)
    }
}
