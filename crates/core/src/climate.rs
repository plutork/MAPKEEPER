//! Climate T2 zonal heuristic (D-90 climate-t2--zonal-heuristic).

use crate::coast_distance::coast_distance_land_steps;
use crate::hex::{Axial, MapBounds};
use crate::hydro::SEA_LEVEL;
use crate::land_mask::LAND_MASK_LAND;
use crate::layer::{DenseLayer, DenseState, LayerValue};
use crate::plates::hash01;

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
enum WindDirection {
    West,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClimateLayers {
    pub temperature: DenseLayer,
    pub precipitation: DenseLayer,
    pub ice: DenseLayer,
}

/// Generate temperature, precipitation, and ice from land_mask + elevation.
pub fn generate_climate_layers(
    bounds: &MapBounds,
    land_mask: &DenseLayer,
    elevation: &DenseLayer,
    style: PrecipitationStyle,
    seed: u64,
) -> ClimateLayers {
    let n = bounds.len();
    let coast_dist = coast_distance_land_steps(bounds, land_mask);
    let _wind = WindDirection::West;

    let mut temperature = DenseLayer::new_integer(TEMPERATURE_LAYER_ID, n);
    let mut precipitation = DenseLayer::new_integer(PRECIPITATION_LAYER_ID, n);
    let mut ice = DenseLayer::new_integer(ICE_LAYER_ID, n);

    let height = bounds.height.max(1) as f64;

    for index in 0..n {
        let land = is_land(land_mask, index);
        let elev = elevation.int_or(index, 0);
        let coast = coast_dist[index];

        if !land || elev <= SEA_LEVEL {
            temperature.set(index, DenseState::Value(LayerValue::Int(12)));
            precipitation.set(index, DenseState::Value(LayerValue::Int(0)));
            ice.set(index, DenseState::Value(LayerValue::Int(0)));
            continue;
        }

        let Some(cell) = bounds.from_index(index) else {
            continue;
        };

        let temp = land_temperature(cell, elev, coast, height);
        temperature.set(index, DenseState::Value(LayerValue::Int(temp)));

        let upwind_elev = upwind_elevation_west(bounds, land_mask, elevation, cell);
        let precip = land_precipitation(cell, elev, coast, upwind_elev, style, seed);
        precipitation.set(index, DenseState::Value(LayerValue::Int(precip)));

        let ice_val = ice_cover(temp, elev);
        ice.set(index, DenseState::Value(LayerValue::Int(ice_val)));
    }

    ClimateLayers {
        temperature,
        precipitation,
        ice,
    }
}

fn is_land(land_mask: &DenseLayer, index: usize) -> bool {
    matches!(
        land_mask.state(index),
        DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
    )
}

fn land_temperature(cell: Axial, elevation: i32, coast_dist: u32, height: f64) -> i32 {
    let r_norm = cell.r as f64 / (height - 1.0).max(1.0);
    let lat = 30.0 - r_norm * 52.0;
    let lapse = -(elevation.max(1) as f64) * 0.38;
    let coast_mod = (8.0 - coast_dist.min(8) as f64) * 1.8;
    (lat + lapse + coast_mod).round() as i32
}

fn upwind_elevation_west(
    bounds: &MapBounds,
    land_mask: &DenseLayer,
    elevation: &DenseLayer,
    cell: Axial,
) -> i32 {
    let up = Axial::new(cell.q - 1, cell.r);
    let Some(ui) = bounds.index_of(up) else {
        return elevation.int_or(0, 0);
    };
    if is_land(land_mask, ui) {
        elevation.int_or(ui, 0)
    } else {
        0
    }
}

fn land_precipitation(
    cell: Axial,
    elevation: i32,
    coast_dist: u32,
    upwind_elev: i32,
    style: PrecipitationStyle,
    seed: u64,
) -> i32 {
    let coast_base = 118.0 / (1.0 + coast_dist as f64 * 0.32);
    let orographic = if elevation > upwind_elev + 10 {
        22.0
    } else {
        0.0
    };
    let rain_shadow = if upwind_elev > elevation + 14 {
        -38.0
    } else {
        0.0
    };
    let mut value = coast_base + orographic + rain_shadow;

    let interior = coast_dist >= 5;
    value *= match style {
        PrecipitationStyle::Balanced => {
            if interior {
                0.92
            } else {
                1.0
            }
        }
        PrecipitationStyle::WetCoasts => {
            if interior {
                0.82
            } else {
                1.35
            }
        }
        PrecipitationStyle::DryInterior => {
            if interior {
                0.52
            } else {
                0.95
            }
        }
    };

    let jitter = (hash01(seed ^ 0xC1AA_7E, cell.q, cell.r) - 0.5) * 14.0;
    value += jitter;
    value.round().clamp(1.0, 220.0) as i32
}

fn ice_cover(temperature: i32, elevation: i32) -> i32 {
    if temperature <= -12 && elevation >= 35 {
        return 100;
    }
    if temperature <= -4 && elevation >= 55 {
        return 80;
    }
    if temperature <= 0 && elevation >= 70 {
        return 60;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevation_gen::{elevation_from_land_mask_and_geology, ElevationIntensity};
    use crate::geology::{generate_geology, GeologyStyle};
    use crate::land_mask::{generate_land_mask, LayoutClass, ShoreCharacter};

    fn fixture_climate(seed: u64, style: PrecipitationStyle, w: i32, h: i32) -> ClimateLayers {
        let bounds = MapBounds::new(w, h);
        let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 4);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 11);
        let elev =
            elevation_from_land_mask_and_geology(&bounds, &mask, &geo, seed, ElevationIntensity::Standard);
        generate_climate_layers(&bounds, &mask, &elev, style, seed)
    }

