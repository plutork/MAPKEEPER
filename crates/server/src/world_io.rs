//! World filesystem, manifest, bounds, and layer I/O helpers (D-96 S0).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mapkeeper_core::cell_id::CellId;
use mapkeeper_core::climate::{ICE_LAYER_ID, PRECIPITATION_LAYER_ID, TEMPERATURE_LAYER_ID};
use mapkeeper_core::geology::GEOLOGY_LAYER_ID;
use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::lakes::{sync_lake_id_layer, LakeCatalog, LAKE_CATALOG_FILE};
use mapkeeper_core::land_mask::LAND_MASK_LAYER_ID;
use mapkeeper_core::layer::Bounds;
use mapkeeper_core::layer::{
    DenseLayer, MapManifest, ValueType, ELEVATION_LAYER_ID, LAKE_ID_LAYER_ID, RIVER_ID_LAYER_ID,
};
use mapkeeper_core::map_preset::{legacy_default_bounds, MapPreset};
use mapkeeper_core::projects::{projects_file_path, ProjectEntry, ProjectsFile};
use mapkeeper_core::rivers::{sync_river_id_layer, RiverCatalog, RIVER_CATALOG_FILE};
use mapkeeper_core::worldgen::hydrology::{
    compatibility_river_id_layer, HydrologySnapshot, HYDROLOGY_SNAPSHOT_FILE,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    world: WorldSection,
}

#[derive(Deserialize)]
struct WorldSection {
    id: String,
}

pub(crate) fn read_manifest_id(world_path: &Path) -> Result<String> {
    let manifest_path = world_path.join("mapkeeper.toml");
    let raw = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "reading {} — is this a mapkeeper world? (see `mapkeeper init`)",
            manifest_path.display()
        )
    })?;
    let manifest: Manifest = toml::from_str(&raw).context("parsing mapkeeper.toml")?;
    Ok(manifest.world.id)
}

pub(crate) fn map_manifest_path(world_path: &Path) -> PathBuf {
    world_path.join("map/manifest.json")
}

pub(crate) fn legacy_map_folder(world_path: &Path) -> bool {
    !map_manifest_path(world_path).exists()
}

pub(crate) fn write_map_manifest(world_path: &Path, preset: MapPreset) -> Result<()> {
    rewrite_world_bounds(world_path, preset, false)?;
    Ok(())
}

/// Rewrite manifest bounds; optionally wipe Geo pipeline layers (D-69 / D-46).
pub(crate) fn rewrite_world_bounds(
    world_path: &Path,
    preset: MapPreset,
    reset_pipeline: bool,
) -> Result<MapBounds> {
    let (width, height) = preset.dimensions();
    let manifest = MapManifest::default_v0(width, height);
    let path = map_manifest_path(world_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, manifest.to_json_pretty()?)?;
    let bounds = MapBounds::new(width, height);
    if reset_pipeline {
        for id in [
            LAND_MASK_LAYER_ID,
            GEOLOGY_LAYER_ID,
            "terrain",
            RIVER_ID_LAYER_ID,
            LAKE_ID_LAYER_ID,
            TEMPERATURE_LAYER_ID,
            PRECIPITATION_LAYER_ID,
            ICE_LAYER_ID,
        ] {
            let _ = std::fs::remove_file(layer_file_path(world_path, id));
        }
        let rivers = world_path.join("map").join(RIVER_CATALOG_FILE);
        let _ = std::fs::remove_file(rivers);
    }
    let ocean = mapkeeper_core::hydro::filled_elevation_layer(
        &bounds,
        mapkeeper_core::hydro::OCEAN_ELEVATION,
    );
    write_dense_layer(world_path, &ocean).map_err(|e| anyhow::anyhow!(e))?;
    Ok(bounds)
}

pub(crate) fn pipeline_has_downstream(world_path: &Path) -> bool {
    layer_file_path(world_path, LAND_MASK_LAYER_ID).exists()
        || layer_file_path(world_path, GEOLOGY_LAYER_ID).exists()
}

