//! Integrity audit adapter + read-only HTTP API (agent-reliability integrity-checker).

use std::path::{Path, PathBuf};

use axum::response::IntoResponse;
use mapkeeper_core::climate::PRECIPITATION_LAYER_ID;
use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::integrity::{
    codes, integrity_error_summary, validate_world_integrity, IntegrityMode, IntegrityReport,
    LayerPayload, WorldIntegrityInput,
};
use mapkeeper_core::lakes::LakeCatalog;
use mapkeeper_core::layer::{
    DenseLayer, MapManifest, LAND_MASK_LAYER_ID, LAKE_ID_LAYER_ID, ELEVATION_LAYER_ID,
    RIVER_ID_LAYER_ID,
};
use mapkeeper_core::rivers::RiverCatalog;
use mapkeeper_core::worldgen::hydrology::{HydrologySnapshot, NamedRiverStore};

use crate::world_io::{
    hydrology_snapshot_path, lakes_file_path, layer_file_path, map_manifest_path,
    named_rivers_file_path, parse_manifest_id_from_str, rivers_file_path,
};
use crate::world_transaction::{EffectiveFile, WorldMutationPlan};

/// Post-commit / open-world audit (read-only; does not mutate or roll back).
pub fn audit_world_integrity(world_path: &Path) -> Result<IntegrityReport, String> {
    let input = load_integrity_input(world_path, None)?;
    Ok(validate_world_integrity(
        &input,
        IntegrityMode::PostCommit,
    ))
}

/// Pre-commit validation for staged transaction plans.
pub(crate) fn pre_commit_check(
    world_path: &Path,
    plan: Option<&WorldMutationPlan>,
) -> Result<(), String> {
    let input = load_integrity_input(world_path, plan)?;
    let mut report = validate_world_integrity(&input, IntegrityMode::PreCommit);
    if plan.is_some_and(|p| p.will_invalidate_hydrology()) {
        report.findings.retain(|f| {
            f.severity != mapkeeper_core::integrity::IntegritySeverity::Error
                || (f.code != codes::HYDROLOGY_SNAPSHOT_STALE_FINGERPRINT
                    && f.code != codes::HYDROLOGY_SNAPSHOT_STALE_REVISION)
        });
    }
    if report.has_errors() {
        return Err(integrity_error_summary(&report));
    }
    Ok(())
}

pub(crate) fn load_integrity_input(
    world_path: &Path,
    plan: Option<&WorldMutationPlan>,
) -> Result<WorldIntegrityInput, String> {
    let manifest_path = map_manifest_path(world_path);
    let toml_path = world_path.join("mapkeeper.toml");

    let mapkeeper_toml_present = match plan {
        Some(p) => !matches!(p.effective_file(&toml_path)?, EffectiveFile::Absent),
        None => toml_path.exists(),
    };
    let manifest_present = match plan {
        Some(p) => !matches!(p.effective_file(&manifest_path)?, EffectiveFile::Absent),
        None => manifest_path.exists(),
    };

    let mut input = WorldIntegrityInput {
        mapkeeper_toml_present,
        manifest_present,
        ..Default::default()
    };

    if mapkeeper_toml_present {
        if let EffectiveRead::Text(raw) = read_effective_text(world_path, &toml_path, plan) {
            if let Err(err) = parse_manifest_id_from_str(&raw) {
                input.mapkeeper_toml_parse_error = Some(err.to_string());
            }
        }
    }

    if manifest_present {
        match read_effective_text(world_path, &manifest_path, plan) {
            EffectiveRead::Absent => {}
            EffectiveRead::ParseError(err) => {
                input.manifest_parse_error = Some(err);
            }
            EffectiveRead::Text(raw) => match MapManifest::from_json(&raw) {
                Ok(manifest) => {
                    input.bounds = manifest_bounds(&manifest);
                    input.manifest = Some(manifest);
                }
                Err(err) => input.manifest_parse_error = Some(err.to_string()),
            },
        }
    }

    let bounds = input.bounds;

    load_rivers_bundle(world_path, plan, bounds, &mut input)?;
    load_lakes_bundle(world_path, plan, bounds, &mut input)?;
    load_hydrology(world_path, plan, &mut input)?;
    load_named_rivers(world_path, plan, &mut input)?;
    load_other_dense_layers(world_path, plan, bounds, &mut input)?;

    Ok(input)
}

