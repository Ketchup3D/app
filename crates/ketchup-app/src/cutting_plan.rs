//! Woodworking cut lists. Geometry is already in world metres, including instances.
//! Dimensions describe finished blanks, not stock-board nesting or grain direction.
use crate::cutting_layout::{self, Settings as LayoutSettings};
use ketchup_core::{DVec3, Model};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
#[path = "cutting_plan_tests.rs"]
mod tests;

#[derive(Debug)]
pub(crate) struct CutGroup {
    /// Thickness, width, cut length in tenths of a millimetre. A short cut can
    /// be shorter than its width when aligned with other parts in its group.
    pub dimensions: [u64; 3],
    pub rectangular: bool,
    /// One named group hierarchy per piece; repeated names still count as pieces.
    pub parts: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct CuttingPlan {
    pub groups: Vec<CutGroup>,
    pub skipped: usize,
}

struct MeasuredPart {
    dimensions: [u64; 3],
    axes: [DVec3; 3],
    rectangular: bool,
}

/// Stock types are suggestions from dimensions, never material assignments.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SuggestedStock {
    Plywood(u64),
    Lumber(&'static str),
    Custom([u64; 2]),
    Review,
}

impl SuggestedStock {
    fn for_group(group: &CutGroup) -> Self {
        let [thickness, width, _] = group.dimensions;
        if !group.rectangular {
            return Self::Review;
        }
        if let Some(nominal) = nominal_stock(thickness, width) {
            return Self::Lumber(nominal);
        }
        // A thin panel can reasonably be cut from sheet goods. Narrow strips
        // remain custom stock rather than assuming every thin piece is plywood.
        const SHEET_THICKNESSES: [u64; 12] =
            [30, 40, 60, 90, 120, 150, 180, 190, 210, 220, 240, 250];
        if width >= thickness * 4 {
            if let Some(t) = SHEET_THICKNESSES
                .iter()
                .find(|t| thickness.abs_diff(**t) <= 2)
            {
                return Self::Plywood(*t);
            }
        }
        Self::Custom([thickness, width])
    }

    fn title(&self) -> String {
        match self {
            Self::Plywood(t) => format!("Suggested stock: {} plywood", metric_length(*t)),
            Self::Lumber(nominal) => format!("Suggested stock: nominal {nominal} lumber"),
            Self::Custom([t, w]) => {
                format!(
                    "Custom stock: {} x {}",
                    metric_length(*t),
                    metric_length(*w)
                )
            }
            Self::Review => "Check shape / stock manually".into(),
        }
    }
}

impl CuttingPlan {
    /// An empty selection means all visible parts in the project.
    /// Selecting a face/edge includes its connected visible geometry in that scope.
    pub fn collect(model: &Model, selected: &BTreeSet<u64>) -> Result<Self, String> {
        let requested = model.expand_entities(selected);
        let parents: BTreeMap<_, _> = model
            .objects
            .values()
            .flat_map(|object| object.entities.iter().map(move |id| (*id, object.id)))
            .collect();
        let mut scopes: BTreeMap<Option<u64>, BTreeSet<u64>> = BTreeMap::new();
        for id in model.edges.keys().chain(model.faces.keys()) {
            if model.is_hidden(*id) {
                continue;
            }
            scopes
                .entry(parents.get(id).copied())
                .or_default()
                .insert(*id);
        }
        let mut parts = Vec::new();
        let mut skipped = 0;
        for (scope, mut remaining) in scopes {
            let vertices: BTreeMap<_, _> = remaining
                .iter()
                .map(|id| (*id, entity_vertices(model, *id)))
                .collect();
            let mut adjacency: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
            for (id, points) in &vertices {
                for point in points {
                    adjacency.entry(*point).or_default().push(*id);
                }
            }
            while let Some(first) = remaining.pop_first() {
                let mut component = BTreeSet::from([first]);
                let mut pending = vec![first];
                let mut seen_vertices = BTreeSet::new();
                while let Some(id) = pending.pop() {
                    for vertex in &vertices[&id] {
                        if seen_vertices.insert(*vertex) {
                            for adjacent in &adjacency[vertex] {
                                if remaining.remove(adjacent) {
                                    component.insert(*adjacent);
                                    pending.push(*adjacent);
                                }
                            }
                        }
                    }
                }
                let included = selected.is_empty() || !component.is_disjoint(&requested);
                let Some(part) = measure(model, &component) else {
                    if included {
                        skipped += 1;
                    }
                    continue;
                };
                let mut path = Vec::new();
                let mut parent = scope;
                while let Some(id) = parent {
                    let name = model.objects[&id].name.trim();
                    if !automatic_group_name(name) && !name.is_empty() {
                        path.push(name.to_owned());
                    }
                    parent = parents.get(&id).copied();
                }
                path.reverse();
                parts.push((path.join("/"), part, included));
            }
        }
        // Infer a common cutting direction from the longest visible blank in
        // each named group and thickness. Include unselected parts so selecting
        // just a short cut does not change its width/length assignment.
        let mut references: BTreeMap<_, &MeasuredPart> = BTreeMap::new();
        for (path, part, _) in &parts {
            if path.is_empty() || !part.rectangular {
                continue;
            }
            let reference = references
                .entry((path.as_str(), part.dimensions[0]))
                .or_insert(part);
            if part.dimensions[2] > reference.dimensions[2] {
                *reference = part;
            }
        }
        let mut groups: BTreeMap<([u64; 3], bool), Vec<String>> = BTreeMap::new();
        for (path, part, included) in &parts {
            if !included {
                continue;
            }
            let mut dimensions = part.dimensions;
            if part.rectangular {
                if let Some(reference) = references.get(&(path.as_str(), dimensions[0])) {
                    if part.axes[0].dot(reference.axes[0]).abs() > 1. - 1e-8
                        && part.axes[1].dot(reference.axes[2]).abs() > 1. - 1e-8
                    {
                        dimensions.swap(1, 2);
                    }
                }
            }
            groups
                .entry((dimensions, part.rectangular))
                .or_default()
                .push(path.clone());
        }
        if groups.is_empty() {
            return Err("No three-dimensional parts found in visible geometry. Model each board as a separate group/component or disconnected solid. Select a board or assembly, or use the whole project.".into());
        }
        Ok(Self {
            groups: groups
                .into_iter()
                .map(|((dimensions, rectangular), parts)| CutGroup {
                    dimensions,
                    rectangular,
                    parts,
                })
                .collect(),
            skipped,
        })
    }