/// Read hex bounds from disk. Missing manifest => Small rectangle default.
pub(crate) fn read_map_bounds(world_path: &Path) -> (MapBounds, bool) {
    let path = map_manifest_path(world_path);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return (legacy_default_bounds(), true);
    };
    let Ok(manifest) = MapManifest::from_json(&raw) else {
        return (legacy_default_bounds(), true);
    };
    match manifest.bounds {
        Bounds::HexRectangle { width, height } => (MapBounds::new(width, height), false),
    }
}

/// scale-layers (D-46): map bounds as the cell-index domain for dense layers.
pub(crate) fn map_bounds(world_path: &Path) -> MapBounds {
    read_map_bounds(world_path).0
}

pub(crate) fn projects_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").ok();
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok();
    PathBuf::from(projects_file_path(appdata.as_deref(), home.as_deref()))
}

pub(crate) fn load_projects() -> ProjectsFile {
    let parsed = match std::fs::read_to_string(projects_path()) {
        Ok(raw) => ProjectsFile::parse(&raw),
        Err(_) => ProjectsFile::default(),
    };
    dedupe_projects(parsed)
}

pub(crate) fn save_projects(file: &ProjectsFile) -> Result<()> {
    let path = projects_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, file.to_json_pretty())?;
    Ok(())
}

pub(crate) fn normalize_world_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let normalized = std::fs::canonicalize(&absolute).unwrap_or(absolute);
    strip_windows_verbatim_prefix(normalized)
}

pub(crate) fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    let raw = path.to_string_lossy();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{}", rest));
    }
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    path
}

pub(crate) fn path_cmp_key(path: &Path) -> String {
    let normalized = normalize_world_path(path);
    let key = normalized.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    }
}

pub(crate) fn dedupe_projects(mut file: ProjectsFile) -> ProjectsFile {
    let mut unique: Vec<ProjectEntry> = Vec::new();
    for p in file.projects.drain(..) {
        let normalized = normalize_world_path(Path::new(&p.path));
        let normalized_path = normalized.display().to_string();
        let key = path_cmp_key(&normalized);
        if let Some(existing) = unique
            .iter_mut()
            .find(|e| path_cmp_key(Path::new(&e.path)) == key)
        {
            *existing = ProjectEntry {
                id: p.id,
                path: normalized_path,
            };
        } else {
            unique.push(ProjectEntry {
                id: p.id,
                path: normalized_path,
            });
        }
    }
    file.projects = unique;
    file
}

pub(crate) fn default_worlds_root_path() -> String {
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        return PathBuf::from(userprofile)
            .join("Documents")
            .join("MAPKEEPER Worlds")
            .display()
            .to_string();
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join("Documents")
            .join("MAPKEEPER Worlds")
            .display()
            .to_string();
    }
    "MAPKEEPER Worlds".to_string()
}

pub(crate) fn default_worlds_root() -> PathBuf {
    PathBuf::from(default_worlds_root_path())
}

