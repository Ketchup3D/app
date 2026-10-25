//! Bounded BSP clipping of convex, consistently oriented surface polygons.
use crate::DVec3;

#[derive(Clone, Debug)]
pub(crate) struct Polygon {
    pub vertices: Vec<DVec3>,
    pub normal: DVec3,
    pub w: f64,
    pub color: Option<[u8; 3]>,
}
impl Polygon {
    pub fn new(mut vertices: Vec<DVec3>, color: Option<[u8; 3]>, epsilon: f64) -> Option<Self> {
        vertices.dedup_by(|a, b| a.distance_squared(*b) < epsilon.powi(2) * 0.01);
        if vertices.len() > 2
            && vertices[0].distance_squared(*vertices.last().unwrap()) < epsilon.powi(2) * 0.01
        {
            vertices.pop();
        }
        let origin = *vertices.first()?;
        let cross = (1..vertices.len().saturating_sub(1))
            .map(|i| (vertices[i] - origin).cross(vertices[i + 1] - origin))
            .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))?;
        if cross.length() <= (epsilon * epsilon * 16.).max(f64::EPSILON * 16.) {
            return None;
        }
        let normal = cross.normalize();
        Some(Self {
            w: normal.dot(origin),
            vertices,
            normal,
            color,
        })
    }
    pub fn flip(&mut self) {
        self.vertices.reverse();
        self.normal = -self.normal;
        self.w = -self.w;
    }
}