    #[cfg(test)]
    pub fn pdf(&self, name: &str, selection: bool, group_stock: bool) -> Vec<u8> {
        self.pdf_with_layout(name, selection, group_stock, LayoutSettings::default())
    }

    pub fn pdf_with_layout(
        &self,
        name: &str,
        selection: bool,
        group_stock: bool,
        settings: LayoutSettings,
    ) -> Vec<u8> {
        let count: usize = self.groups.iter().map(|g| g.parts.len()).sum();
        let mut pdf = Pdf::new(
            name,
            if selection {
                "Selected parts (visible only)"
            } else {
                ""
            },
        );
        pdf.heading("CUT LIST", 18.);
        pdf.paragraph(
            &format!(
                "{count} pieces  |  {} sizes  |  Dimensions in mm or cm",
                self.groups.len()
            ),
            10.,
        );
        if self.skipped > 0 {
            pdf.paragraph(
                &format!(
                    "Omitted: {} flat or line-only connected items (no usable thickness).",
                    self.skipped
                ),
                9.,
            );
        }
        pdf.gap(8.);
        let mut groups: Vec<_> = self.groups.iter().collect();
        groups.sort_by(|a, b| {
            group_names(a)
                .cmp(&group_names(b))
                .then_with(|| b.parts.len().cmp(&a.parts.len()))
                .then_with(|| b.dimensions[2].cmp(&a.dimensions[2]))
        });
        if group_stock {
            let mut stock_groups: BTreeMap<SuggestedStock, Vec<(usize, &CutGroup)>> =
                BTreeMap::new();
            for (index, group) in groups.into_iter().enumerate() {
                stock_groups
                    .entry(SuggestedStock::for_group(group))
                    .or_default()
                    .push((index, group));
            }
            for (stock, groups) in stock_groups {
                pdf.ensure(170.);
                let title = stock.title();
                pdf.heading(&title, 11.);
                let quantity: usize = groups.iter().map(|(_, g)| g.parts.len()).sum();
                let pieces = format!("{quantity} piece{}", if quantity == 1 { "" } else { "s" });
                let cuts: Vec<_> = groups
                    .iter()
                    .map(|(mark, g)| cutting_layout::Piece {
                        mark: *mark,
                        length: g.dimensions[2],
                        width: g.dimensions[1],
                        quantity: g.parts.len(),
                    })
                    .collect();
                let layout = match &stock {
                    SuggestedStock::Plywood(_) => Some(cutting_layout::sheets(&cuts, settings)),
                    SuggestedStock::Lumber(_) | SuggestedStock::Custom(_) => {
                        Some(cutting_layout::boards(&cuts, settings))
                    }
                    SuggestedStock::Review => None,
                };
                let sheet_stock = matches!(&stock, SuggestedStock::Plywood(_));
                let summary = match &stock {
                    SuggestedStock::Plywood(_) => {
                        let area: f64 = groups
                            .iter()
                            .map(|(_, g)| {
                                g.dimensions[1] as f64
                                    * g.dimensions[2] as f64
                                    * g.parts.len() as f64
                                    / 100_000_000.
                            })
                            .sum();
                        format!("{pieces} | Net cut area: {area:.2} m2")
                    }
                    SuggestedStock::Lumber(_) | SuggestedStock::Custom(_) => {
                        let length: f64 = groups
                            .iter()
                            .map(|(_, g)| g.dimensions[2] as f64 * g.parts.len() as f64 / 10_000.)
                            .sum();
                        format!("{pieces} | Total cut length: {length:.2} m")
                    }
                    SuggestedStock::Review => {
                        format!("{pieces} | Review blank dimensions")
                    }
                };
                pdf.paragraph(&summary, 9.);
                if let Some(layout) = &layout {
                    pdf.paragraph(
                        &format!(
                            "Stock required: {} {} | Saw kerf: {}",
                            layout.stock.len(),
                            match (sheet_stock, layout.stock.len()) {
                                (true, 1) => "sheet",
                                (true, _) => "sheets",
                                (false, 1) => "board",
                                (false, _) => "boards",
                            },
                            metric_length(cutting_layout::tenths(settings.kerf_mm))
                        ),
                        9.,
                    );
                }
                if layout.as_ref().is_some_and(|l| !l.oversized.is_empty()) {
                    pdf.paragraph("Stock count covers fitting cuts only; oversized cuts are listed separately.", 9.);
                }
                pdf.table_header();
                for &(index, group) in &groups {
                    if pdf.y < 115. {
                        pdf.new_page();
                        pdf.heading(&format!("{title} (continued)"), 10.);
                        pdf.table_header();
                    }
                    cut_row(&mut pdf, index, group, true);
                }
                if let Some(layout) = &layout {
                    draw_layout(&mut pdf, &title, layout, sheet_stock, settings, &groups);
                }
                pdf.gap(12.);
            }
        } else {
            pdf.table_header();
            for (index, group) in groups.into_iter().enumerate() {
                if pdf.y < 115. {
                    pdf.new_page();
                    pdf.table_header();
                }
                cut_row(&mut pdf, index, group, false);
            }
        }
        pdf.finish()
    }
}

fn automatic_group_name(name: &str) -> bool {
    name.strip_prefix("Group")
        .and_then(|suffix| suffix.trim_start().strip_prefix('#'))
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit()))
}