enum EffectiveRead {
    Absent,
    Text(String),
    ParseError(String),
}

fn read_effective_text(
    _world_path: &Path,
    path: &Path,
    plan: Option<&WorldMutationPlan>,
) -> EffectiveRead {
    let bytes = match plan {
        Some(p) => match p.effective_file(path) {
            Ok(EffectiveFile::Absent) => return EffectiveRead::Absent,
            Ok(EffectiveFile::Bytes(b)) => b,
            Err(err) => return EffectiveRead::ParseError(err),
        },
        None => {
            if !path.exists() {
                return EffectiveRead::Absent;
            }
            match std::fs::read(path) {
                Ok(b) => b,
                Err(err) => return EffectiveRead::ParseError(err.to_string()),
            }
        }
    };
    match String::from_utf8(bytes) {
        Ok(text) => EffectiveRead::Text(text),
        Err(err) => EffectiveRead::ParseError(err.to_string()),
    }
}

fn manifest_bounds(manifest: &MapManifest) -> Option<MapBounds> {
    match manifest.bounds {
        mapkeeper_core::layer::Bounds::HexRectangle { width, height } => {
            Some(MapBounds::new(width, height))
        }
    }
}

fn load_rivers_bundle(
    world_path: &Path,
    plan: Option<&WorldMutationPlan>,
    bounds: Option<MapBounds>,
    input: &mut WorldIntegrityInput,
) -> Result<(), String> {
    let catalog_path = rivers_file_path(world_path);
    let layer_path = layer_file_path(world_path, RIVER_ID_LAYER_ID);
    input.rivers_catalog_present =
        !matches!(read_effective_text(world_path, &catalog_path, plan), EffectiveRead::Absent);
    input.river_id_layer_present =
        !matches!(read_effective_text(world_path, &layer_path, plan), EffectiveRead::Absent);

    match read_effective_text(world_path, &catalog_path, plan) {
        EffectiveRead::Absent => {}
        EffectiveRead::ParseError(err) => input.rivers_catalog_parse_error = Some(err),
        EffectiveRead::Text(raw) => match RiverCatalog::from_json(&raw) {
            Ok(catalog) => input.rivers_catalog = Some(catalog),
            Err(err) => input.rivers_catalog_parse_error = Some(err.to_string()),
        },
    }
    if let Some(bounds) = bounds {
        match read_effective_text(world_path, &layer_path, plan) {
            EffectiveRead::Absent => {}
            EffectiveRead::ParseError(err) => input.river_id_parse_error = Some(err),
            EffectiveRead::Text(raw) => match DenseLayer::from_json(&raw) {
                Ok(layer) => input.river_id_layer = Some(layer),
                Err(err) => input.river_id_parse_error = Some(err.to_string()),
            },
        }
        let _ = bounds;
    }
    Ok(())
}

fn load_lakes_bundle(
    world_path: &Path,
    plan: Option<&WorldMutationPlan>,
    bounds: Option<MapBounds>,
    input: &mut WorldIntegrityInput,
) -> Result<(), String> {
    let catalog_path = lakes_file_path(world_path);
    let layer_path = layer_file_path(world_path, LAKE_ID_LAYER_ID);
    input.lakes_catalog_present =
        !matches!(read_effective_text(world_path, &catalog_path, plan), EffectiveRead::Absent);
    input.lake_id_layer_present =
        !matches!(read_effective_text(world_path, &layer_path, plan), EffectiveRead::Absent);

    match read_effective_text(world_path, &catalog_path, plan) {
        EffectiveRead::Absent => {}
        EffectiveRead::ParseError(err) => input.lakes_catalog_parse_error = Some(err),
        EffectiveRead::Text(raw) => match LakeCatalog::from_json(&raw) {
            Ok(catalog) => input.lakes_catalog = Some(catalog),
            Err(err) => input.lakes_catalog_parse_error = Some(err.to_string()),
        },
    }
    if let Some(bounds) = bounds {
        match read_effective_text(world_path, &layer_path, plan) {
            EffectiveRead::Absent => {}
            EffectiveRead::ParseError(err) => input.lake_id_parse_error = Some(err),
            EffectiveRead::Text(raw) => match DenseLayer::from_json(&raw) {
                Ok(layer) => input.lake_id_layer = Some(layer),
                Err(err) => input.lake_id_parse_error = Some(err.to_string()),
            },
        }
        let _ = bounds;
    }
    Ok(())
}

