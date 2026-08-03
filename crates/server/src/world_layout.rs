//! N-035 / N-037 two-level world⊃maps layout helpers.

use std::path::{Path, PathBuf};

use mapkeeper_core::world::{self, map_rel_path, WorldMapRef, DEFAULT_FIRST_MAP_ID};

pub const LEGACY_REFUSE_MSG: &str =
    "legacy_world_format: this folder is an old single-level world; create a new world (N-037)";

pub fn read_world_manifest(world_path: &Path) -> Result<world::WorldManifest, String> {
    let path = world_path.join("mapkeeper.toml");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    world::parse_manifest(&raw).map_err(|e| format!("invalid {}: {e}", path.display()))
}

pub fn is_legacy_world_dir(world_path: &Path) -> bool {
    let root_spatial = world_path.join("spatial").is_dir();
    let Ok(raw) = std::fs::read_to_string(world_path.join("mapkeeper.toml")) else {
        return root_spatial;
    };
    world::looks_legacy_single_level(&raw, root_spatial)
}

pub fn map_abs_path(world_path: &Path, map_ref: &WorldMapRef) -> PathBuf {
    world_path.join(&map_ref.path)
}

pub fn resolve_map_ref<'a>(
    manifest: &'a world::WorldManifest,
    map_id: Option<&str>,
) -> Result<&'a WorldMapRef, String> {
    if !world::is_two_level_world(manifest) {
        return Err("world has no maps".into());
    }
    match map_id {
        None | Some("") => Ok(&manifest.maps[0]),
        Some(id) => manifest
            .maps
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| format!("unknown map `{id}`")),
    }
}

/// Open gate: refuse legacy; require two-level + map.toml present.
pub fn prepare_open(
    world_path: &Path,
    map_id: Option<&str>,
) -> Result<(String, PathBuf, String), String> {
    if is_legacy_world_dir(world_path) {
        return Err(LEGACY_REFUSE_MSG.into());
    }
    let manifest = read_world_manifest(world_path)?;
    if !world::is_valid_world_id(&manifest.world.id) {
        return Err("invalid world id".into());
    }
    if !world::is_two_level_world(&manifest) {
        return Err("world_format: missing maps list (create a new world)".into());
    }
    let map_ref = resolve_map_ref(&manifest, map_id)?;
    let map_path = map_abs_path(world_path, map_ref);
    if !map_path.join("map.toml").is_file() {
        return Err(format!("missing map.toml at {}", map_path.display()));
    }
    Ok((manifest.world.id.clone(), map_path, map_ref.id.clone()))
}

pub fn default_first_map_ref() -> WorldMapRef {
    WorldMapRef {
        id: DEFAULT_FIRST_MAP_ID.to_string(),
        name: DEFAULT_FIRST_MAP_ID.to_string(),
        path: map_rel_path(DEFAULT_FIRST_MAP_ID),
    }
}

/// Test/seed helper: write two-level world + one map (no spatial state).
#[cfg(test)]
pub fn write_world_skeleton(
    world_path: &Path,
    world_id: &str,
    preset: &mapkeeper_core::spatial::MapExtentPreset,
) -> Result<PathBuf, String> {
    let map_ref = default_first_map_ref();
    let map_path = map_abs_path(world_path, &map_ref);
    std::fs::create_dir_all(&map_path).map_err(|e| e.to_string())?;
    std::fs::write(
        world_path.join("mapkeeper.toml"),
        world::world_manifest_toml(world_id, &[map_ref]),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        map_path.join("map.toml"),
        world::map_manifest_toml(DEFAULT_FIRST_MAP_ID, preset),
    )
    .map_err(|e| e.to_string())?;
    Ok(map_path)
}