fn group_names(group: &CutGroup) -> BTreeSet<&str> {
    group
        .parts
        .iter()
        .map(|name| if name.is_empty() { "-" } else { name.as_str() })
        .collect()
}

fn group_lines(group: &CutGroup) -> Vec<String> {
    let mut lines = Vec::new();
    for name in group_names(group) {
        let characters: Vec<_> = name
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        // A fixed-width font keeps long paths inside the Group column.
        for chunk in characters.chunks(32) {
            lines.push(chunk.iter().collect());
        }
    }
    if lines.is_empty() {
        lines.push("-".into());
    }
    lines
}

fn cut_row(pdf: &mut Pdf, index: usize, group: &CutGroup, colored: bool) {
    let [t, w, l] = group.dimensions;
    let lines = group_lines(group);
    let mut offset = 0;
    while offset < lines.len() {
        let continued = offset > 0;
        if continued {
            pdf.new_page();
            pdf.table_header();
        }
        if colored {
            pdf.rectangle(42., pdf.y - 1., 7., 7., mark_color(index));
            pdf.cell(53., 41., &format!("P{:03}", index + 1));
        } else {
            pdf.cell(42., 52., &format!("P{:03}", index + 1));
        }
        if !continued {
            pdf.cell(286., 34., &group.parts.len().to_string());
            pdf.cell(
                330.,
                132.,
                &format!("{} x {}", metric_length(t), metric_length(w)),
            );
            pdf.cell(474., 79., &metric_length(l));
        } else {
            pdf.text(100., pdf.y, 8., "(continued)");
            pdf.y -= 12.;
        }
        let available = ((pdf.y - 100.) / 12.).floor().max(1.) as usize;
        let count = available.min(lines.len() - offset);
        for (row, line) in lines[offset..offset + count].iter().enumerate() {
            pdf.text(100., pdf.y - row as f64 * 12., 9., line);
        }
        pdf.y -= (count as f64 * 12.).max(14.);
        if !continued {
            if let Some(stock) = nominal_stock(t, w) {
                pdf.text(330., pdf.y, 8., &format!("Approx. {stock} nominal"));
                pdf.y -= 12.;
            }
            if !group.rectangular {
                pdf.text(100., pdf.y, 8., "CHECK SHAPE - bounding blank only");
                pdf.y -= 12.;
            }
        }
        pdf.separator();
        offset += count;
    }
}

