//! Scale factors come from the same world-space targets as Move.
use super::*;

impl KetchupApp {
    // A tiny reference amplifies pointer rounding and snap changes. Measure
    // usable handles in pixels so sensitivity stays consistent at every zoom.
    const MIN_SCALE_REFERENCE_PIXELS: f64 = 24.;

    fn scale_reference(edit: &Modeling, camera: &Camera, size: DVec2) -> Option<DVec3> {
        let reference = edit.anchor - edit.center;
        let reference = edit
            .scale_constraint
            .map_or(reference, |lock| lock.project(reference));
        let start = camera.project(edit.anchor, size)?;
        let center = camera.project(edit.anchor - reference, size)?;
        (reference.length_squared() > 1e-12
            && start.distance(center) >= Self::MIN_SCALE_REFERENCE_PIXELS)
            .then_some(reference)
    }

    pub(super) fn scale_snap(
        edit: &Modeling,
        mut snap: Snap,
        grid_spacing: Option<f64>,
        snap_to_axes: bool,
    ) -> Snap {
        if let Some(spacing) = grid_spacing {
            if snap.kind == Inference::Plane
                && let Some(AxisConstraint::Axis(axis)) = edit.scale_constraint
            {
                snap.kind = Inference::Axis(axis);
            }
            grid::snap_with_geometry(
                snap,
                spacing,
                match edit.scale_constraint {
                    Some(AxisConstraint::Plane(normal)) => Some(normal.vector()),
                    _ => None,
                },
                grid::movement_axis(
                    snap.point,
                    Some(edit.anchor),
                    edit.scale_constraint,
                    snap_to_axes,
                ),
                &edit.base,
                Some(&edit.snap_excluded),
            )
        } else {
            snap
        }
    }

    pub(super) fn scale_value(
        edit: &Modeling,
        camera: &Camera,
        size: DVec2,
        snap_to_grid: bool,
    ) -> f64 {
        if edit.scale_constraint.is_none()
            && let Some(value) = Self::unlocked_scale_value(edit, camera, size, snap_to_grid)
        {
            return value;
        }
        let project = |delta| {
            edit.scale_constraint
                .map_or(delta, |lock| lock.project(delta))
        };
        if let Some(reference) = Self::scale_reference(edit, camera, size) {
            let target = reference + project(edit.direction);
            return if matches!(edit.scale_constraint, Some(AxisConstraint::Axis(_))) {
                target.dot(reference) / reference.length_squared()
            } else {
                target.length() / reference.length()
            };
        }

        // Center clicks, small selections, and foreshortened locked axes use
        // a visible selection extent instead of dividing by a tiny reference.
        // The snapped displacement still supplies the movement and direction.
        let extent = edit
            .base
            .selection_vertices(&edit.entities)
            .into_iter()
            .fold(DVec3::ZERO, |extent, id| {
                extent.max(project(edit.base.vertices[&id].position - edit.center).abs())
            });
        let Some(start) = camera.project(edit.anchor, size) else {
            return 1.;
        };
        let free = !snap_to_grid && edit.snap.is_some_and(|snap| {
            snap.kind == Inference::Plane
                || matches!(snap.kind, Inference::Axis(axis)
                    if edit.scale_constraint == Some(AxisConstraint::Axis(axis)))
        });
        let handle = [DVec3::X, DVec3::Y, DVec3::Z]
            .into_iter()
            .filter_map(|axis| {
                let world_handle = axis * extent.dot(axis);
                let endpoint = camera.project(edit.anchor + world_handle, size)?;
                let handle = endpoint - start;
                (handle.length_squared() > 1e-12).then_some((world_handle, handle))
            })
            .max_by(|(_, a), (_, b)| a.length_squared().total_cmp(&b.length_squared()));
        if let Some((world_handle, handle)) = handle {
            if !free {
                // Exact targets and grid steps keep their world-space size.
                1. + project(edit.direction).dot(world_handle) / world_handle.length_squared()
            } else {
                // A snapped face center may differ from the initial click.
                // Measure from the click so free scaling begins at exactly 1x.
                1. + dv(edit.pointer - edit.pixel).dot(handle.normalize())
                    / handle.length().max(Self::MIN_SCALE_REFERENCE_PIXELS)
            }
        } else {
            1.
        }
    }

    fn unlocked_scale_value(
        edit: &Modeling,
        camera: &Camera,
        size: DVec2,
        snap_to_grid: bool,
    ) -> Option<f64> {
        let start = camera.project(edit.anchor, size)?;
        let center = camera.project(edit.center, size)?;
        let mut reference = start - center;
        let plane = edit.snap.is_some_and(|snap| snap.kind == Inference::Plane);
        if !plane && reference.length() >= Self::MIN_SCALE_REFERENCE_PIXELS {
            // Exact geometry and automatic axis targets retain world-space ratios.
            return None;
        }
        let movement = if plane && !snap_to_grid {
            dv(edit.pointer - edit.pixel)
        } else {
            camera.project(edit.anchor + edit.direction, size)? - start
        };
        if reference.length() < Self::MIN_SCALE_REFERENCE_PIXELS {
            // A center click has no radial direction. Use a stable screen-space
            // handle sized to the selection, without choosing a model axis.
            let radius = edit
                .base
                .selection_vertices(&edit.entities)
                .into_iter()
                .filter_map(|id| camera.project(edit.base.vertices[&id].position, size))
                .map(|point| point.distance(center))
                .fold(Self::MIN_SCALE_REFERENCE_PIXELS, f64::max);
            reference = DVec2::new(1., -1.).normalize() * radius;
        }
        // Distance responds to movement around the center in every direction;
        // projecting onto the original reference line would create an implicit lock.
        Some((reference + movement).length() / reference.length())
    }
}
