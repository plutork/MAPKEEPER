use super::precipitation::is_land;
use super::*;
use crate::hex::{Axial, MapBounds};
use crate::hydro::SEA_LEVEL;
use crate::layer::{DenseLayer, DenseState, LayerValue};
use crate::worldgen::elevation::{elevation_from_land_mask_and_geology, ElevationIntensity};
use crate::worldgen::geology::{generate_geology, GeologyStyle};
use crate::worldgen::land::{generate_land_mask, LayoutClass, ShoreCharacter, LAND_MASK_LAND};

fn fixture_climate(seed: u64, style: PrecipitationStyle, w: i32, h: i32) -> ClimateLayers {
    let bounds = MapBounds::new(w, h);
    let mask = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 4);
    let geo = generate_geology(&bounds, &mask, GeologyStyle::Belts, 11);
    let elev = elevation_from_land_mask_and_geology(
        &bounds,
        &mask,
        &geo,
        seed,
        ElevationIntensity::Standard,
    );
    generate_climate_layers(&bounds, &mask, &elev, style, seed)
}

fn land_indices(mask: &DenseLayer) -> Vec<usize> {
    (0..mask.len()).filter(|&i| is_land(mask, i)).collect()
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

    let south = bounds.from_index(0).map(|c| c.r).unwrap_or(0);
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
        assert!(
            south_avg > north_avg,
            "south {south_avg} vs north {north_avg}"
        );
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
    let dist = crate::worldgen::coast::coast_distance_land_steps(&bounds, &mask);
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
    let layers = generate_climate_layers(&bounds, &mask, &elev, PrecipitationStyle::Balanced, 99);
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