/// Golden-angle hues are deterministic across stock groups and pages.
fn mark_color(mark: usize) -> [f64; 3] {
    let hue = (mark as f64 * 0.618033988749895).fract() * 6.;
    let fraction = hue.fract();
    let low = 0.42;
    let high = 0.92;
    let rising = low + (high - low) * fraction;
    let falling = high - (high - low) * fraction;
    match hue.floor() as u32 {
        0 => [high, rising, low],
        1 => [falling, high, low],
        2 => [low, high, rising],
        3 => [low, falling, high],
        4 => [rising, low, high],
        _ => [high, low, falling],
    }
}

fn draw_layout(
    pdf: &mut Pdf,
    title: &str,
    layout: &cutting_layout::Layout,
    sheet: bool,
    settings: LayoutSettings,
    groups: &[(usize, &CutGroup)],
) {
    if !layout.oversized.is_empty() {
        pdf.ensure(70.);
        pdf.heading("DOES NOT FIT SELECTED STOCK", 10.);
        for (mark, quantity) in layout.oversized_marks() {
            let group = groups.iter().find(|(index, _)| *index == mark).unwrap().1;
            pdf.paragraph(
                &format!(
                    "P{:03}: {} piece{}, {} x {} - choose larger stock",
                    mark + 1,
                    quantity,
                    if quantity == 1 { "" } else { "s" },
                    metric_length(group.dimensions[1]),
                    metric_length(group.dimensions[2])
                ),
                9.,
            );
        }
    }
    for (number, stock) in layout.stock.iter().enumerate() {
        let scale = (511. / stock.length as f64).min(320. / stock.width as f64);
        let width = if sheet {
            stock.length as f64 * scale
        } else {
            511.
        };
        let height = if sheet {
            stock.width as f64 * scale
        } else {
            48.
        };
        pdf.ensure(height + 115.);
        pdf.heading(
            &format!(
                "{} {} of {}",
                if sheet { "SHEET" } else { "BOARD" },
                number + 1,
                layout.stock.len()
            ),
            12.,
        );
        pdf.paragraph(title, 9.);
        pdf.paragraph(
            &format!(
                "Stock: {} x {} | Kerf: {} | Gray: offcut / kerf",
                metric_length(stock.length),
                metric_length(stock.width),
                metric_length(cutting_layout::tenths(settings.kerf_mm))
            ),
            8.,
        );
        let top = pdf.y;
        pdf.rectangle(42., top - height, width, height, [0.94; 3]);
        let x_scale = width / stock.length as f64;
        let y_scale = height / stock.width as f64;
        for placement in &stock.placements {
            let r = placement.rect;
            let x = 42. + r.x as f64 * x_scale;
            let y = top - (r.y + r.width) as f64 * y_scale;
            let w = r.length as f64 * x_scale;
            let h = r.width as f64 * y_scale;
            pdf.rectangle(x, y, w, h, mark_color(placement.mark));
            let label = format!("P{:03}", placement.mark + 1);
            let size = (w / (label.len() as f64 * 0.6 + 1.)).min(9.);
            if size >= 5. && h >= size + 4. {
                pdf.text(
                    x + (w - label.len() as f64 * size * 0.6) * 0.5,
                    y + h * 0.5,
                    size,
                    &label,
                );
                let dimensions = if sheet {
                    format!("{} x {}", metric_length(r.length), metric_length(r.width))
                } else {
                    metric_length(r.length)
                };
                if h >= 30. && dimensions.len() as f64 * 4.2 + 4. <= w {
                    pdf.text(
                        x + (w - dimensions.len() as f64 * 4.2) * 0.5,
                        y + h * 0.5 - 12.,
                        7.,
                        &dimensions,
                    );
                }
            }
        }
        pdf.y = top - height - 20.;
        let mut legend = BTreeMap::new();
        for placement in &stock.placements {
            let entry = legend.entry(placement.mark).or_insert((0_usize, false));
            entry.0 += 1;
            entry.1 |= placement.rotated;
        }
        for (mark, (quantity, rotated)) in legend {
            pdf.ensure(22.);
            let group = groups.iter().find(|(index, _)| *index == mark).unwrap().1;
            pdf.rectangle(42., pdf.y - 1., 7., 7., mark_color(mark));
            pdf.text(
                55.,
                pdf.y,
                9.,
                &format!(
                    "P{:03} x{} - {} x {}{}",
                    mark + 1,
                    quantity,
                    metric_length(group.dimensions[1]),
                    metric_length(group.dimensions[2]),
                    if rotated { " (rotation used)" } else { "" }
                ),
            );
            pdf.y -= 17.;
        }
        pdf.gap(12.);
    }
}

