//! Delete saga: inflight marker, registry removal, trash, rollback (N-025 / N-031).

use std::path::Path;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use mapkeeper_core::world;

use super::{clear_active_if_key, registry_error, DeleteProjectInput};
use crate::state::ServerState;
use crate::world_io;

pub(super) fn delete_world(
    server: &Arc<ServerState>,
    input: DeleteProjectInput,
) -> axum::response::Response {
    // Mutation entry: surface prior interrupted Delete before starting a new one.
    if let Err(error) = world_io::reconcile_delete_inflights() {
        return (StatusCode::UNPROCESSABLE_ENTITY, error).into_response();
    }

    let path = world_io::normalize_world_path(Path::new(&input.path));
    let key = world_io::path_cmp_key(&path);
    let expected_id = input.expected_id.trim();
    if expected_id.is_empty() || !world::is_valid_world_id(expected_id) {
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: expected_id required".to_string(),
        )
            .into_response();
    }

    let file = match world_io::load_projects() {
        Ok(file) => file,
        Err(error) => return registry_error(error),
    };

    let registered = world_io::find_registered(&file, &path).cloned();
    if registered.is_none() {
        // Idempotent: already removed from registry and disk (successful prior Delete).
        if !path.exists() {
            clear_active_if_key(server, &key);
            if let Err(error) = world_io::clear_delete_inflight(&key) {
                eprintln!("mapkeeper: delete_recovery: inflight_clear_failed: {error}");
            }
            return StatusCode::NO_CONTENT.into_response();
        }
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: world is not registered".to_string(),
        )
            .into_response();
    }
    let registered = registered.unwrap();

    if registered.id != expected_id {
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: expected_id does not match registry".to_string(),
        )
            .into_response();
    }

    // Always operate on the canonical registered path (reject planted aliases).
    let registered_path = world_io::normalize_world_path(Path::new(&registered.path));
    let registered_key = world_io::path_cmp_key(&registered_path);
    if key != registered_key {
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: path does not match registry entry".to_string(),
        )
            .into_response();
    }

    let manifest_id = match world_io::read_manifest_id(&registered_path) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "delete_rejected: target is not a mapkeeper world workspace".to_string(),
            )
                .into_response();
        }
    };
    if manifest_id != expected_id {
        return (
            StatusCode::BAD_REQUEST,
            "delete_rejected: expected_id does not match manifest".to_string(),
        )
            .into_response();
    }

    let inflight = world_io::DeleteInflight {
        key: key.clone(),
        id: registered.id.clone(),
        path: registered.path.clone(),
    };
    if let Err(error) = world_io::write_delete_inflight(&inflight) {
        return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
    }

    // Registry first so Home never points at a mid-delete path.
    if let Err(error) = world_io::mutate_projects(|next| {
        next.projects
            .retain(|item| world_io::path_cmp_key(Path::new(&item.path)) != key);
        Ok(())
    }) {
        if let Err(clear_err) = world_io::clear_delete_inflight(&key) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{error}; inflight_clear_failed: {clear_err}"),
            )
                .into_response();
        }
        return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
    }

    match world_io::move_world_to_trash(&registered_path, expected_id) {
        Ok(_trash) => {
            clear_active_if_key(server, &key);
            if let Err(error) = world_io::clear_delete_inflight(&key) {
                // World trashed + registry clean — success for author.
                // Residual inflight is recovered at next startup reconcile (not via 204 body).
                eprintln!("mapkeeper: delete_recovery: inflight_clear_failed: {error}");
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(move_error) => {
            #[cfg(test)]
            let force_restore_fail = world_io::take_delete_restore_failpoint();
            #[cfg(not(test))]
            let force_restore_fail = false;

            let restore_result = if force_restore_fail {
                Err("delete_recovery: restore failpoint".to_string())
            } else {
                world_io::mutate_projects(|restore| {
                    world_io::upsert_registered(restore, registered.clone());
                    Ok(())
                })
            };

            match restore_result {
                Ok(()) => {
                    if let Err(clear_err) = world_io::clear_delete_inflight(&key) {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!(
                                "delete_recovery: move_failed_registry_restored; \
                                 inflight_clear_failed: {clear_err}; move: {move_error}"
                            ),
                        )
                            .into_response();
                    }
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("delete_rejected: {move_error}"),
                    )
                        .into_response()
                }
                Err(restore_error) => {
                    // Keep inflight; world on disk; registry missing → startup reconcile.
                    (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        format!(
                            "delete_recovery: registry_rollback_failed ({restore_error}); \
                             move: {move_error}"
                        ),
                    )
                        .into_response()
                }
            }
        }
    }
}
