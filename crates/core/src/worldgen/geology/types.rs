//! Geology layer types and constants (D-63 / D-87).

pub use crate::layer::GEOLOGY_LAYER_ID;

pub const GEOLOGY_NONE: &str = "none";
pub const GEOLOGY_STABLE: &str = "stable";
pub const GEOLOGY_BASIN: &str = "basin";
pub const GEOLOGY_RIDGE: &str = "ridge";
pub const GEOLOGY_RIFT: &str = "rift";
pub const GEOLOGY_VOLCANIC_ARC: &str = "volcanic_arc";

/// Author-facing geology generation styles (wizard buttons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeologyStyle {
    /// Orogenic chains along convergent/transform boundaries.
    Belts,
    /// Stable interiors; restrained boundary mountains.
    Shields,
    /// Volcanic arcs at coast-adjacent convergent boundaries.
    Arcs,
    /// Tectonically constrained but varied placement.
    Random,
}

impl GeologyStyle {
    pub fn parse(raw: &str) -> GeologyStyle {
        match raw.trim().to_ascii_lowercase().as_str() {
            "shields" | "shield" => GeologyStyle::Shields,
            "arcs" | "arc" => GeologyStyle::Arcs,
            "random" => GeologyStyle::Random,
            _ => GeologyStyle::Belts,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            GeologyStyle::Belts => "belts",
            GeologyStyle::Shields => "shields",
            GeologyStyle::Arcs => "arcs",
            GeologyStyle::Random => "random",
        }
    }
}