fn entity_vertices(model: &Model, id: u64) -> BTreeSet<u64> {
    if let Some(edge) = model.edges.get(&id) {
        BTreeSet::from([edge.a, edge.b])
    } else if let Some(face) = model.faces.get(&id) {
        face.vertices
            .iter()
            .chain(face.holes.iter().flatten())
            .copied()
            .collect()
    } else {
        BTreeSet::new()
    }
}

fn measure(model: &Model, entities: &BTreeSet<u64>) -> Option<MeasuredPart> {
    if !entities.iter().any(|id| model.faces.contains_key(id)) {
        return None;
    }
    let vertices: BTreeSet<_> = entities
        .iter()
        .flat_map(|id| entity_vertices(model, *id))
        .collect();
    let origin = model.vertices.get(vertices.first()?)?.position;
    // Work relative to a nearby origin to preserve precision far from world zero.
    let points: Vec<_> = vertices
        .iter()
        .map(|id| model.vertices[id].position - origin)
        .collect();
    let mut directions: Vec<(DVec3, f64)> = Vec::new();
    let mut add_direction = |a: u64, b: u64| {
        let delta = model.vertices[&b].position - model.vertices[&a].position;
        let Some(direction) = delta.try_normalize() else {
            return;
        };
        if let Some((_, weight)) = directions
            .iter_mut()
            .find(|(d, _)| d.dot(direction).abs() > 1. - 1e-8)
        {
            *weight += delta.length();
        } else if directions.len() < 256 {
            directions.push((direction, delta.length()));
        }
    };
    for id in entities {
        if let Some(edge) = model.edges.get(id) {
            add_direction(edge.a, edge.b);
        }
        if let Some(face) = model.faces.get(id) {
            for ring in std::iter::once(&face.vertices).chain(face.holes.iter()) {
                for i in 0..ring.len() {
                    add_direction(ring[i], ring[(i + 1) % ring.len()]);
                }
            }
        }
    }
    directions.sort_by(|a, b| b.1.total_cmp(&a.1));
    directions.truncate(32);
    let mut basis = [DVec3::X, DVec3::Y, DVec3::Z];
    let mut bounds = projected_bounds(&points, basis);
    let mut volume = (bounds.1 - bounds.0).element_product();
    for (u, _) in &directions {
        for (v, _) in &directions {
            let cross = u.cross(*v);
            if cross.length_squared() < 1e-12 {
                continue;
            }
            let normal = cross.normalize();
            let axes = [*u, normal.cross(*u), normal];
            let candidate = projected_bounds(&points, axes);
            let candidate_volume = (candidate.1 - candidate.0).element_product();
            if candidate_volume < volume {
                basis = axes;
                bounds = candidate;
                volume = candidate_volume;
            }
        }
    }
    let sizes = (bounds.1 - bounds.0).to_array();
    if sizes.iter().any(|v| !v.is_finite() || *v < 0.00005) {
        return None;
    }
    let mut order = [0, 1, 2];
    order.sort_by(|a, b| sizes[*a].total_cmp(&sizes[*b]));
    let dimensions = order.map(|axis| (sizes[axis] * 10_000.).round() as u64);
    let rectangular = is_box(model, entities, origin, basis, bounds);
    Some(MeasuredPart {
        dimensions,
        axes: order.map(|axis| basis[axis]),
        rectangular,
    })
}

