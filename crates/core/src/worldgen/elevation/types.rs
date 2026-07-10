//! Wizard step-5 relief intensity (D-89).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationIntensity {
    /// D-88 defaults: tight geology bands + light smooth.
    Standard,
    /// Wider bands, looser smooth; class median order preserved.
    Bold,
    /// Land-wide random with weak geology bias.
    Chaos,
}

impl ElevationIntensity {
    pub fn parse(raw: &str) -> ElevationIntensity {
        match raw.trim().to_ascii_lowercase().as_str() {
            "bold" | "strong" | "enhanced" => ElevationIntensity::Bold,
            "chaos" | "wild" => ElevationIntensity::Chaos,
            _ => ElevationIntensity::Standard,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            ElevationIntensity::Standard => "standard",
            ElevationIntensity::Bold => "bold",
            ElevationIntensity::Chaos => "chaos",
        }
    }
}
