use serde::{Deserialize, Serialize};

/// Canonical geometric frame for persisted world coordinates (meters, N-014).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldFrame {
    pub id: String,
    /// Local origin X in meters (not a CRS).
    pub origin_x: f64,
    /// Local origin Y in meters (not a CRS).
    pub origin_y: f64,
}

impl WorldFrame {
    pub fn default_probe() -> Self {
        Self {
            id: "world".to_string(),
            origin_x: 0.0,
            origin_y: 0.0,
        }
    }
}