fn load_hydrology(
    world_path: &Path,
    plan: Option<&WorldMutationPlan>,
    input: &mut WorldIntegrityInput,
) -> Result<(), String> {
    let path = hydrology_snapshot_path(world_path);
    input.hydrology_snapshot_present =
        !matches!(read_effective_text(world_path, &path, plan), EffectiveRead::Absent);
    match read_effective_text(world_path, &path, plan) {
        EffectiveRead::Absent => {}
        EffectiveRead::ParseError(err) => input.hydrology_snapshot_parse_error = Some(err),
        EffectiveRead::Text(raw) => match HydrologySnapshot::from_json(&raw) {
            Ok(snapshot) => input.hydrology_snapshot = Some(snapshot),
            Err(err) => input.hydrology_snapshot_parse_error = Some(err.to_string()),
        },
    }
    let (revision, fingerprint) = hydrology_base_fingerprint_merged(world_path, plan);
    input.hydrology_base_revision = Some(revision);
    input.hydrology_base_fingerprint = Some(fingerprint);
    Ok(())
}

fn load_named_rivers(
    world_path: &Path,
    plan: Option<&WorldMutationPlan>,
    input: &mut WorldIntegrityInput,
) -> Result<(), String> {
    let path = named_rivers_file_path(world_path);
    input.named_rivers_present =
        !matches!(read_effective_text(world_path, &path, plan), EffectiveRead::Absent);
    match read_effective_text(world_path, &path, plan) {
        EffectiveRead::Absent => {}
        EffectiveRead::ParseError(err) => input.named_rivers_parse_error = Some(err),
        EffectiveRead::Text(raw) => match NamedRiverStore::from_json(&raw) {
            Ok(store) => input.named_rivers = Some(store),
            Err(err) => input.named_rivers_parse_error = Some(err.to_string()),
        },
    }
    Ok(())
}

fn load_other_dense_layers(
    world_path: &Path,
    plan: Option<&WorldMutationPlan>,
    _bounds: Option<MapBounds>,
    input: &mut WorldIntegrityInput,
) -> Result<(), String> {
    let layers_dir = world_path.join("map/layers");
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&layers_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if stem == RIVER_ID_LAYER_ID || stem == LAKE_ID_LAYER_ID {
                continue;
            }
            paths.push(path);
        }
    }
    if let Some(plan) = plan {
        for path in plan.staged_active_paths() {
            if !is_layer_json_path(path) {
                continue;
            }
            if !paths.iter().any(|x| x == path) {
                paths.push(path.to_path_buf());
            }
        }
    }
    for path in paths {
        let layer_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        match read_effective_text(world_path, &path, plan) {
            EffectiveRead::Absent => {}
            EffectiveRead::ParseError(err) => input.dense_layers.push(LayerPayload {
                layer_id: layer_id.clone(),
                layer: None,
                parse_error: Some(err),
            }),
            EffectiveRead::Text(raw) => match DenseLayer::from_json(&raw) {
                Ok(layer) => input.dense_layers.push(LayerPayload {
                    layer_id,
                    layer: Some(layer),
                    parse_error: None,
                }),
                Err(err) => input.dense_layers.push(LayerPayload {
                    layer_id,
                    layer: None,
                    parse_error: Some(err.to_string()),
                }),
            },
        }
    }
    Ok(())
}

