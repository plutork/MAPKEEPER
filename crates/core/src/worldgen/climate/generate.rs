//! Climate T2 zonal heuristic entry point (D-90).

use crate::hex::MapBounds;
use crate::hydro::SEA_LEVEL;
use crate::layer::{DenseLayer, DenseState, LayerValue};
use crate::worldgen::coast::coast_distance_land_steps;

use super::ice::ice_cover;
use super::precipitation::{is_land, land_precipitation, upwind_elevation_west};
use super::temperature::land_temperature;
use super::types::{
    ClimateLayers, PrecipitationStyle, WindDirection, ICE_LAYER_ID, PRECIPITATION_LAYER_ID,
    TEMPERATURE_LAYER_ID,
};

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
