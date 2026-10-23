pub mod camera;
pub mod document;
pub mod inference;
pub mod selection;

pub use camera::{Camera, Ray};
pub use document::{Document, Edge, History, Model, Vertex};
pub use glam::{DVec2, DVec3};
pub use inference::{Axis, Inference, Snap, infer};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Line,
    Erase,
    Orbit,
}

impl Tool {
    pub const ALL: [Self; 4] = [Self::Select, Self::Line, Self::Erase, Self::Orbit];
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Line => "Line",
            Self::Erase => "Erase",
            Self::Orbit => "Orbit",
        }
    }
    pub fn shortcut(self) -> &'static str {
        match self {
            Self::Select => "Space",
            Self::Line => "L",
            Self::Erase => "E",
            Self::Orbit => "O",
        }
    }
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Select => "Click an edge to select · Drag to box select · Shift to toggle",
            Self::Line => {
                "Click to start a line · Arrow keys lock axes · Type a length for precision"
            }
            Self::Erase => "Click or drag across edges to erase · Ctrl / ⌘ Z to undo",
            Self::Orbit => "Drag to orbit · Hold Shift to pan · Scroll to zoom",
        }
    }
}
