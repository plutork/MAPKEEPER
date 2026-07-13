//! World integrity validation — pure, no I/O (agent-reliability integrity-checker).

use serde::{Deserialize, Serialize};

use crate::hex::MapBounds;
use crate::lakes::{sync_lake_id_layer, validate_catalog, LakeCatalog};
use crate::layer::{DenseLayer, MapManifest, RIVER_ID_LAYER_ID, LAKE_ID_LAYER_ID};
use crate::rivers::{sync_river_id_layer, RiverCatalog};
use crate::worldgen::hydrology::{
    HydrologySnapshot, HydrologySnapshotError, NamedRiverStore,
};

/// Stable machine-readable finding codes.
pub mod codes {
    pub const WORLD_MAPKEEPER_TOML_MISSING: &str = "world.mapkeeper_toml_missing";
    pub const WORLD_MAPKEEPER_TOML_PARSE: &str = "world.mapkeeper_toml_parse_error";
    pub const WORLD_MANIFEST_MISSING: &str = "world.manifest_missing";
    pub const WORLD_MANIFEST_PARSE: &str = "world.manifest_parse_error";
    pub const RIVERS_CATALOG_PARSE: &str = "rivers.catalog_parse_error";
    pub const RIVERS_LAYER_PARSE: &str = "rivers.layer_parse_error";
    pub const RIVERS_LAYER_MISSING: &str = "rivers.layer_missing";
    pub const RIVERS_CATALOG_LAYER_MISMATCH: &str = "rivers.catalog_layer_mismatch";
    pub const LAKES_CATALOG_PARSE: &str = "lakes.catalog_parse_error";
    pub const LAKES_LAYER_PARSE: &str = "lakes.layer_parse_error";
    pub const LAKES_LAYER_MISSING: &str = "lakes.layer_missing";
    pub const LAKES_CATALOG_LAYER_MISMATCH: &str = "lakes.catalog_layer_mismatch";
    pub const LAKES_CATALOG_INVARIANT: &str = "lakes.catalog_invariant";
    pub const LAYER_PARSE: &str = "layer.parse_error";
    pub const LAYER_BOUNDS_LENGTH: &str = "layer.bounds_length_mismatch";
    pub const HYDROLOGY_SNAPSHOT_PARSE: &str = "hydrology.snapshot_parse_error";
    pub const HYDROLOGY_SNAPSHOT_STALE_FINGERPRINT: &str = "hydrology.snapshot_stale_fingerprint";
    pub const HYDROLOGY_SNAPSHOT_STALE_REVISION: &str = "hydrology.snapshot_stale_revision";
    pub const HYDROLOGY_SNAPSHOT_SCHEMA: &str = "hydrology.snapshot_schema_version";
    pub const HYDROLOGY_SNAPSHOT_PROJECTION: &str = "hydrology.snapshot_invalid_projection";
    pub const NAMED_RIVERS_PARSE: &str = "named_rivers.parse_error";
    pub const NAMED_RIVERS_UNKNOWN_SEGMENT: &str = "named_rivers.unknown_segment";
    pub const NAMED_RIVERS_SNAPSHOT_ABSENT: &str = "named_rivers.snapshot_absent";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegritySeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityMode {
    PreCommit,
    PostCommit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityFinding {
    pub code: String,
    pub severity: IntegritySeverity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityReport {
    pub mode: IntegrityMode,
    pub findings: Vec<IntegrityFinding>,
}

impl IntegrityReport {
    pub fn has_errors(&self) -> bool {
        self.findings
            .iter()
            .any(|f| f.severity == IntegritySeverity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == IntegritySeverity::Error)
            .count()
    }
}

/// Optional parsed layer payload or a parse error string.
#[derive(Debug, Clone)]
pub struct LayerPayload {
    pub layer_id: String,
    pub layer: Option<DenseLayer>,
    pub parse_error: Option<String>,
}

/// In-memory world snapshot for validation (loaded by server/cli adapters).
#[derive(Debug, Clone, Default)]
pub struct WorldIntegrityInput {
    pub mapkeeper_toml_present: bool,
    pub mapkeeper_toml_parse_error: Option<String>,
    pub manifest_present: bool,
    pub manifest: Option<MapManifest>,
    pub manifest_parse_error: Option<String>,
    pub bounds: Option<MapBounds>,
    pub rivers_catalog: Option<RiverCatalog>,
    pub rivers_catalog_parse_error: Option<String>,
    pub river_id_layer: Option<DenseLayer>,
    pub river_id_parse_error: Option<String>,
    pub rivers_catalog_present: bool,
    pub river_id_layer_present: bool,
    pub lakes_catalog: Option<LakeCatalog>,
    pub lakes_catalog_parse_error: Option<String>,
    pub lake_id_layer: Option<DenseLayer>,
    pub lake_id_parse_error: Option<String>,
    pub lakes_catalog_present: bool,
    pub lake_id_layer_present: bool,
    pub hydrology_snapshot: Option<HydrologySnapshot>,
    pub hydrology_snapshot_parse_error: Option<String>,
    pub hydrology_snapshot_present: bool,
    pub hydrology_base_revision: Option<u64>,
    pub hydrology_base_fingerprint: Option<String>,
    pub named_rivers: Option<NamedRiverStore>,
    pub named_rivers_parse_error: Option<String>,
    pub named_rivers_present: bool,
    pub dense_layers: Vec<LayerPayload>,
}

/// Pure validator — no filesystem or logging side effects.
pub fn validate_world_integrity(input: &WorldIntegrityInput, mode: IntegrityMode) -> IntegrityReport {
    let mut findings = Vec::new();
    check_required_world_files(input, &mut findings);
    check_parse_errors(input, &mut findings);
    if let Some(bounds) = input.bounds {
        check_catalog_layer_sync(input, bounds, &mut findings);
        check_dense_layer_lengths(input, bounds, &mut findings);
        check_hydrology_snapshot(input, &mut findings);
        check_named_rivers(input, &mut findings);
    }
    let _ = mode; // mode is recorded on the report; checks are identical in v1
    IntegrityReport { mode, findings }
}

pub fn integrity_error_summary(report: &IntegrityReport) -> String {
    report
        .findings
        .iter()
        .filter(|f| f.severity == IntegritySeverity::Error)
        .map(|f| format!("{}: {}", f.code, f.message))
        .collect::<Vec<_>>()
        .join("; ")
}

fn push(
    findings: &mut Vec<IntegrityFinding>,
    code: &str,
    severity: IntegritySeverity,
    message: impl Into<String>,
    detail: Option<String>,
) {
    findings.push(IntegrityFinding {
        code: code.to_string(),
        severity,
        message: message.into(),
        detail,
    });
}

fn check_required_world_files(input: &WorldIntegrityInput, findings: &mut Vec<IntegrityFinding>) {
    if !input.mapkeeper_toml_present {
        push(
            findings,
            codes::WORLD_MAPKEEPER_TOML_MISSING,
            IntegritySeverity::Error,
            "mapkeeper.toml is missing",
            None,
        );
    }
    if !input.manifest_present {
        push(
            findings,
            codes::WORLD_MANIFEST_MISSING,
            IntegritySeverity::Error,
            "map/manifest.json is missing",
            None,
        );
    }
}

fn check_parse_errors(input: &WorldIntegrityInput, findings: &mut Vec<IntegrityFinding>) {
    if let Some(err) = &input.mapkeeper_toml_parse_error {
        push(
            findings,
            codes::WORLD_MAPKEEPER_TOML_PARSE,
            IntegritySeverity::Error,
            "mapkeeper.toml failed to parse",
            Some(err.clone()),
        );
    }
    if let Some(err) = &input.manifest_parse_error {
        push(
            findings,
            codes::WORLD_MANIFEST_PARSE,
            IntegritySeverity::Error,
            "map/manifest.json failed to parse",
            Some(err.clone()),
        );
    }
    if let Some(err) = &input.rivers_catalog_parse_error {
        push(
            findings,
            codes::RIVERS_CATALOG_PARSE,
            IntegritySeverity::Error,
            "map/rivers.json failed to parse",
            Some(err.clone()),
        );
    }
    if let Some(err) = &input.river_id_parse_error {
        push(
            findings,
            codes::RIVERS_LAYER_PARSE,
            IntegritySeverity::Error,
            "map/layers/river_id.json failed to parse",
            Some(err.clone()),
        );
    }
    if let Some(err) = &input.lakes_catalog_parse_error {
        push(
            findings,
            codes::LAKES_CATALOG_PARSE,
            IntegritySeverity::Error,
            "map/lakes.json failed to parse",
            Some(err.clone()),
        );
    }
    if let Some(err) = &input.lake_id_parse_error {
        push(
            findings,
            codes::LAKES_LAYER_PARSE,
            IntegritySeverity::Error,
            "map/layers/lake_id.json failed to parse",
            Some(err.clone()),
        );
    }
    if let Some(err) = &input.hydrology_snapshot_parse_error {
        push(
            findings,
            codes::HYDROLOGY_SNAPSHOT_PARSE,
            IntegritySeverity::Error,
            "map/hydrology-v2.json failed to parse",
            Some(err.clone()),
        );
    }
    if let Some(err) = &input.named_rivers_parse_error {
        push(
            findings,
            codes::NAMED_RIVERS_PARSE,
            IntegritySeverity::Error,
            "map/named-rivers.json failed to parse",
            Some(err.clone()),
        );
    }
    for layer in &input.dense_layers {
        if let Some(err) = &layer.parse_error {
            push(
                findings,
                codes::LAYER_PARSE,
                IntegritySeverity::Error,
                format!("map/layers/{}.json failed to parse", layer.layer_id),
                Some(err.clone()),
            );
        }
    }
}

fn check_catalog_layer_sync(
    input: &WorldIntegrityInput,
    bounds: MapBounds,
    findings: &mut Vec<IntegrityFinding>,
) {
    if input.rivers_catalog_parse_error.is_some() || input.river_id_parse_error.is_some() {
        return;
    }
    if input.rivers_catalog_present || input.river_id_layer_present {
        let catalog = input
            .rivers_catalog
            .as_ref()
            .cloned()
            .unwrap_or_default();
        let Some(layer) = &input.river_id_layer else {
            if input.rivers_catalog_present {
                push(
                    findings,
                    codes::RIVERS_LAYER_MISSING,
                    IntegritySeverity::Error,
                    "rivers.json exists but river_id layer is missing",
                    None,
                );
            }
            return;
        };
        let expected = sync_river_id_layer(&catalog, &bounds);
        if let Some(cell) = first_layer_mismatch(&expected, layer) {
            push(
                findings,
                codes::RIVERS_CATALOG_LAYER_MISMATCH,
                IntegritySeverity::Error,
                "rivers.json does not match river_id layer",
                Some(format!("first mismatch at cell index {cell}")),
            );
        }
    }

    if input.lakes_catalog_parse_error.is_some() || input.lake_id_parse_error.is_some() {
        return;
    }
    if input.lakes_catalog_present || input.lake_id_layer_present {
        let catalog = input.lakes_catalog.as_ref().cloned().unwrap_or_default();
        if let Some(catalog) = input.lakes_catalog.as_ref() {
            if let Err(err) = validate_catalog(catalog, &bounds) {
                push(
                    findings,
                    codes::LAKES_CATALOG_INVARIANT,
                    IntegritySeverity::Error,
                    "lakes.json catalog invariants failed",
                    Some(err.to_string()),
                );
            }
        }
        let Some(layer) = &input.lake_id_layer else {
            if input.lakes_catalog_present {
                push(
                    findings,
                    codes::LAKES_LAYER_MISSING,
                    IntegritySeverity::Error,
                    "lakes.json exists but lake_id layer is missing",
                    None,
                );
            }
            return;
        };
        let expected = sync_lake_id_layer(&catalog, &bounds);
        if let Some(cell) = first_layer_mismatch(&expected, layer) {
            push(
                findings,
                codes::LAKES_CATALOG_LAYER_MISMATCH,
                IntegritySeverity::Error,
                "lakes.json does not match lake_id layer",
                Some(format!("first mismatch at cell index {cell}")),
            );
        }
    }
}

fn check_dense_layer_lengths(
    input: &WorldIntegrityInput,
    bounds: MapBounds,
    findings: &mut Vec<IntegrityFinding>,
) {
    let expected = bounds.len();
    let mut check = |layer_id: &str, layer: &DenseLayer| {
        if layer.len() != expected {
            push(
                findings,
                codes::LAYER_BOUNDS_LENGTH,
                IntegritySeverity::Error,
                format!("layer {layer_id} length does not match manifest bounds"),
                Some(format!("expected {expected} cells, found {}", layer.len())),
            );
        }
    };
    if let Some(layer) = &input.river_id_layer {
        check(RIVER_ID_LAYER_ID, layer);
    }
    if let Some(layer) = &input.lake_id_layer {
        check(LAKE_ID_LAYER_ID, layer);
    }
    for payload in &input.dense_layers {
        if let Some(layer) = &payload.layer {
            check(&payload.layer_id, layer);
        }
    }
}

fn check_hydrology_snapshot(input: &WorldIntegrityInput, findings: &mut Vec<IntegrityFinding>) {
    if !input.hydrology_snapshot_present
        || input.hydrology_snapshot_parse_error.is_some()
        || input.hydrology_snapshot.is_none()
    {
        return;
    }
    let snapshot = input.hydrology_snapshot.as_ref().unwrap();
    let (revision, fingerprint) = match (
        input.hydrology_base_revision,
        input.hydrology_base_fingerprint.as_deref(),
    ) {
        (Some(rev), Some(fp)) => (rev, fp),
        _ => return,
    };
    if let Err(err) = snapshot.validate_current(revision, fingerprint) {
        let (code, message) = match err {
            HydrologySnapshotError::SchemaVersion { found } => (
                codes::HYDROLOGY_SNAPSHOT_SCHEMA,
                format!("hydrology snapshot schema version {found} is unsupported"),
            ),
            HydrologySnapshotError::StaleBaseRevision { expected, found } => (
                codes::HYDROLOGY_SNAPSHOT_STALE_REVISION,
                format!("hydrology snapshot base revision stale (expected {expected}, found {found})"),
            ),
            HydrologySnapshotError::StaleFingerprint => (
                codes::HYDROLOGY_SNAPSHOT_STALE_FINGERPRINT,
                "hydrology snapshot input fingerprint is stale".to_string(),
            ),
            HydrologySnapshotError::InvalidChannelProjection => (
                codes::HYDROLOGY_SNAPSHOT_PROJECTION,
                "hydrology snapshot channel projection is invalid".to_string(),
            ),
        };
        push(findings, code, IntegritySeverity::Error, message, None);
    }
}

fn check_named_rivers(input: &WorldIntegrityInput, findings: &mut Vec<IntegrityFinding>) {
    if !input.named_rivers_present || input.named_rivers_parse_error.is_some() {
        return;
    }
    let Some(store) = &input.named_rivers else {
        return;
    };
    if store.rivers.is_empty() {
        return;
    }
    let Some(snapshot) = &input.hydrology_snapshot else {
        push(
            findings,
            codes::NAMED_RIVERS_SNAPSHOT_ABSENT,
            IntegritySeverity::Warning,
            "named-rivers.json present but hydrology snapshot is absent",
            None,
        );
        return;
    };
    let segment_ids: std::collections::HashSet<u32> = snapshot
        .catalog
        .physical_segments
        .iter()
        .map(|s| s.id)
        .collect();
    for binding in &store.rivers {
        for seg_id in &binding.segment_ids {
            if !segment_ids.contains(seg_id) {
                push(
                    findings,
                    codes::NAMED_RIVERS_UNKNOWN_SEGMENT,
                    IntegritySeverity::Error,
                    format!(
                        "named river '{}' references unknown segment {seg_id}",
                        binding.name
                    ),
                    Some(format!("named_river_id={}", binding.id)),
                );
            }
        }
    }
}

fn first_layer_mismatch(expected: &DenseLayer, actual: &DenseLayer) -> Option<usize> {
    let len = expected.len().min(actual.len());
    for i in 0..len {
        if expected.int_or(i, -1) != actual.int_or(i, -1) {
            return Some(i);
        }
    }
    if expected.len() != actual.len() {
        return Some(len);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rivers::River;
    use crate::worldgen::hydrology::{
        ChannelGraph, DrainageGraph, HydrologyFlow, RiverGraph,
    };

    fn small_bounds() -> MapBounds {
        MapBounds::new(14, 8)
    }

    fn minimal_valid_input() -> WorldIntegrityInput {
        let bounds = small_bounds();
        let manifest = MapManifest::default_v0(14, 8);
        WorldIntegrityInput {
            mapkeeper_toml_present: true,
            manifest_present: true,
            manifest: Some(manifest),
            bounds: Some(bounds),
            ..Default::default()
        }
    }

    #[test]
    fn valid_minimal_world_has_no_errors() {
        let report = validate_world_integrity(&minimal_valid_input(), IntegrityMode::PostCommit);
        assert!(!report.has_errors(), "{:?}", report.findings);
    }

    #[test]
    fn river_catalog_layer_mismatch_is_reported() {
        let bounds = small_bounds();
        let mut catalog = RiverCatalog::default();
        catalog.rivers.push(River {
            id: 1,
            cells: vec![3],
            source: 3,
            mouth: 3,
            parent: 1,
            basin: 1,
            name: None,
        });
        let mut layer = sync_river_id_layer(&catalog, &bounds);
        layer.set(3, crate::layer::DenseState::Value(crate::layer::LayerValue::Int(2)));
        let input = WorldIntegrityInput {
            rivers_catalog_present: true,
            river_id_layer_present: true,
            rivers_catalog: Some(catalog),
            river_id_layer: Some(layer),
            ..minimal_valid_input()
        };
        let report = validate_world_integrity(&input, IntegrityMode::PostCommit);
        assert!(report.findings.iter().any(|f| {
            f.code == codes::RIVERS_CATALOG_LAYER_MISMATCH
                && f.severity == IntegritySeverity::Error
        }));
    }

    #[test]
    fn missing_required_files_are_errors() {
        let input = WorldIntegrityInput::default();
        let report = validate_world_integrity(&input, IntegrityMode::PostCommit);
        assert!(report.findings.iter().any(|f| f.code == codes::WORLD_MAPKEEPER_TOML_MISSING));
        assert!(report.findings.iter().any(|f| f.code == codes::WORLD_MANIFEST_MISSING));
    }

    #[test]
    fn lake_layer_length_mismatch_is_reported() {
        let bounds = small_bounds();
        let mut layer = DenseLayer::new_integer(LAKE_ID_LAYER_ID, bounds.len() - 1);
        for i in 0..layer.len() {
            layer.set(i, crate::layer::DenseState::Value(crate::layer::LayerValue::Int(0)));
        }
        let input = WorldIntegrityInput {
            lake_id_layer_present: true,
            lake_id_layer: Some(layer),
            ..minimal_valid_input()
        };
        let report = validate_world_integrity(&input, IntegrityMode::PostCommit);
        assert!(report.findings.iter().any(|f| f.code == codes::LAYER_BOUNDS_LENGTH));
    }

    #[test]
    fn named_river_unknown_segment_is_reported() {
        let store = NamedRiverStore {
            schema_version: 1,
            next_id: 2,
            rivers: vec![crate::worldgen::hydrology::NamedRiverBinding {
                id: 1,
                name: "Stem".into(),
                segment_ids: vec![99],
            }],
        };
        let bounds = small_bounds();
        let snapshot = HydrologySnapshot::new(
            0,
            "abc".into(),
            "hydrology-v2".into(),
            "channel-v1".into(),
            1,
            DrainageGraph {
                nodes: vec![],
                receiver: vec![],
                rank: vec![],
                terrain_receiver: vec![],
            },
            ChannelGraph {
                flow: HydrologyFlow {
                    local_runoff: vec![],
                    accumulated_flow: vec![],
                    contributing_area: vec![],
                },
                river_graph: RiverGraph {
                    nodes: vec![],
                    segments: vec![],
                    channel_mask: vec![],
                    channel_segment_id: vec![],
                    channel_node_id: vec![],
                },
            },
        );
        let input = WorldIntegrityInput {
            named_rivers_present: true,
            named_rivers: Some(store),
            hydrology_snapshot_present: true,
            hydrology_snapshot: Some(snapshot),
            hydrology_base_revision: Some(0),
            hydrology_base_fingerprint: Some("abc".into()),
            ..minimal_valid_input()
        };
        let _ = bounds;
        let report = validate_world_integrity(&input, IntegrityMode::PostCommit);
        assert!(report.findings.iter().any(|f| f.code == codes::NAMED_RIVERS_UNKNOWN_SEGMENT));
    }
}
