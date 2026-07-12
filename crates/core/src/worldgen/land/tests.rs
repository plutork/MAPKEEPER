use super::enforce::enforce_layout_class;
use super::growth::prune_thin_corridors;
use super::*;
use super::util::{half_extent, is_land_cell, land_components};
use crate::hex::{Axial, MapBounds};
use crate::layer::{DenseLayer, DenseState, LayerValue};

fn count_kind(layer: &DenseLayer, kind: &str) -> usize {
        (0..layer.len())
            .filter(|&i| {
                matches!(
                    layer.state(i),
                    DenseState::Value(LayerValue::Text(ref t)) if t == kind
                )
            })
            .count()
    }

    #[test]
    fn catalog_has_five_per_class() {
        assert_eq!(RECIPE_CATALOG.len(), 30);
        for class in LayoutClass::ALL {
            assert_eq!(recipes_for(class).len(), 5, "{}", class.id());
        }
    }

    #[test]
    fn next_recipe_changes_within_class() {
        let a = pick_recipe(LayoutClass::Island, 0);
        let b = next_recipe(LayoutClass::Island, a.id, 1);
        assert_eq!(a.layout_class, b.layout_class);
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn compare_trio_has_distinct_classes() {
        for seed in [0u64, 1, 7, 42, 99, 1000] {
            let trio = pick_compare_trio(seed);
            assert_ne!(trio[0].layout_class, trio[1].layout_class);
            assert_ne!(trio[1].layout_class, trio[2].layout_class);
            assert_ne!(trio[0].layout_class, trio[2].layout_class);
        }
    }

    #[test]
    fn regenerating_changes_recipe_or_class_set() {
        let a = pick_compare_trio(0);
        let b = pick_compare_trio(1);
        let same = a[0].id == b[0].id && a[1].id == b[1].id && a[2].id == b[2].id;
        assert!(!same, "different nonce should reshuffle trio");
    }

    #[test]
    fn recipes_in_class_differ_macroform() {
        let bounds = MapBounds::new(28, 16);
        let recipes = recipes_for(LayoutClass::Pangea);
        let layers: Vec<_> = recipes
            .iter()
            .map(|r| generate_land_mask_recipe(&bounds, r, ShoreCharacter::Smooth, 0))
            .collect();
        let mut differ = false;
        'outer: for i in 0..layers.len() {
            for j in (i + 1)..layers.len() {
                for idx in 0..bounds.len() {
                    if layers[i].state(idx) != layers[j].state(idx) {
                        differ = true;
                        break 'outer;
                    }
                }
            }
        }
        assert!(differ, "pangea growth plans should not be identical masks");
    }

    #[test]
    fn different_seeds_change_form() {
        let bounds = MapBounds::new(28, 16);
        let recipe = find_recipe("island_irregular").expect("recipe");
        let a = generate_land_mask_recipe(&bounds, recipe, ShoreCharacter::Smooth, 1);
        let b = generate_land_mask_recipe(&bounds, recipe, ShoreCharacter::Smooth, 99);
        let mut differ = false;
        for idx in 0..bounds.len() {
            if a.state(idx) != b.state(idx) {
                differ = true;
                break;
            }
        }
        assert!(differ, "seed should change organic form");
    }

    #[test]
    fn prune_removes_opposite_corridor_cell() {
        let bounds = MapBounds::new(8, 6);
        let mut layer = DenseLayer::new_categorical(LAND_MASK_LAYER_ID, bounds.len());
        for index in 0..bounds.len() {
            layer.set(
                index,
                DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
            );
        }
        // Three-cell diagonal corridor: middle has exactly two opposite land neighbors.
        let a = Axial { q: 0, r: 0 };
        let b = Axial { q: 1, r: 0 };
        let c = Axial { q: 2, r: 0 };
        for cell in [a, b, c] {
            let i = bounds.index_of(cell).expect("in bounds");
            layer.set(
                i,
                DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
            );
        }
        prune_thin_corridors(&bounds, &mut layer, 4);
        let mid = bounds.index_of(b).unwrap();
        assert!(
            !is_land_cell(&layer, mid),
            "1-hex-wide corridor middle should be pruned"
        );
    }

    #[test]
    fn generate_keeps_bounds_length() {
        let bounds = MapBounds::new(14, 8);
        let layer = generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Smooth, 42);
        assert_eq!(layer.len(), bounds.len());
        assert_eq!(layer.layer_id, LAND_MASK_LAYER_ID);
    }

    #[test]
    fn land_mask_syncs_to_elevation() {
        let bounds = MapBounds::new(4, 3);
        let mut mask = DenseLayer::new_categorical(LAND_MASK_LAYER_ID, bounds.len());
        mask.set(
            0,
            DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
        );
        mask.set(
            1,
            DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
        );
        let elev = elevation_from_land_mask(&bounds, &mask);
        assert_eq!(elev.int_or(0, 0), 1);
        assert_eq!(elev.int_or(1, 1), 0);
    }

    #[test]
    fn normalizes_unknown_kind_to_ocean() {
        assert_eq!(normalize_kind("land"), LAND_MASK_LAND);
        assert_eq!(normalize_kind("inland_sea"), LAND_MASK_INLAND_SEA);
        assert_eq!(normalize_kind("mystery"), LAND_MASK_OCEAN);
    }

    #[test]
    fn all_layout_classes_produce_land() {
        let bounds = MapBounds::new(24, 14);
        for class in LayoutClass::ALL {
            let layer = generate_land_mask(&bounds, class, ShoreCharacter::Smooth, 7);
            assert!(
                count_kind(&layer, LAND_MASK_LAND) > 0,
                "{} should produce land",
                class.id()
            );
        }
    }

    #[test]
    fn mediterranean_marks_inland_sea() {
        let bounds = MapBounds::new(28, 16);
        let recipe = find_recipe("med_c_basin").expect("recipe");
        let layer = generate_land_mask_recipe(&bounds, recipe, ShoreCharacter::Smooth, 11);
        assert!(
            count_kind(&layer, LAND_MASK_INLAND_SEA) > 0,
            "mediterranean should enclose inland_sea"
        );
    }

    #[test]
    fn continent_and_islands_has_separated_land() {
        let bounds = MapBounds::new(36, 20);
        let recipe = find_recipe("cai_irregular_main").expect("recipe");
        let layer = generate_land_mask_recipe(&bounds, recipe, ShoreCharacter::Smooth, 19);
        let land = count_kind(&layer, LAND_MASK_LAND);
        assert!(
            land > 40,
            "main continent should be substantial, got {land}"
        );
        let mut left_land = false;
        for index in 0..bounds.len() {
            let Some(cell) = bounds.from_index(index) else {
                continue;
            };
            let (x, _) = cell.to_pixel(1.0);
            let (max_x, _) = half_extent(&bounds);
            if x / max_x > -0.55 {
                continue;
            }
            if matches!(
                layer.state(index),
                DenseState::Value(LayerValue::Text(ref t)) if t == LAND_MASK_LAND
            ) {
                left_land = true;
                break;
            }
        }
        assert!(left_land, "expected satellite land on the far side");
    }

    #[test]
    fn pangea_is_single_landmass() {
        let bounds = MapBounds::new(28, 16);
        for seed in [0u64, 3, 11, 42, 99] {
            let layer =
                generate_land_mask(&bounds, LayoutClass::Pangea, ShoreCharacter::Jagged, seed);
            let comps = land_components(&bounds, &layer);
            assert_eq!(
                comps.len(),
                1,
                "pangea seed {seed} should be one mass, got {}",
                comps.len()
            );
        }
    }

    #[test]
    fn enforce_keeps_largest_only_for_pangea() {
        let bounds = MapBounds::new(10, 8);
        let mut layer = DenseLayer::new_categorical(LAND_MASK_LAYER_ID, bounds.len());
        for index in 0..bounds.len() {
            layer.set(
                index,
                DenseState::Value(LayerValue::Text(LAND_MASK_OCEAN.to_string())),
            );
        }
        // Two separate blobs.
        for cell in [
            Axial { q: -2, r: 0 },
            Axial { q: -1, r: 0 },
            Axial { q: 0, r: 0 },
        ] {
            let i = bounds.index_of(cell).expect("in bounds");
            layer.set(
                i,
                DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
            );
        }
        for cell in [Axial { q: 3, r: 1 }, Axial { q: 4, r: 1 }] {
            let i = bounds.index_of(cell).expect("in bounds");
            layer.set(
                i,
                DenseState::Value(LayerValue::Text(LAND_MASK_LAND.to_string())),
            );
        }
        enforce_layout_class(&bounds, &mut layer, LayoutClass::Pangea, 0);
        assert_eq!(land_components(&bounds, &layer).len(), 1);
    }

    #[test]
    fn continents_masses_are_comparable() {
        let bounds = MapBounds::new(32, 18);
        for seed in [0u64, 5, 17, 42, 88] {
            let layer = generate_land_mask(
                &bounds,
                LayoutClass::Continents,
                ShoreCharacter::Jagged,
                seed,
            );
            let comps = land_components(&bounds, &layer);
            assert!(
                comps.len() >= 2,
                "continents seed {seed} should have ≥2 masses, got {}",
                comps.len()
            );
            let ratio = comps[1].len() as f64 / comps[0].len().max(1) as f64;
            assert!(
                ratio >= 0.40,
                "continents seed {seed}: second/first={ratio:.2} ({} vs {})",
                comps[1].len(),
                comps[0].len()
            );
            // No half-map flood: largest mass must stay under ~55% of map cells.
            let frac = comps[0].len() as f64 / bounds.len() as f64;
            assert!(
                frac < 0.55,
                "continents seed {seed}: largest mass fills {frac:.2} of map"
            );
        }
    }

    /// Dogfood: erode-before-grow crushed Large Continents to ~1% land (seed from UI).
    #[test]
    fn continents_l_and_blob_keeps_land_fraction_on_large() {
        use crate::map_preset::MapPreset;
        let bounds = MapPreset::Large.bounds();
        let recipe = find_recipe("continents_l_and_blob").expect("recipe");
        let seed = 0xec3d502bf050082au64;
        let layer = generate_land_mask_recipe(&bounds, recipe, ShoreCharacter::Smooth, seed);
        let land = count_kind(&layer, LAND_MASK_LAND);
        let frac = land as f64 / bounds.len() as f64;
        assert!(
            frac >= 0.25,
            "expected substantial Continents land, got {land} ({frac:.3})"
        );
        let comps = land_components(&bounds, &layer);
        assert!(comps.len() >= 2, "expected ≥2 masses, got {}", comps.len());
        let ratio = comps[1].len() as f64 / comps[0].len().max(1) as f64;
        assert!(
            ratio >= 0.35,
            "second/first={ratio:.2} ({} vs {})",
            comps[1].len(),
            comps[0].len()
        );
    }

    /// Dogfood: archipelago_twin_groups was two continent blobs (seed from UI).
    #[test]
    fn archipelago_twin_groups_has_many_islands() {
        use crate::map_preset::MapPreset;
        let bounds = MapPreset::Large.bounds();
        let recipe = find_recipe("archipelago_twin_groups").expect("recipe");
        let seed = 0xf1e019cdfda6e1c0u64;
        let layer = generate_land_mask_recipe(&bounds, recipe, ShoreCharacter::Smooth, seed);
        let comps = land_components(&bounds, &layer);
        assert!(
            comps.len() >= 6,
            "archipelago should have many islands, got {} sizes={:?}",
            comps.len(),
            comps.iter().map(|c| c.len()).collect::<Vec<_>>()
        );
        let max_ok = ((bounds.len() as f64) * 0.10).round() as usize;
        assert!(
            comps[0].len() <= max_ok,
            "largest island {} exceeds ~10% map ({})",
            comps[0].len(),
            max_ok
        );
    }
