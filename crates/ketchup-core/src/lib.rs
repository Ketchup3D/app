pub mod camera;
mod csg;
pub mod display;
pub mod document;
pub mod edit_context;
pub mod faces;
pub mod inference;
pub mod measurements;
pub mod modeling;
pub mod objects;
pub mod selection;
pub mod solids;
pub mod tags;

pub use camera::{Camera, CameraView, Ray};
pub use display::{DisplaySettings, FaceStyle};
pub use document::{Document, Edge, Face, History, Model, Vertex};
pub use edit_context::InstanceTransform;
pub use glam::{DVec2, DVec3};
pub use inference::{
    Axis, AxisGuide, Inference, MoveInference, Snap, infer, infer_move_with_snapping,
    infer_with_snapping,
};
pub use measurements::{Dimension, DimensionAnchor, angle_arc, angle_degrees};
pub use objects::{Object, ObjectKind};
pub use solids::SolidOperation;
pub use tags::Tag;

#[cfg(test)]
mod edit_context_tests;
#[cfg(test)]
mod measurement_tests;
#[cfg(test)]
mod modeling_tests;
#[cfg(test)]
mod move_inference_tests;
#[cfg(test)]
mod object_tests;
#[cfg(test)]
mod reference_inference_tests;
#[cfg(test)]
mod solid_tests;
#[cfg(test)]
mod surface_tests;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Line,
    Rectangle,
    Circle,
    Polygon,
    Freehand,
    PushPull,
    Flip,
    OuterShell,
    Union,
    Intersect,
    Trim,
    Split,
    Scale,
    Offset,
    Move,
    Paint,
    TapeMeasure,
    Protractor,
    Dimensions,
    Erase,
    Orbit,
}

impl Tool {
    pub const SOLID: [Self; 6] = [
        Self::Flip,
        Self::OuterShell,
        Self::Union,
        Self::Intersect,
        Self::Trim,
        Self::Split,
    ];
    pub fn is_solid(self) -> bool {
        Self::SOLID.contains(&self)
    }

    pub const MEASURING: [Self; 3] = [Self::Dimensions, Self::TapeMeasure, Self::Protractor];
    pub fn is_measuring(self) -> bool {
        Self::MEASURING.contains(&self)
    }
    pub const DRAWING: [Self; 5] = [
        Self::Line,
        Self::Rectangle,
        Self::Circle,
        Self::Polygon,
        Self::Freehand,
    ];
    pub fn is_drawing(self) -> bool {
        Self::DRAWING.contains(&self)
    }
    pub const ALL: [Self; 22] = [
        Self::Select,
        Self::Line,
        Self::Rectangle,
        Self::Circle,
        Self::Polygon,
        Self::Freehand,
        Self::PushPull,
        Self::Flip,
        Self::OuterShell,
        Self::Union,
        Self::Intersect,
        Self::Trim,
        Self::Split,
        Self::Scale,
        Self::Offset,
        Self::Move,
        Self::Paint,
        Self::TapeMeasure,
        Self::Protractor,
        Self::Dimensions,
        Self::Erase,
        Self::Orbit,
    ];
    pub const RAIL: [Self; 11] = [
        Self::Select,
        Self::Line,
        Self::Erase,
        Self::Orbit,
        Self::PushPull,
        Self::Scale,
        Self::Offset,
        Self::Move,
        Self::Paint,
        Self::Dimensions,
        Self::Union,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Line => "Line",
            Self::Rectangle => "Rectangle",
            Self::Circle => "Circle",
            Self::Polygon => "Polygon",
            Self::Freehand => "Freehand",
            Self::PushPull => "Push/Pull",
            Self::Flip => "Flip",
            Self::OuterShell => "Outer Shell",
            Self::Union => "Union",
            Self::Intersect => "Intersect",
            Self::Trim => "Trim",
            Self::Split => "Split",
            Self::Scale => "Scale",
            Self::Offset => "Offset",
            Self::Move => "Move",
            Self::Paint => "Paint",
            Self::TapeMeasure => "Tape Measure",
            Self::Protractor => "Protractor",
            Self::Dimensions => "Dimensions",
            Self::Erase => "Erase",
            Self::Orbit => "Orbit",
        }
    }
    pub fn shortcut(self) -> &'static str {
        match self {
            Self::Select => "Space",
            Self::Line => "L",
            Self::Rectangle => "R",
            Self::Circle => "C",
            Self::Polygon => "G",
            Self::Freehand => "D",
            Self::PushPull => "P",
            Self::Flip
            | Self::OuterShell
            | Self::Union
            | Self::Intersect
            | Self::Trim
            | Self::Split => "",
            Self::Scale => "S",
            Self::Offset => "Q",
            Self::Move => "M",
            Self::Paint => "B",
            Self::TapeMeasure => "T",
            Self::Protractor => "A",
            Self::Dimensions => "I",
            Self::Erase => "E",
            Self::Orbit => "O",
        }
    }
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Select => "Click an edge or face · Drag to box select · Shift to toggle",
            Self::Line => {
                "Click to start a line · Arrow keys lock axes · Type a length for precision"
            }
            Self::Rectangle => "Click two opposite corners · Arrow keys lock the drawing plane",
            Self::Circle => {
                "Click the center, then a point on the circle · Arrow keys lock the drawing plane"
            }
            Self::Polygon => "Click the center, then a vertex · Arrow keys lock the drawing plane",
            Self::Freehand => {
                "Drag to draw · Release to finish · Arrow keys lock the drawing plane"
            }
            Self::Erase => "Click or drag across edges to erase · Ctrl / ⌘ Z to undo",
            Self::Flip => {
                "Select geometry · Choose a red, green, or blue flip plane · Arrow keys choose X / Y / Z"
            }
            Self::OuterShell => "Click two closed solids to combine their outer surfaces",
            Self::Union => "Click two closed solids to combine their volumes",
            Self::Intersect => "Click two closed solids to keep their shared volume",
            Self::Trim => "Click the cutter first, then the solid to trim · The cutter stays",
            Self::Split => {
                "Click two closed solids to keep the overlap and remaining pieces separately"
            }
            Self::PushPull => {
                "Click a face · Move along its normal · Click or type distance + Enter"
            }
            Self::Scale => {
                "Select geometry · Click a reference point · Move or type a scale factor + Enter"
            }
            Self::Offset => {
                "Click a face near its boundary · Move inward or outward · Type distance + Enter"
            }
            Self::Move => {
                "Select geometry · Click a base point and destination · Arrow keys lock axes"
            }
            Self::Paint => "Choose a color in the inspector · Click a face to paint",
            Self::TapeMeasure => "Click two points to measure their distance · Esc to clear",
            Self::Protractor => {
                "Click a center, a baseline point, then an angle point · Esc to clear"
            }
            Self::Dimensions => {
                "Click two endpoints, then click to place the dimension · Esc to cancel"
            }
            Self::Orbit => "Drag to orbit · Hold Shift to pan · Scroll to zoom",
        }
    }
}
