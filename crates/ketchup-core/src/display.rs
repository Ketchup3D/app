use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaceStyle {
    Wireframe,
    HiddenLine,
    Monochrome,
    Shaded,
    #[default]
    Materials,
}
impl FaceStyle {
    pub const ALL: [Self; 5] = [
        Self::Wireframe,
        Self::HiddenLine,
        Self::Monochrome,
        Self::Shaded,
        Self::Materials,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Wireframe => "Wireframe",
            Self::HiddenLine => "Hidden line",
            Self::Monochrome => "Monochrome",
            Self::Shaded => "Shaded",
            Self::Materials => "Shaded with materials",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplaySettings {
    pub joinery_preview: bool,
    /// Show clickable markers at saved wood joints.
    pub wood_joinery: bool,
    pub fasteners: bool,
    pub fastener_preview: bool,
    pub fastener_face_handles: bool,
    pub units: crate::UnitSettings,
    /// Smooth only cast-shadow boundaries, preserving original geometry rendering.
    #[serde(alias = "antialiasing")]
    pub shadow_antialiasing: bool,
    pub grid: bool,
    pub grid_auto_size: bool,
    pub grid_spacing: f64,
    pub grid_infinite: bool,
    /// Side length in meters of the finite square centered on the model origin.
    pub grid_max_length: f64,
    pub axis_colors: [[u8; 3]; 3],
    pub inference_color: [u8; 3],
    pub midpoint_color: [u8; 3],
    pub axes: bool,
    pub guides: bool,
    pub dimensions: bool,
    pub hidden_geometry: bool,
    pub hidden_objects: bool,
    pub group_emphasis: bool,
    /// Show group names at their projected bounds centers.
    pub group_names: bool,
    pub face_style: FaceStyle,
    pub x_ray: bool,
    pub edges: bool,
    pub back_edges: bool,
    pub profiles: bool,
    pub profile_width: f64,
    pub depth_cue: bool,
    pub endpoints: bool,
    pub extensions: bool,
    pub color_by_tag: bool,
    pub section_planes: bool,
    pub section_cuts: bool,
    pub section_fill: bool,
    pub section_height: f64,
    pub section_axes: [bool; 3],
    pub section_offsets: [f64; 2],
    pub shadows: bool,
    pub shadow_darkness: f64,
    pub sun_angle: f64,
    pub sun_elevation: f64,
    pub fog: bool,
    pub fog_start: f64,
    pub fog_end: f64,
    pub background: [u8; 3],
    pub sky: bool,
    pub ground: bool,
    pub sky_color: [u8; 3],
    pub ground_color: [u8; 3],
    pub horizon_color: [u8; 3],
}
impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            joinery_preview: true,
            wood_joinery: true,
            fasteners: true,
            fastener_preview: false,
            fastener_face_handles: false,
            units: crate::UnitSettings::default(),
            shadow_antialiasing: true,
            grid: true,
            grid_auto_size: true,
            grid_spacing: 1.,
            grid_infinite: true,
            grid_max_length: 100.,
            axis_colors: [[192, 94, 76], [97, 145, 92], [90, 133, 176]],
            inference_color: [97, 145, 92],
            midpoint_color: [85, 150, 183],
            axes: true,
            guides: true,
            dimensions: true,
            hidden_geometry: false,
            hidden_objects: false,
            group_emphasis: true,
            group_names: false,
            face_style: FaceStyle::Materials,
            x_ray: false,
            edges: true,
            back_edges: false,
            profiles: false,
            profile_width: 3.,
            depth_cue: false,
            endpoints: false,
            extensions: false,
            color_by_tag: false,
            section_planes: false,
            section_cuts: false,
            section_fill: true,
            section_height: 1.,
            section_axes: [false, false, true],
            section_offsets: [1., 1.],
            shadows: false,
            shadow_darkness: 0.35,
            sun_angle: 135.,
            sun_elevation: 45.,
            fog: false,
            fog_start: 10.,
            fog_end: 50.,
            background: [240, 242, 234],
            sky: true,
            ground: true,
            sky_color: [153, 190, 215],
            ground_color: [218, 214, 190],
            horizon_color: [238, 240, 226],
        }
    }
}
impl DisplaySettings {
    /// Z retains the legacy height field so older documents keep their cut.
    pub fn section_limits(&self) -> [Option<f64>; 3] {
        let offsets = [
            self.section_offsets[0],
            self.section_offsets[1],
            self.section_height,
        ];
        std::array::from_fn(|axis| {
            (self.section_cuts && self.section_axes[axis]).then_some(offsets[axis])
        })
    }
    pub fn section_contains(&self, point: crate::DVec3) -> bool {
        self.section_limits()
            .iter()
            .enumerate()
            .all(|(axis, limit)| limit.is_none_or(|v| point[axis] <= v + 1e-8))
    }
    pub fn section_segment(
        &self,
        mut a: crate::DVec3,
        mut b: crate::DVec3,
    ) -> Option<(crate::DVec3, crate::DVec3)> {
        for (axis, limit) in self.section_limits().into_iter().enumerate() {
            let Some(limit) = limit else { continue };
            if a[axis] > limit && b[axis] > limit {
                return None;
            }
            if (a[axis] > limit) != (b[axis] > limit) {
                let mut cut = a.lerp(b, (limit - a[axis]) / (b[axis] - a[axis]));
                cut[axis] = limit;
                if a[axis] > limit {
                    a = cut;
                } else {
                    b = cut;
                }
            }
        }
        Some((a, b))
    }
    /// Clip a triangle into at most six vertices without per-frame allocations.
    pub fn section_triangle(&self, triangle: [crate::DVec3; 3]) -> ([crate::DVec3; 6], usize) {
        let mut polygon = [crate::DVec3::ZERO; 6];
        polygon[..3].copy_from_slice(&triangle);
        let mut len = 3;
        for (axis, limit) in self.section_limits().into_iter().enumerate() {
            let Some(limit) = limit else { continue };
            let input = polygon;
            let input_len = len;
            len = 0;
            for i in 0..input_len {
                let a = input[i];
                let b = input[(i + 1) % input_len];
                if a[axis] <= limit {
                    polygon[len] = a;
                    len += 1;
                }
                if (a[axis] <= limit) != (b[axis] <= limit) {
                    let mut cut = a.lerp(b, (limit - a[axis]) / (b[axis] - a[axis]));
                    cut[axis] = limit;
                    polygon[len] = cut;
                    len += 1;
                }
            }
        }
        (polygon, len)
    }
    pub fn valid(&self) -> bool {
        (0.000001..=1e6).contains(&self.grid_spacing)
            && (0.000001..=1e6).contains(&self.grid_max_length)
            && (1. ..=8.).contains(&self.profile_width)
            && self
                .section_offsets
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1e10)
            && self.section_height.is_finite()
            && self.section_height.abs() <= 1e10
            && (0. ..=1.).contains(&self.shadow_darkness)
            && (0. ..=360.).contains(&self.sun_angle)
            && (5. ..=85.).contains(&self.sun_elevation)
            && self.fog_start.is_finite()
            && self.fog_start >= 0.
            && self.fog_end.is_finite()
            && self.fog_end > self.fog_start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;

