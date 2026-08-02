//! Create saga: preset resolution + durable world+first-map creation (N-025 / N-035 / N-037).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use mapkeeper_core::projects::ProjectEntry;
use mapkeeper_core::spatial::preset_by_id;
use mapkeeper_core::world::{self, SpatialConfig, DEFAULT_FIRST_MAP_ID};

use super::{forget_registry_only, registry_error, CreateProjectResult};
use crate::state::{ActiveWorld, ServerState};
use crate::world_io;
use crate::world_layout;

pub(super) fn resolve_create_preset(
    preset_id: Option<&str>,
) -> Result<&'static mapkeeper_core::spatial::MapExtentPreset, String> {
    let preset = match preset_id {
        None | Some("") => mapkeeper_core::spatial::alpha_default_preset(),
        Some(id) => match preset_by_id(id) {
            Some(preset)
                if mapkeeper_core::spatial::create_presets()
                    .iter()
                    .any(|p| p.id == preset.id) =>
            {
                preset
            }
            Some(_) => {
                return Err(format!("preset `{id}` is not available in Create catalog"));
            }
            None => {
                return Err(format!("unknown map extent preset `{id}`"));
            }
        },
    };
    SpatialConfig::from_preset(preset).assert_matches_catalog()?;
    Ok(preset)
}

/// Create dir + world manifest + first map + spatial + registry + activate (N-035).
pub(super) fn transactional_create(
    server: &Arc<ServerState>,
    id: String,
    path: PathBuf,
    preset: &mapkeeper_core::spatial::MapExtentPreset,
) -> axum::response::Response {
    let manifest_path = path.join("mapkeeper.toml");

    let registry = match world_io::load_projects() {
        Ok(file) => file,
        Err(error) => return registry_error(error),
    };
    match world_io::classify_create_marker_at(&path, &registry) {
        world_io::CreateMarkerClass::SafeIncomplete => {
            if let Err(error) = world_io::cleanup_incomplete_create(&path, &registry) {
                return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
            }
        }
        world_io::CreateMarkerClass::CompleteRegistered { .. }
        | world_io::CreateMarkerClass::CompleteUnregistered { .. } => {
            return (
                StatusCode::CONFLICT,
                format!("{} already contains a world", path.display()),
            )
                .into_response();
        }
        world_io::CreateMarkerClass::Ambiguous { reason } => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("create_incomplete: recovery_required ({reason})"),
            )
                .into_response();
        }
        world_io::CreateMarkerClass::NoMarker => {}
    }

    if manifest_path.exists() {
        return (
            StatusCode::CONFLICT,
            format!("{} already contains a world", path.display()),
        )
            .into_response();
    }

    if path.exists() {
        let is_empty = std::fs::read_dir(&path)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            return (
                StatusCode::BAD_REQUEST,
                format!(
                    "create_incomplete: existing non-world directory at {}",
                    path.display()
                ),
            )
                .into_response();
        }
    } else if let Err(error) = std::fs::create_dir_all(&path) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
    }

    if let Err(error) = world_io::write_incomplete_marker(&path) {
        let cleanup = world_io::cleanup_after_failed_create_checked(&path);
        let msg = match cleanup {
            Ok(()) => error,
            Err(cleanup_err) => format!("{error}; {cleanup_err}"),
        };
        return (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response();
    }

    let fail = |server: &Arc<ServerState>, path: &Path, msg: String| -> axum::response::Response {
        let cleanup = world_io::cleanup_after_failed_create_checked(path);
        let key = world_io::path_cmp_key(path);
        let forget = forget_registry_only(server, &key);
        let mut full = msg;
        if let Err(cleanup_err) = cleanup {
            full = format!("{full}; {cleanup_err}");
        }
        if let Err(forget_err) = forget {
            full = format!("{full}; registry_forget: {forget_err}");
        }
        (StatusCode::INTERNAL_SERVER_ERROR, full).into_response()
    };

    let map_ref = world_layout::default_first_map_ref();
    let map_path = world_layout::map_abs_path(&path, &map_ref);
    if let Err(error) = std::fs::create_dir_all(&map_path) {
        return fail(
            server,
            &path,
            format!("create_incomplete: maps dir: {error}"),
        );
    }

    if let Err(error) = crate::atomic_io::atomic_replace(
        &manifest_path,
        world::world_manifest_toml(&id, std::slice::from_ref(&map_ref)).as_bytes(),
    ) {
        return fail(
            server,
            &path,
            format!("create_incomplete: world manifest: {error}"),
        );
    }

    if let Err(error) = crate::atomic_io::atomic_replace(
        &map_path.join("map.toml"),
        world::map_manifest_toml(DEFAULT_FIRST_MAP_ID, preset).as_bytes(),
    ) {
        return fail(
            server,
            &path,
            format!("create_incomplete: map manifest: {error}"),
        );
    }

    if let Err(error) = crate::spatial::ensure_spatial_state(&map_path) {
        return fail(
            server,
            &path,
            format!("create_incomplete: spatial: {error}"),
        );
    }

    let entry = ProjectEntry {
        id: id.clone(),
        path: path.display().to_string(),
    };
    if let Err(error) = world_io::mutate_projects(|file| {
        file.projects.retain(|item| item.id != id);
        world_io::upsert_registered(file, entry.clone());
        Ok(())
    }) {
        return fail(
            server,
            &path,
            format!("create_incomplete: registry: {error}"),
        );
    }

    let create_recovery = match world_io::clear_incomplete_marker(&path) {
        Ok(()) => None,
        Err(_) => Some("marker_residual"),
    };

    server.app.lock().unwrap().active = Some(ActiveWorld {
        path: path.clone(),
        id: id.clone(),
        map_path,
        map_id: DEFAULT_FIRST_MAP_ID.to_string(),
    });
    Json(CreateProjectResult {
        id: entry.id,
        path: entry.path,
        create_recovery,
    })
    .into_response()
}
