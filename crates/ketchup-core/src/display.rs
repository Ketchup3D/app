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
    pub grid: bool,
    pub axes: bool,
    pub guides: bool,
    pub dimensions: bool,
    pub hidden_geometry: bool,
    pub hidden_objects: bool,
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
            grid: false,
            axes: true,
            guides: true,
            dimensions: true,
            hidden_geometry: false,
            hidden_objects: false,
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
    pub fn valid(&self) -> bool {
        (1. ..=8.).contains(&self.profile_width)
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