    #[test]
    fn grid_and_wood_joinery_default_on_and_preserve_saved_visibility() {
        for display in [
            DisplaySettings::default(),
            serde_json::from_str::<DisplaySettings>("{}").unwrap(),
        ] {
            assert!(display.grid);
            assert!(display.wood_joinery);
        }
        let saved: DisplaySettings =
            serde_json::from_str(r#"{"grid":false,"wood_joinery":false}"#).unwrap();
        assert!(!saved.grid);
        assert!(!saved.wood_joinery);
    }

    #[test]
    fn finite_grid_defaults_round_trip_and_validate_length() {
        let old: DisplaySettings =
            serde_json::from_str(r#"{"grid":true,"grid_spacing":0.5}"#).unwrap();
        assert!(old.grid_infinite);
        assert_eq!(old.grid_max_length, 100.);
        let finite = DisplaySettings {
            grid_infinite: false,
            grid_max_length: 30.48,
            ..old
        };
        assert!(finite.valid());
        assert_eq!(
            serde_json::from_str::<DisplaySettings>(&serde_json::to_string(&finite).unwrap())
                .unwrap(),
            finite
        );
        for length in [0., -1., f64::NAN, f64::INFINITY, 1e7] {
            assert!(
                !DisplaySettings {
                    grid_max_length: length,
                    ..finite.clone()
                }
                .valid()
            );
        }
    }

    #[test]
    fn sections_restore_legacy_z_and_round_trip_all_axes() {
        let old: DisplaySettings =
            serde_json::from_str(r#"{"section_cuts":true,"section_height":2.5}"#).unwrap();
        assert_eq!(old.section_limits(), [None, None, Some(2.5)]);
        let mut display = old;
        display.section_axes = [true; 3];
        display.section_offsets = [-2., 4.];
        let restored: DisplaySettings =
            serde_json::from_str(&serde_json::to_string(&display).unwrap()).unwrap();
        assert_eq!(restored, display);
        assert_eq!(restored.section_limits(), [Some(-2.), Some(4.), Some(2.5)]);
        for value in [f64::NAN, f64::INFINITY, 1e11] {
            display.section_offsets[0] = value;
            assert!(!display.valid());
        }
    }

    #[test]
    fn sections_clip_segments_and_triangles_against_every_active_axis() {
        use crate::DVec3;
        let display = DisplaySettings {
            section_cuts: true,
            section_axes: [true; 3],
            ..Default::default()
        };
        let (a, b) = display
            .section_segment(DVec3::ZERO, DVec3::splat(2.))
            .unwrap();
        assert_eq!(a, DVec3::ZERO);
        assert_eq!(b, DVec3::ONE);
        assert!(
            display
                .section_segment(DVec3::splat(2.), DVec3::splat(3.))
                .is_none()
        );
        let (polygon, len) =
            display.section_triangle([DVec3::ZERO, DVec3::new(3., 0., 2.), DVec3::new(0., 3., 2.)]);
        assert!(len >= 3);
        assert!(polygon[..len].iter().all(|&p| display.section_contains(p)));
        for axis in 0..3 {
            let mut display = display.clone();
            display.section_axes = [false; 3];
            display.section_axes[axis] = true;
            assert!(!display.section_contains(DVec3::splat(2.)));
            assert!(display.section_contains(DVec3::ZERO));
        }
    }

    #[test]
    fn older_display_settings_default_grid_and_colors_and_reject_invalid_spacing() {
        let display: DisplaySettings = serde_json::from_str("{}").unwrap();
        assert_eq!(display, DisplaySettings::default());
        assert!(display.grid_auto_size);
        for spacing in [0., -1., f64::NAN, f64::INFINITY, 1e7] {
            assert!(
                !DisplaySettings {
                    grid_spacing: spacing,
                    ..display.clone()
                }
                .valid()
            );
        }
    }

    #[test]
    fn display_settings_round_trip_default_old_documents_and_validate_ranges() {
        let mut doc = Document::default();
        doc.display.hidden_objects = true;
        doc.display.axes = false;
        doc.display.fog = true;
        doc.display.face_style = FaceStyle::Wireframe;
        assert_eq!(
            Document::from_json(&serde_json::to_string(&doc).unwrap())
                .unwrap()
                .display,
            doc.display
        );
        let mut old = serde_json::to_value(&doc).unwrap();
        old.as_object_mut().unwrap().remove("display");
        assert_eq!(
            Document::from_json(&old.to_string()).unwrap().display,
            DisplaySettings::default()
        );
        doc.display.fog_end = doc.display.fog_start;
        assert!(Document::from_json(&serde_json::to_string(&doc).unwrap()).is_err());
    }
}