#[derive(Clone, Copy)]
struct Plane {
    normal: DVec3,
    w: f64,
}
#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    polygons: Vec<Polygon>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
}
struct Budget {
    left: usize,
    epsilon: f64,
}
impl Budget {
    fn charge(&mut self, n: usize) -> Result<(), &'static str> {
        self.left = self
            .left
            .checked_sub(n)
            .ok_or("These solids are too complex for this operation")?;
        Ok(())
    }
}
impl Plane {
    fn split(
        self,
        polygon: Polygon,
        budget: &mut Budget,
    ) -> Result<(Vec<Polygon>, Vec<Polygon>, Vec<Polygon>, Vec<Polygon>), &'static str> {
        budget.charge(polygon.vertices.len())?;
        let types: Vec<_> = polygon
            .vertices
            .iter()
            .map(|p| {
                let t = self.normal.dot(*p) - self.w;
                if t < -budget.epsilon {
                    2
                } else if t > budget.epsilon {
                    1
                } else {
                    0
                }
            })
            .collect();
        let kind = types.iter().fold(0, |a, b| a | b);
        let (mut same, mut opposite, mut front, mut back) = (vec![], vec![], vec![], vec![]);
        match kind {
            0 => {
                if self.normal.dot(polygon.normal) > 0. {
                    same.push(polygon);
                } else {
                    opposite.push(polygon);
                }
            }
            1 => front.push(polygon),
            2 => back.push(polygon),
            _ => {
                let (mut f, mut b) = (vec![], vec![]);
                for i in 0..polygon.vertices.len() {
                    let j = (i + 1) % polygon.vertices.len();
                    let (a, c) = (polygon.vertices[i], polygon.vertices[j]);
                    if types[i] != 2 {
                        f.push(a);
                    }
                    if types[i] != 1 {
                        b.push(a);
                    }
                    if types[i] | types[j] == 3 {
                        let t =
                            ((self.w - self.normal.dot(a)) / self.normal.dot(c - a)).clamp(0., 1.);
                        let point = a + (c - a) * t;
                        f.push(point);
                        b.push(point);
                    }
                }
                if let Some(p) = Polygon::new(f, polygon.color, budget.epsilon) {
                    front.push(p);
                }
                if let Some(p) = Polygon::new(b, polygon.color, budget.epsilon) {
                    back.push(p);
                }
            }
        }
        Ok((same, opposite, front, back))
    }
}
impl Node {
    fn build(
        &mut self,
        polygons: Vec<Polygon>,
        budget: &mut Budget,
        depth: usize,
    ) -> Result<(), &'static str> {
        if polygons.is_empty() {
            return Ok(());
        }
        if depth > 256 {
            return Err("These solids need too many clipping planes");
        }
        if self.plane.is_none() {
            // Sampling balances the tree without a quadratic plane search.
            let best = (0..polygons.len())
                .step_by((polygons.len() / 8).max(1))
                .take(8)
                .min_by_key(|&i| {
                    let plane = &polygons[i];
                    let (mut front, mut back, mut spanning) = (0, 0, 0);
                    for p in &polygons {
                        let (f, b) = p.vertices.iter().fold((false, false), |(f, b), v| {
                            let d = plane.normal.dot(*v) - plane.w;
                            (f || d > budget.epsilon, b || d < -budget.epsilon)
                        });
                        front += usize::from(f);
                        back += usize::from(b);
                        spanning += usize::from(f && b);
                    }
                    spanning * 4 + front.abs_diff(back)
                })
                .unwrap();
            self.plane = Some(Plane {
                normal: polygons[best].normal,
                w: polygons[best].w,
            });
        }
        let (mut front, mut back) = (vec![], vec![]);
        for polygon in polygons {
            let (same, opposite, f, b) = self.plane.unwrap().split(polygon, budget)?;
            self.polygons.extend(same);
            self.polygons.extend(opposite);
            front.extend(f);
            back.extend(b);
        }
        if !front.is_empty() {
            self.front
                .get_or_insert_with(Default::default)
                .build(front, budget, depth + 1)?;
        }
        if !back.is_empty() {
            self.back
                .get_or_insert_with(Default::default)
                .build(back, budget, depth + 1)?;
        }
        Ok(())
    }
    fn invert(&mut self) {
        for p in &mut self.polygons {
            p.flip();
        }
        if let Some(p) = &mut self.plane {
            p.normal = -p.normal;
            p.w = -p.w;
        }
        if let Some(node) = &mut self.front {
            node.invert();
        }
        if let Some(node) = &mut self.back {
            node.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }
    fn clipped(
        &self,
        polygons: Vec<Polygon>,
        budget: &mut Budget,
    ) -> Result<Vec<Polygon>, &'static str> {
        self.clipped_with_boundary(polygons, budget, false)
    }
    fn clipped_with_boundary(
        &self,
        polygons: Vec<Polygon>,
        budget: &mut Budget,
        keep_boundary: bool,
    ) -> Result<Vec<Polygon>, &'static str> {
        let Some(plane) = self.plane else {
            return Ok(polygons);
        };
        let (mut front, mut back) = (vec![], vec![]);
        for polygon in polygons {
            let (same, opposite, f, b) = plane.split(polygon, budget)?;
            front.extend(same);
            front.extend(f);
            if keep_boundary {
                front.extend(opposite);
            } else {
                back.extend(opposite);
            }
            back.extend(b);
        }
        if let Some(node) = &self.front {
            front = node.clipped_with_boundary(front, budget, keep_boundary)?;
        }
        back = if let Some(node) = &self.back {
            node.clipped_with_boundary(back, budget, keep_boundary)?
        } else {
            vec![]
        };
        front.extend(back);
        Ok(front)
    }
    fn clip_to(&mut self, other: &Node, budget: &mut Budget) -> Result<(), &'static str> {
        self.polygons = other.clipped(std::mem::take(&mut self.polygons), budget)?;
        if let Some(node) = &mut self.front {
            node.clip_to(other, budget)?;
        }
        if let Some(node) = &mut self.back {
            node.clip_to(other, budget)?;
        }
        Ok(())
    }
    fn all(&self) -> Vec<Polygon> {
        let mut polygons = self.polygons.clone();
        if let Some(node) = &self.front {
            polygons.extend(node.all());
        }
        if let Some(node) = &self.back {
            polygons.extend(node.all());
        }
        polygons
    }
}
#[derive(Clone, Copy)]
pub(crate) enum Boolean {
    Union,
    Intersect,
    Difference,
}
/// Clip an open surface against a closed volume, including coplanar boundary
/// fragments regardless of the surface's winding.
pub(crate) fn intersect_surface(
    surfaces: Vec<Polygon>,
    solid: Vec<Polygon>,
    epsilon: f64,
) -> Result<Vec<Polygon>, &'static str> {
    let mut budget = Budget {
        left: 2_000_000,
        epsilon,
    };
    let mut node = Node::default();
    node.build(solid, &mut budget, 0)?;
    node.invert();
    node.clipped_with_boundary(surfaces, &mut budget, true)
}
pub(crate) fn apply(
    a: Vec<Polygon>,
    b: Vec<Polygon>,
    operation: Boolean,
    epsilon: f64,
) -> Result<Vec<Polygon>, &'static str> {
    if a.is_empty() || b.is_empty() {
        return Ok(match operation {
            Boolean::Union => a.into_iter().chain(b).collect(),
            Boolean::Intersect => vec![],
            Boolean::Difference => a,
        });
    }
    let mut budget = Budget {
        left: 2_000_000,
        epsilon,
    };
    let (mut a_node, mut b_node) = (Node::default(), Node::default());
    a_node.build(a, &mut budget, 0)?;
    b_node.build(b, &mut budget, 0)?;
    match operation {
        Boolean::Union => {
            a_node.clip_to(&b_node, &mut budget)?;
            b_node.clip_to(&a_node, &mut budget)?;
            b_node.invert();
            b_node.clip_to(&a_node, &mut budget)?;
            b_node.invert();
            a_node.build(b_node.all(), &mut budget, 0)?;
        }
        Boolean::Intersect => {
            a_node.invert();
            b_node.clip_to(&a_node, &mut budget)?;
            b_node.invert();
            a_node.clip_to(&b_node, &mut budget)?;
            b_node.clip_to(&a_node, &mut budget)?;
            a_node.build(b_node.all(), &mut budget, 0)?;
            a_node.invert();
        }
        Boolean::Difference => {
            a_node.invert();
            a_node.clip_to(&b_node, &mut budget)?;
            b_node.clip_to(&a_node, &mut budget)?;
            b_node.invert();
            b_node.clip_to(&a_node, &mut budget)?;
            b_node.invert();
            a_node.build(b_node.all(), &mut budget, 0)?;
            a_node.invert();
        }
    }
    Ok(a_node.all())
}
pub(crate) fn volume(polygons: &[Polygon]) -> f64 {
    polygons
        .iter()
        .map(|p| {
            (1..p.vertices.len() - 1)
                .map(|i| p.vertices[0].dot(p.vertices[i].cross(p.vertices[i + 1])) / 6.)
                .sum::<f64>()
        })
        .sum()
}