fn is_layer_json_path(path: &Path) -> bool {
    path.to_string_lossy().contains("/layers/")
        || path.to_string_lossy().contains("\\layers\\")
}

fn hydrology_base_fingerprint_merged(
    world_path: &Path,
    plan: Option<&WorldMutationPlan>,
) -> (u64, String) {
    const INPUTS: &[&str] = &[
        LAND_MASK_LAYER_ID,
        ELEVATION_LAYER_ID,
        LAKE_ID_LAYER_ID,
        PRECIPITATION_LAYER_ID,
    ];
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for layer_id in INPUTS {
        let path = layer_file_path(world_path, layer_id);
        let bytes = match plan {
            Some(p) => match p.effective_file(&path) {
                Ok(EffectiveFile::Bytes(b)) => b,
                Ok(EffectiveFile::Absent) | Err(_) => Vec::new(),
            },
            None => std::fs::read(&path).unwrap_or_default(),
        };
        for byte in layer_id.bytes().chain([0u8]).chain(bytes) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    }
    (hash, format!("{hash:016x}"))
}

pub(crate) fn routes() -> axum::Router<std::sync::Arc<crate::state::ServerState>> {
    use axum::routing::get;
    axum::Router::new().route("/api/integrity", get(get_integrity))
}

async fn get_integrity(
    axum::extract::State(server): axum::extract::State<std::sync::Arc<crate::state::ServerState>>,
    headers: axum::http::HeaderMap,
) -> impl axum::response::IntoResponse {
    let world = match crate::world_scope::resolve_world(&server.app, &headers, crate::world_scope::ScopeMode::Read) {
        Ok(world) => world,
        Err(err) => return err.into_response(),
    };
    match audit_world_integrity(&world.path) {
        Ok(report) => axum::Json(report).into_response(),
        Err(err) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, err).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mapkeeper_core::integrity::codes;
    use std::path::PathBuf;

    fn fixture_world(slug: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/worlds")
            .join(slug)
    }

    #[test]
    fn gentle_plain_fixture_has_no_integrity_errors() {
        let world = fixture_world("gentle-plain");
        let report = audit_world_integrity(&world).expect("audit");
        assert!(
            !report.has_errors(),
            "unexpected errors: {:?}",
            report.findings
        );
    }

    #[test]
    fn corrupt_river_fixture_reports_mismatch_code() {
        let world = fixture_world("integrity-river-mismatch");
        let report = audit_world_integrity(&world).expect("audit");
        assert!(report.findings.iter().any(|f| {
            f.code == codes::RIVERS_CATALOG_LAYER_MISMATCH
        }));
    }

    #[test]
    fn pre_commit_blocks_commit_on_corrupt_world() {
        use crate::world_transaction::{CommitError, WorldMutationPlan};
        use mapkeeper_core::hydro::ELEVATION_LAYER_ID;
        use mapkeeper_core::layer::{DenseLayer, DenseState, LayerValue};
        use tempfile::tempdir;

        let src = fixture_world("integrity-river-mismatch");
        let dir = tempdir().unwrap();
        let world = dir.path().join("world");
        copy_dir_all(&src, &world).unwrap();
        let bounds = MapBounds::new(14, 8);
        let mut layer = DenseLayer::new_integer(ELEVATION_LAYER_ID, bounds.len());
        for i in 0..layer.len() {
            layer.set(i, DenseState::Value(LayerValue::Int(0)));
        }
        let elevation = world.join("map/layers/elevation.json");
        let mut plan = WorldMutationPlan::begin(&world).unwrap();
        plan.stage_write(&elevation, layer.to_json_pretty().unwrap().into_bytes())
            .unwrap();
        let err = plan.commit(None).unwrap_err();
        match err {
            CommitError::Op(msg) => assert!(msg.contains(codes::RIVERS_CATALOG_LAYER_MISMATCH)),
            other => panic!("expected Op error, got {other:?}"),
        }
    }

    fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let target = dst.join(entry.file_name());
            if path.is_dir() {
                copy_dir_all(&path, &target)?;
            } else {
                std::fs::copy(&path, &target)?;
            }
        }
        Ok(())
    }
}