fn project(p: DVec3, axes: [DVec3; 3]) -> DVec3 {
    DVec3::new(p.dot(axes[0]), p.dot(axes[1]), p.dot(axes[2]))
}
fn projected_bounds(points: &[DVec3], axes: [DVec3; 3]) -> (DVec3, DVec3) {
    points.iter().fold(
        (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| {
            let p = project(*p, axes);
            (lo.min(p), hi.max(p))
        },
    )
}

fn is_box(
    model: &Model,
    entities: &BTreeSet<u64>,
    origin: DVec3,
    axes: [DVec3; 3],
    (lo, hi): (DVec3, DVec3),
) -> bool {
    let mut sides = BTreeSet::new();
    let mut boundaries: BTreeMap<(u64, u64), usize> = BTreeMap::new();
    for id in entities {
        let Some(face) = model.faces.get(id) else {
            continue;
        };
        if !face.holes.is_empty() || face.vertices.len() < 3 {
            return false;
        }
        let points: Vec<_> = face
            .vertices
            .iter()
            .map(|id| project(model.vertices[id].position - origin, axes))
            .collect();
        let side = (0..3)
            .flat_map(|axis| [(axis, false), (axis, true)])
            .find(|(axis, high)| {
                let coordinate = if *high { hi[*axis] } else { lo[*axis] };
                points.iter().all(|p| (p[*axis] - coordinate).abs() < 1e-7)
            });
        let Some(side) = side else { return false };
        sides.insert(side);
        for i in 0..face.vertices.len() {
            let a = face.vertices[i];
            let b = face.vertices[(i + 1) % face.vertices.len()];
            *boundaries.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    sides.len() == 6 && boundaries.values().all(|count| *count == 2)
}

fn metric_length(tenths: u64) -> String {
    if tenths > 1000 {
        let value = format!("{}.{:02}", tenths / 100, tenths % 100);
        return format!("{} cm", value.trim_end_matches('0').trim_end_matches('.'));
    }
    if tenths.is_multiple_of(10) {
        format!("{} mm", tenths / 10)
    } else {
        format!("{}.{:01} mm", tenths / 10, tenths % 10)
    }
}

fn nominal_stock(thickness: u64, width: u64) -> Option<&'static str> {
    // Rounded metric S4S sizes plus exact inch equivalents where rounding differs.
    // AWC: https://awc.org/faq/how-do-i-convert-to-metric-lumber-sizes/
    const STOCK: &[(u64, u64, &str)] = &[
        (190, 635, "1x3"),
        (190, 640, "1x3"),
        (190, 890, "1x4"),
        (190, 1400, "1x6"),
        (380, 380, "2x2"),
        (380, 635, "2x3"),
        (380, 640, "2x3"),
        (380, 890, "2x4"),
        (380, 1400, "2x6"),
        (380, 1840, "2x8"),
        (380, 2350, "2x10"),
        (380, 2860, "2x12"),
        (635, 635, "3x3"),
        (640, 640, "3x3"),
        (635, 890, "3x4"),
        (640, 890, "3x4"),
        (890, 890, "4x4"),
        (1400, 1840, "6x8"),
    ];
    STOCK
        .iter()
        .find(|(t, w, _)| thickness.abs_diff(*t) <= 2 && width.abs_diff(*w) <= 2)
        .map(|(_, _, label)| *label)
}

/// Small dependency-free PDF 1.4 writer. Courier keeps wrapping and table widths
/// deterministic on desktop and WASM. Strings use escaped WinAnsi bytes.
struct Pdf {
    pages: Vec<String>,
    page: String,
    y: f64,
    name: String,
    scope: String,
}
impl Pdf {
    fn new(name: &str, scope: &str) -> Self {
        let mut pdf = Self {
            pages: Vec::new(),
            page: String::new(),
            y: 0.,
            name: name.into(),
            scope: scope.into(),
        };
        pdf.new_page();
        pdf
    }
    fn new_page(&mut self) {
        if !self.page.is_empty() {
            self.pages.push(std::mem::take(&mut self.page));
        }
        self.y = 800.;
        let name: String = self.name.chars().take(84).collect();
        self.text(42., self.y, 10., &name);
        if !self.scope.is_empty() {
            self.y -= 20.;
            self.text(42., self.y, 9., &self.scope.clone());
        }
        self.y -= 24.;
        self.line(self.y);
        self.y -= 28.;
    }
    fn ensure(&mut self, height: f64) {
        if self.y - height < 48. {
            self.new_page();
        }
    }
    fn gap(&mut self, height: f64) {
        self.y -= height;
    }
    fn heading(&mut self, value: &str, size: f64) {
        self.ensure(size + 20.);
        self.text(42., self.y, size, value);
        self.y -= size + 12.;
    }
    fn paragraph(&mut self, value: &str, size: f64) {
        // Hard wrapping also bounds long object names without spaces.
        let width = (511. / (size * 0.6)) as usize;
        let normalized: String = value
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let chars: Vec<_> = normalized.chars().collect();
        let mut start = 0;
        while start < chars.len() {
            let mut end = (start + width).min(chars.len());
            if end < chars.len() {
                if let Some(space) = chars[start..end].iter().rposition(|c| *c == ' ') {
                    end = start + space + 1;
                }
            }
            self.ensure(size + 6.);
            self.text(
                42.,
                self.y,
                size,
                chars[start..end].iter().collect::<String>().trim_end(),
            );
            self.y -= size + 4.;
            start = end;
        }
        self.y -= 4.;
    }
    fn cell(&mut self, x: f64, width: f64, text: &str) {
        let size = (width / (text.chars().count().max(1) as f64 * 0.6)).min(10.);
        self.text(x, self.y, size, text);
    }
    fn text(&mut self, x: f64, y: f64, size: f64, text: &str) {
        use std::fmt::Write;
        let _ = writeln!(
            self.page,
            "0.12 0.14 0.15 rg BT /F1 {size} Tf {x:.2} {y:.2} Td ({}) Tj ET",
            pdf_string(text)
        );
    }
    fn rectangle(&mut self, x: f64, y: f64, width: f64, height: f64, color: [f64; 3]) {
        use std::fmt::Write;
        let [r, g, b] = color;
        let _ = writeln!(
            self.page,
            "q {r:.8} {g:.8} {b:.8} rg 0.35 G 0.4 w {x:.3} {y:.3} {width:.3} {height:.3} re B Q"
        );
    }
    fn line(&mut self, y: f64) {
        use std::fmt::Write;
        let _ = writeln!(self.page, "0.78 G 0.5 w 42 {y:.2} m 553 {y:.2} l S");
    }
    fn separator(&mut self) {
        self.gap(6.);
        self.line(self.y);
        self.gap(20.);
    }
    fn table_header(&mut self) {
        self.ensure(130.);
        self.gap(20.);
        for (x, title) in [
            (42., "MARK"),
            (100., "GROUP"),
            (286., "QTY"),
            (330., "SECTION (T x W)"),
            (474., "CUT LENGTH"),
        ] {
            self.text(x, self.y, 9., title);
        }
        self.y -= 12.;
        self.separator();
    }
    fn finish(mut self) -> Vec<u8> {
        self.pages.push(self.page);
        let count = self.pages.len();
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!(
                "<< /Type /Pages /Count {count} /Kids [{}] >>",
                (0..count)
                    .map(|i| format!("{} 0 R", 4 + i * 2))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>"
                .to_owned(),
        ];
        for (i, page) in self.pages.iter_mut().enumerate() {
            use std::fmt::Write;
            let _ = writeln!(
                page,
                "0.4 g BT /F1 8 Tf 42 28 Td (Finished sizes - verify before cutting) Tj ET BT /F1 8 Tf 463 28 Td (Page {} / {count}) Tj ET",
                i + 1
            );
            objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", 5 + i * 2));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{page}endstream",
                page.len()
            ));
        }
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        bytes
    }
}

fn pdf_string(text: &str) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for c in text.chars() {
        let byte = match c {
            ' '..='~' | '\u{a0}'..='\u{ff}' => c as u8,
            '\u{2013}' | '\u{2014}' => b'-',
            '\u{2018}' | '\u{2019}' => b'\'',
            '\u{201c}' | '\u{201d}' => b'"',
            _ if c.is_control() => b' ',
            _ => b'?',
        };
        if byte == b'(' || byte == b')' || byte == b'\\' || !byte.is_ascii() {
            let _ = write!(out, "\\{byte:03o}");
        } else {
            out.push(byte as char);
        }
    }
    out
}