pub(crate) fn is_valid_fixture_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 64
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Locate `fixtures/worlds` for river dogfood (dev / repo checkout).
pub(crate) fn fixture_worlds_root() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("MAPKEEPER_FIXTURE_WORLDS") {
        let path = PathBuf::from(raw);
        if path.is_dir() {
            return Some(path);
        }
    }
    let mut dir = std::env::current_dir().ok()?;
    for _ in 0..8 {
        let candidate = dir.join("fixtures").join("worlds");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

pub(crate) fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

pub(crate) fn import_fixture_world(slug: &str) -> Result<PathBuf> {
    if !is_valid_fixture_slug(slug) {
        anyhow::bail!("invalid fixture slug");
    }
    let root = fixture_worlds_root().context(
        "fixture worlds not found (run from MAPKEEPER repo or set MAPKEEPER_FIXTURE_WORLDS)",
    )?;
    let src = root.join(slug);
    if !src.join("mapkeeper.toml").is_file() {
        anyhow::bail!("unknown fixture world: {slug}");
    }
    let dest = default_worlds_root().join(format!("fixture-{slug}"));
    if !dest.join("mapkeeper.toml").exists() {
        std::fs::create_dir_all(dest.parent().unwrap_or(Path::new(".")))?;
        if dest.exists() {
            std::fs::remove_dir_all(&dest).context("replacing incomplete fixture import")?;
        }
        copy_dir_all(&src, &dest)?;
    }
    Ok(normalize_world_path(&dest))
}
pub(crate) fn profiles_dir(world_path: &Path) -> PathBuf {
    world_path.join("profiles")
}

pub(crate) fn profile_path(world_path: &Path, world_id: &str, q: i32, r: i32) -> PathBuf {
    let id = CellId::new(world_id, q, r);
    profiles_dir(world_path).join(id.filename())
}
pub(crate) fn layer_file_path(world_path: &Path, layer_id: &str) -> PathBuf {
    world_path
        .join("map")
        .join("layers")
        .join(format!("{layer_id}.json"))
}

/// Default value kind for a not-yet-created layer. Only `elevation` is integer
/// today; everything else defaults to categorical.
pub(crate) fn default_value_type(layer_id: &str) -> ValueType {
    if layer_id == ELEVATION_LAYER_ID
        || layer_id == RIVER_ID_LAYER_ID
        || layer_id == LAKE_ID_LAYER_ID
        || layer_id == TEMPERATURE_LAYER_ID
        || layer_id == PRECIPITATION_LAYER_ID
        || layer_id == ICE_LAYER_ID
    {
        ValueType::Integer
    } else {
        ValueType::Categorical
    }
}

pub(crate) fn read_optional_precip_layer(
    world_path: &Path,
    bounds: &MapBounds,
) -> Option<DenseLayer> {
    let path = layer_file_path(world_path, PRECIPITATION_LAYER_ID);
    if !path.exists() {
        return None;
    }
    Some(read_dense_layer(world_path, PRECIPITATION_LAYER_ID, bounds))
}

pub(crate) fn read_dense_layer(
    world_path: &Path,
    layer_id: &str,
    bounds: &MapBounds,
) -> DenseLayer {
    let raw = std::fs::read_to_string(layer_file_path(world_path, layer_id)).ok();
    DenseLayer::read_or_empty(
        raw.as_deref(),
        layer_id,
        default_value_type(layer_id),
        bounds,
    )
}

pub(crate) fn write_dense_layer(world_path: &Path, layer: &DenseLayer) -> Result<(), String> {
    #[cfg(test)]
    if SIMULATE_LAYER_WRITE_FAILURE.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("simulated layer write failure".to_string());
    }
    let path = layer_file_path(world_path, &layer.layer_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = layer.to_json_pretty().map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    if is_hydrology_base_input_layer(&layer.layer_id) {
        invalidate_hydrology_snapshot(world_path)?;
    }
    Ok(())
}
pub(crate) fn rivers_file_path(world_path: &Path) -> PathBuf {
    world_path.join("map").join(RIVER_CATALOG_FILE)
}

pub(crate) fn read_river_catalog(world_path: &Path) -> RiverCatalog {
    std::fs::read_to_string(rivers_file_path(world_path))
        .ok()
        .and_then(|raw| RiverCatalog::from_json(&raw).ok())
        .unwrap_or_default()
}

pub(crate) fn write_river_catalog(world_path: &Path, catalog: &RiverCatalog) -> Result<(), String> {
    let path = rivers_file_path(world_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = catalog.to_json_pretty().map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
}

pub(crate) fn persist_rivers(
    world_path: &Path,
    catalog: &RiverCatalog,
    bounds: &MapBounds,
) -> Result<(), String> {
    write_river_catalog(world_path, catalog)?;
    let layer = sync_river_id_layer(catalog, bounds);
    write_dense_layer(world_path, &layer)
}

#[allow(dead_code)] // Removed with the final legacy hydrology cleanup slice.
pub(crate) fn persist_generated_rivers(
    world_path: &Path,
    catalog: &RiverCatalog,
    bounds: &MapBounds,
) -> Result<(), String> {
    persist_rivers(world_path, catalog, bounds)
}

pub(crate) fn lakes_file_path(world_path: &Path) -> PathBuf {
    world_path.join("map").join(LAKE_CATALOG_FILE)
}

pub(crate) fn hydrology_snapshot_path(world_path: &Path) -> PathBuf {
    world_path.join("map").join(HYDROLOGY_SNAPSHOT_FILE)
}

/// Stable fingerprint/revision of all base inputs owned outside Hydrology v2.
#[allow(dead_code)] // Snapshot activation endpoint follows in the next slice.
pub(crate) fn hydrology_base_fingerprint(world_path: &Path) -> (u64, String) {
    const INPUTS: &[&str] = &[
        LAND_MASK_LAYER_ID,
        ELEVATION_LAYER_ID,
        LAKE_ID_LAYER_ID,
        PRECIPITATION_LAYER_ID,
    ];
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for layer_id in INPUTS {
        for byte in layer_id.bytes().chain([0u8]).chain(
            std::fs::read(layer_file_path(world_path, layer_id))
                .unwrap_or_default()
                .into_iter(),
        ) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    }
    (hash, format!("{hash:016x}"))
}

fn is_hydrology_base_input_layer(layer_id: &str) -> bool {
    matches!(
        layer_id,
        LAND_MASK_LAYER_ID | ELEVATION_LAYER_ID | LAKE_ID_LAYER_ID | PRECIPITATION_LAYER_ID
    )
}

/// Atomically activate one complete v2 hydrology bundle. The previous bundle
/// is restored if staging or activation fails.
pub(crate) fn persist_hydrology_snapshot(
    world_path: &Path,
    snapshot: &HydrologySnapshot,
) -> Result<(), String> {
    let (base_revision, fingerprint) = hydrology_base_fingerprint(world_path);
    snapshot
        .validate_current(base_revision, &fingerprint)
        .map_err(|err| format!("refusing stale hydrology snapshot: {err:?}"))?;
    let path = hydrology_snapshot_path(world_path);
    let river_layer_path = layer_file_path(world_path, RIVER_ID_LAYER_ID);
    let prior_snapshot = std::fs::read_to_string(&path).ok();
    let prior_river_layer = std::fs::read_to_string(&river_layer_path).ok();
    let parent = path.parent().ok_or("hydrology snapshot has no parent")?;
    std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let staged = path.with_extension("json.staged");
    let backup = path.with_extension("json.previous");
    let body = snapshot.to_json_pretty().map_err(|err| err.to_string())?;
    std::fs::write(&staged, body).map_err(|err| err.to_string())?;
    #[cfg(test)]
    if SIMULATE_HYDROLOGY_ACTIVATION_FAILURE.load(std::sync::atomic::Ordering::SeqCst) {
        let _ = std::fs::remove_file(&staged);
        return Err("simulated hydrology activation failure".to_string());
    }
    if backup.exists() {
        std::fs::remove_file(&backup).map_err(|err| err.to_string())?;
    }
    let had_active = path.exists();
    if had_active {
        std::fs::rename(&path, &backup).map_err(|err| err.to_string())?;
    }
    if let Err(err) = std::fs::rename(&staged, &path) {
        if had_active {
            let _ = std::fs::rename(&backup, &path);
        }
        let _ = std::fs::remove_file(&staged);
        return Err(err.to_string());
    }
    let river_layer = compatibility_river_id_layer(
        &snapshot.channels.river_graph,
        snapshot.channels.river_graph.channel_mask.len(),
    );
    if let Err(err) = write_dense_layer(world_path, &river_layer) {
        restore_file(&path, prior_snapshot)?;
        restore_file(&river_layer_path, prior_river_layer)?;
        return Err(err);
    }
    if had_active {
        std::fs::remove_file(&backup).map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[allow(dead_code)] // Renderer/catalog cutover reads this in the next slice.
pub(crate) fn read_current_hydrology_snapshot(
    world_path: &Path,
) -> Result<Option<HydrologySnapshot>, String> {
    let path = hydrology_snapshot_path(world_path);
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let snapshot = HydrologySnapshot::from_json(&raw).map_err(|err| err.to_string())?;
    let (base_revision, fingerprint) = hydrology_base_fingerprint(world_path);
    snapshot
        .validate_current(base_revision, &fingerprint)
        .map_err(|err| format!("stale hydrology snapshot: {err:?}"))?;
    Ok(Some(snapshot))
}

pub(crate) fn invalidate_hydrology_snapshot(world_path: &Path) -> Result<(), String> {
    let path = hydrology_snapshot_path(world_path);
    if path.exists() {
        std::fs::remove_file(path).map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub(crate) fn read_lake_catalog(world_path: &Path) -> LakeCatalog {
    std::fs::read_to_string(lakes_file_path(world_path))
        .ok()
        .and_then(|raw| LakeCatalog::from_json(&raw).ok())
        .unwrap_or_default()
}

pub(crate) fn write_lake_catalog(world_path: &Path, catalog: &LakeCatalog) -> Result<(), String> {
    let path = lakes_file_path(world_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = catalog.to_json_pretty().map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
}

fn restore_file(path: &Path, backup: Option<String>) -> Result<(), String> {
    match backup {
        Some(bytes) => std::fs::write(path, bytes).map_err(|e| e.to_string()),
        None => {
            if path.exists() {
                std::fs::remove_file(path).map_err(|e| e.to_string())
            } else {
                Ok(())
            }
        }
    }
}

/// Write `lakes.json` then derive `lake_id` layer. Rolls back catalog on layer failure.
pub(crate) fn persist_lakes(
    world_path: &Path,
    catalog: &LakeCatalog,
    bounds: &MapBounds,
) -> Result<(), String> {
    let catalog_path = lakes_file_path(world_path);
    let backup_catalog = std::fs::read_to_string(&catalog_path).ok();

    write_lake_catalog(world_path, catalog)?;
    let layer = sync_lake_id_layer(catalog, bounds);
    if let Err(err) = write_dense_layer(world_path, &layer) {
        restore_file(&catalog_path, backup_catalog)?;
        return Err(err);
    }
    Ok(())
}

/// Replace rivers with an empty catalog + zero `river_id` layer.
pub(crate) fn clear_rivers(world_path: &Path, bounds: &MapBounds) -> Result<(), String> {
    persist_rivers(world_path, &RiverCatalog::default(), bounds)
}

/// Persist lakes then clear rivers (lake regen invalidates river mouths).
pub(crate) fn persist_lake_generation(
    world_path: &Path,
    catalog: &LakeCatalog,
    bounds: &MapBounds,
) -> Result<(), String> {
    persist_lakes(world_path, catalog, bounds)?;
    clear_rivers(world_path, bounds)
}

#[cfg(test)]
pub(crate) static SIMULATE_LAYER_WRITE_FAILURE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
pub(crate) static SIMULATE_HYDROLOGY_ACTIVATION_FAILURE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
mod persist_lakes_tests {
    use super::*;
    use mapkeeper_core::hydro::SEA_LEVEL;
    use mapkeeper_core::lakes::Lake;
    use mapkeeper_core::layer::{DenseState, LayerValue};
    use mapkeeper_core::map_preset::MapPreset;
    use mapkeeper_core::worldgen::hydrology::{
        analyze_depressions, build_channel_graph, build_drainage_graph, ChannelPolicy,
        HydrologySnapshot,
    };
    use std::sync::atomic::Ordering;
    use tempfile::tempdir;

    static HYDROLOGY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn seed_world(path: &Path) -> MapBounds {
        std::fs::create_dir_all(path.join("map/layers")).unwrap();
        rewrite_world_bounds(path, MapPreset::Small, false).unwrap()
    }

    fn snapshot(world: &Path, bounds: &MapBounds, seed: u64) -> HydrologySnapshot {
        let mut elevation = DenseLayer::new_integer(ELEVATION_LAYER_ID, bounds.len());
        for cell in 0..bounds.len() {
            let height = if cell < bounds.width as usize {
                SEA_LEVEL
            } else {
                SEA_LEVEL + 20
            };
            elevation.set(cell, DenseState::Value(LayerValue::Int(height)));
        }
        write_dense_layer(world, &elevation).unwrap();
        let analysis = analyze_depressions(&elevation, bounds);
        let drainage = build_drainage_graph(&analysis, &LakeCatalog::default(), bounds).unwrap();
        let channels =
            build_channel_graph(&drainage, &analysis, None, ChannelPolicy::default()).unwrap();
        let (base_revision, fingerprint) = hydrology_base_fingerprint(world);
        HydrologySnapshot::new(
            base_revision,
            fingerprint,
            "hydrology-v2".to_string(),
            "channel-v1".to_string(),
            seed,
            drainage,
            channels,
        )
    }

    #[test]
    fn persist_lakes_matches_sync_layer() {
        let dir = tempdir().unwrap();
        let world = dir.path();
        let bounds = seed_world(world);
        let mut catalog = LakeCatalog::default();
        catalog.lakes.push(Lake {
            id: 1,
            cells: vec![2, 3],
            outlet_cell: Some(2),
            endorheic: false,
            name: None,
        });
        persist_lakes(world, &catalog, &bounds).unwrap();
        let read = read_lake_catalog(world);
        assert_eq!(read, catalog);
        let on_disk = read_dense_layer(world, LAKE_ID_LAYER_ID, &bounds);
        let expected = sync_lake_id_layer(&catalog, &bounds);
        assert_eq!(on_disk, expected);
    }

    #[test]
    fn empty_catalog_roundtrip() {
        let dir = tempdir().unwrap();
        let world = dir.path();
        let bounds = seed_world(world);
        let catalog = LakeCatalog::default();
        persist_lakes(world, &catalog, &bounds).unwrap();
        assert_eq!(read_lake_catalog(world), catalog);
        let layer = read_dense_layer(world, LAKE_ID_LAYER_ID, &bounds);
        assert_eq!(layer.int_or(0, -1), 0);
    }

    #[test]
    fn rollback_catalog_when_layer_write_fails() {
        let dir = tempdir().unwrap();
        let world = dir.path();
        let bounds = seed_world(world);
        let initial = LakeCatalog::default();
        persist_lakes(world, &initial, &bounds).unwrap();
        let before_catalog = std::fs::read_to_string(lakes_file_path(world)).unwrap();

        let mut next = LakeCatalog::default();
        next.lakes.push(Lake {
            id: 1,
            cells: vec![5],
            outlet_cell: None,
            endorheic: false,
            name: None,
        });

        SIMULATE_LAYER_WRITE_FAILURE.store(true, Ordering::SeqCst);
        let err = persist_lakes(world, &next, &bounds).unwrap_err();
        SIMULATE_LAYER_WRITE_FAILURE.store(false, Ordering::SeqCst);
        assert!(err.contains("simulated"));

        let after_catalog = std::fs::read_to_string(lakes_file_path(world)).unwrap();
        assert_eq!(after_catalog, before_catalog);
        let layer = read_dense_layer(world, LAKE_ID_LAYER_ID, &bounds);
        assert_eq!(layer, sync_lake_id_layer(&initial, &bounds));
    }

    #[test]
    fn hydrology_snapshot_activation_keeps_prior_bundle_on_failure() {
        let _lock = HYDROLOGY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempdir().unwrap();
        let world = dir.path();
        let bounds = seed_world(world);
        let first = snapshot(world, &bounds, 1);
        persist_hydrology_snapshot(world, &first).unwrap();

        let mut second = first.clone();
        second.effective_seed = 2;
        SIMULATE_HYDROLOGY_ACTIVATION_FAILURE.store(true, Ordering::SeqCst);
        assert!(persist_hydrology_snapshot(world, &second).is_err());
        SIMULATE_HYDROLOGY_ACTIVATION_FAILURE.store(false, Ordering::SeqCst);

        assert_eq!(read_current_hydrology_snapshot(world).unwrap(), Some(first));
    }

    #[test]
    fn base_layer_write_invalidates_hydrology_snapshot() {
        let _lock = HYDROLOGY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempdir().unwrap();
        let world = dir.path();
        let bounds = seed_world(world);
        let snapshot = snapshot(world, &bounds, 1);
        persist_hydrology_snapshot(world, &snapshot).unwrap();

        let mut elevation = read_dense_layer(world, ELEVATION_LAYER_ID, &bounds);
        elevation.set(0, DenseState::Value(LayerValue::Int(SEA_LEVEL + 1)));
        write_dense_layer(world, &elevation).unwrap();

        assert_eq!(read_current_hydrology_snapshot(world).unwrap(), None);
    }

    #[test]
    fn mismatched_base_fingerprint_is_not_current_hydrology() {
        let _lock = HYDROLOGY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempdir().unwrap();
        let world = dir.path();
        let bounds = seed_world(world);
        let snapshot = snapshot(world, &bounds, 1);
        persist_hydrology_snapshot(world, &snapshot).unwrap();

        std::fs::write(layer_file_path(world, ELEVATION_LAYER_ID), "changed").unwrap();

        assert!(read_current_hydrology_snapshot(world).is_err());
    }
}
