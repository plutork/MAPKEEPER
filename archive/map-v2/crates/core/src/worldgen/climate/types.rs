//! Climate layer ids and author-facing types (D-90).

use crate::layer::DenseLayer;

pub const TEMPERATURE_LAYER_ID: &str = "temperature";
pub const PRECIPITATION_LAYER_ID: &str = "precipitation";
pub const ICE_LAYER_ID: &str = "ice";

/// Wizard precipitation styles (D-90).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrecipitationStyle {
    Balanced,
    WetCoasts,
    DryInterior,
}

impl PrecipitationStyle {
    pub fn parse(raw: &str) -> PrecipitationStyle {
        match raw.trim().to_ascii_lowercase().as_str() {
            "wet" | "wet_coasts" | "wetcoasts" | "coasts" => PrecipitationStyle::WetCoasts,
            "dry" | "dry_interior" | "dryinterior" | "interior" => PrecipitationStyle::DryInterior,
            _ => PrecipitationStyle::Balanced,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            PrecipitationStyle::Balanced => "balanced",
            PrecipitationStyle::WetCoasts => "wet_coasts",
            PrecipitationStyle::DryInterior => "dry_interior",
        }
    }
}

/// Internal prevailing wind (no UI in D-90); west = maritime flow left→right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindDirection {
    West,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClimateLayers {
    pub temperature: DenseLayer,
    pub precipitation: DenseLayer,
    pub ice: DenseLayer,
}