    fn land_indices(mask: &DenseLayer) -> Vec<usize> {
        (0..mask.len())
            .filter(|&i| is_land(mask, i))
            .collect()
    }

    #[test]
    fn climate_is_deterministic() {
        let a = fixture_climate(9, PrecipitationStyle::Balanced, 24, 14);
        let b = fixture_climate(9, PrecipitationStyle::Balanced, 24, 14);
        assert_eq!(a.temperature, b.temperature);
        assert_eq!(a.precipitation, b.precipitation);
        assert_eq!(a.ice, b.ice);
    }

    #[test]
    fn temperature_colder_toward_north_and_high_elevation() {
        let bounds = MapBounds::new(32, 20);
        let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 3);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 5);
        let elev =
            elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 3, ElevationIntensity::Standard);
        let layers = generate_climate_layers(&bounds, &mask, &elev, PrecipitationStyle::Balanced, 3);

        let south = bounds
            .from_index(0)
            .map(|c| c.r)
            .unwrap_or(0);
        let mut south_temps = Vec::new();
        let mut north_temps = Vec::new();
        let mut high_temps = Vec::new();
        let mut low_temps = Vec::new();

        for i in land_indices(&mask) {
            let t = layers.temperature.int_or(i, 0);
            let Some(cell) = bounds.from_index(i) else {
                continue;
            };
            let z = elev.int_or(i, 0);
            if cell.r >= bounds.height - 2 {
                south_temps.push(t);
            }
            if cell.r <= 1 {
                north_temps.push(t);
            }
            if z >= 55 {
                high_temps.push(t);
            }
            if z <= 20 {
                low_temps.push(t);
            }
        }

        if !south_temps.is_empty() && !north_temps.is_empty() {
            let south_avg = south_temps.iter().sum::<i32>() as f64 / south_temps.len() as f64;
            let north_avg = north_temps.iter().sum::<i32>() as f64 / north_temps.len() as f64;
            assert!(south_avg > north_avg, "south {south_avg} vs north {north_avg}");
        }
        if !high_temps.is_empty() && !low_temps.is_empty() {
            let hi = high_temps.iter().sum::<i32>() as f64 / high_temps.len() as f64;
            let lo = low_temps.iter().sum::<i32>() as f64 / low_temps.len() as f64;
            assert!(lo > hi, "low elev {lo} vs high elev {hi}");
        }
        let _ = south;
    }

    #[test]
    fn coast_moderation_warms_coastal_land() {
        let bounds = MapBounds::new(28, 16);
        let mask = generate_land_mask(&bounds, LayoutClass::Island, ShoreCharacter::Smooth, 2);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Shields, 2);
        let elev =
            elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 2, ElevationIntensity::Standard);
        let dist = coast_distance_land_steps(&bounds, &mask);
        let layers = generate_climate_layers(&bounds, &mask, &elev, PrecipitationStyle::Balanced, 2);

        let mut coastal = Vec::new();
        let mut interior = Vec::new();
        for i in land_indices(&mask) {
            if dist[i] <= 2 {
                coastal.push(layers.temperature.int_or(i, 0));
            } else if dist[i] >= 5 {
                interior.push(layers.temperature.int_or(i, 0));
            }
        }
        if !coastal.is_empty() && !interior.is_empty() {
            let c = coastal.iter().sum::<i32>() as f64 / coastal.len() as f64;
            let inner = interior.iter().sum::<i32>() as f64 / interior.len() as f64;
            assert!(c > inner, "coastal {c} vs interior {inner}");
        }
    }

    #[test]
    fn precipitation_differs_by_style() {
        let balanced = fixture_climate(7, PrecipitationStyle::Balanced, 40, 24);
        let wet = fixture_climate(7, PrecipitationStyle::WetCoasts, 40, 24);
        let dry = fixture_climate(7, PrecipitationStyle::DryInterior, 40, 24);
        let mut diff_wet = 0usize;
        let mut diff_dry = 0usize;
        for i in 0..balanced.precipitation.len() {
            if balanced.precipitation.int_or(i, -1) != wet.precipitation.int_or(i, -1) {
                diff_wet += 1;
            }
            if balanced.precipitation.int_or(i, -1) != dry.precipitation.int_or(i, -1) {
                diff_dry += 1;
            }
        }
        assert!(diff_wet > 10);
        assert!(diff_dry > 10);
    }

    #[test]
    fn rain_shadow_reduces_leeward_precip() {
        let bounds = MapBounds::new(24, 14);
        let mut mask = DenseLayer::new_categorical("land_mask", bounds.len());
        let mut elev = DenseLayer::new_integer("elevation", bounds.len());
        for i in 0..bounds.len() {
            mask.set(
                i,
                DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
            );
            elev.set(i, DenseState::Value(LayerValue::Int(25)));
        }
        let mut windward = None;
        let mut ridge = None;
        let mut leeward = None;
        for cell in bounds.cells() {
            let up = Axial::new(cell.q - 1, cell.r);
            let down = Axial::new(cell.q + 1, cell.r);
            if bounds.index_of(up).is_some() && bounds.index_of(down).is_some() {
                windward = bounds.index_of(up);
                ridge = bounds.index_of(cell);
                leeward = bounds.index_of(down);
                break;
            }
        }
        let (windward, ridge, leeward) = (
            windward.expect("windward"),
            ridge.expect("ridge"),
            leeward.expect("leeward"),
        );
        elev.set(ridge, DenseState::Value(LayerValue::Int(75)));
        let layers = generate_climate_layers(
            &bounds,
            &mask,
            &elev,
            PrecipitationStyle::Balanced,
            99,
        );
        assert!(
            layers.precipitation.int_or(windward, 0) > layers.precipitation.int_or(leeward, 0),
            "windward should be wetter than leeward"
        );
    }

    #[test]
    fn precipitation_not_flat_on_medium_map() {
        let layers = fixture_climate(21, PrecipitationStyle::Balanced, 52, 29);
        let mut values = std::collections::BTreeSet::new();
        for i in 0..layers.precipitation.len() {
            let v = layers.precipitation.int_or(i, 0);
            if v > 0 {
                values.insert(v);
            }
        }
        assert!(values.len() > 8, "expected varied precipitation values");
    }

    #[test]
    fn water_cells_have_zero_land_precip() {
        let bounds = MapBounds::new(20, 12);
        let mask = generate_land_mask(&bounds, LayoutClass::Island, ShoreCharacter::Smooth, 1);
        let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 1);
        let elev =
            elevation_from_land_mask_and_geology(&bounds, &mask, &geo, 1, ElevationIntensity::Standard);
        let layers = generate_climate_layers(&bounds, &mask, &elev, PrecipitationStyle::Balanced, 1);
        for i in 0..bounds.len() {
            if !is_land(&mask, i) {
                assert_eq!(layers.precipitation.int_or(i, -1), 0);
            } else if elev.int_or(i, 0) > SEA_LEVEL {
                assert!(layers.temperature.int_or(i, 0) > -60);
                assert!(layers.precipitation.int_or(i, 0) >= 1);
            }
        }
    }
}
