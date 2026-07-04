//! Local server — owns filesystem, world folder, HTTP API, projects list.
//! Calls into mapkeeper-core for rules; HTTP framework choice (axum) is an
//! open implementation detail (not blocking repo layout, D-20) picked here.
//!
//! V0 flow-first slice (roadmap D-21): serves the WASM web UI as static
//! files and a small JSON API so a browser can paint hex cells and save
//! placeholder profiles into one world folder.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use mapkeeper_core::cell_id::CellId;
use mapkeeper_core::profile::CellProfile;
use serde::{Deserialize, Serialize};
use tower_http::services::ServeDir;

#[derive(Parser)]
#[command(name = "mapkeeper-server", version, about)]
struct Args {
    /// World folder to serve — must contain `mapkeeper.toml` (see `mapkeeper init`).
    #[arg(long, default_value = ".")]
    world: PathBuf,
    #[arg(long, default_value_t = 4000)]
    port: u16,
    /// Built web UI (wasm-bindgen output) to serve as static files.
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/dist"))]
    web_dist: PathBuf,
}

struct AppState {
    world_path: PathBuf,
    world_id: String,
}

#[derive(Deserialize)]
struct Manifest {
    world: WorldSection,
}

#[derive(Deserialize)]
struct WorldSection {
    id: String,
}

#[derive(Serialize)]
struct CellSummary {
    cell_id: String,
    q: i32,
    r: i32,
    display_name: String,
}

#[derive(Serialize)]
struct MapResponse {
    world_id: String,
    cells: Vec<CellSummary>,
}

#[derive(Deserialize)]
struct ProfileInput {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    notes: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let manifest_path = args.world.join("mapkeeper.toml");
    let manifest_raw = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "reading {} — is this a mapkeeper world? (see `mapkeeper init`)",
            manifest_path.display()
        )
    })?;
    let manifest: Manifest = toml::from_str(&manifest_raw).context("parsing mapkeeper.toml")?;

    let state = Arc::new(AppState {
        world_path: args.world.clone(),
        world_id: manifest.world.id,
    });

    let app = Router::new()
        .route("/api/map", get(get_map))
        .route("/api/cells/:q/:r/profile", get(get_profile).put(put_profile))
        .with_state(state)
        .fallback_service(ServeDir::new(&args.web_dist));

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    println!("mapkeeper-server: world '{}' at http://{addr}", args.world.display());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn profiles_dir(state: &AppState) -> PathBuf {
    state.world_path.join("profiles")
}

fn profile_path(state: &AppState, q: i32, r: i32) -> PathBuf {
    let id = CellId::new(&state.world_id, q, r);
    profiles_dir(state).join(id.filename())
}

async fn get_map(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let dir = profiles_dir(&state);
    let mut cells = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else { continue };
            let Ok(profile) = serde_json::from_str::<CellProfile>(&raw) else { continue };
            let Some(id) = CellId::parse(&profile.cell_id) else { continue };
            cells.push(CellSummary {
                cell_id: profile.cell_id,
                q: id.q,
                r: id.r,
                display_name: profile.display_name,
            });
        }
    }
    Json(MapResponse { world_id: state.world_id.clone(), cells })
}

async fn get_profile(
    State(state): State<Arc<AppState>>,
    AxPath((q, r)): AxPath<(i32, i32)>,
) -> impl IntoResponse {
    let id = CellId::new(&state.world_id, q, r);
    let path = profile_path(&state, q, r);
    let profile = match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(profile) => profile,
            Err(err) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
            }
        },
        Err(_) => CellProfile::new(&id, ""),
    };
    Json(profile).into_response()
}

async fn put_profile(
    State(state): State<Arc<AppState>>,
    AxPath((q, r)): AxPath<(i32, i32)>,
    Json(input): Json<ProfileInput>,
) -> impl IntoResponse {
    let id = CellId::new(&state.world_id, q, r);
    let mut profile = CellProfile::new(&id, input.display_name);
    profile.notes = input.notes;

    let issues = profile.validate();
    if issues.iter().any(|i| matches!(i, mapkeeper_core::profile::ValidationIssue::Error(_))) {
        return (StatusCode::BAD_REQUEST, format!("{issues:?}")).into_response();
    }

    let dir = profiles_dir(&state);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }
    let path = profile_path(&state, q, r);
    let body = match serde_json::to_string_pretty(&profile) {
        Ok(body) => body,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    };
    if let Err(err) = std::fs::write(&path, body) {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }
    Json(profile).into_response()
}
