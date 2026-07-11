//! Local server — owns filesystem, world folder, HTTP API, projects list.
//! Calls into mapkeeper-core for rules; HTTP framework choice (axum) is an
//! open implementation detail (not blocking repo layout, D-20) picked here.
//!
//! V0 flow-first slice (roadmap D-21): serves the WASM web UI as static
//! files and a small JSON API so a browser can paint hex cells and save
//! placeholder profiles into one world folder.
//!
//! Launcher slice (roadmap D-12/5.7): with no active world the server
//! starts with no active world; the web UI shows a Home screen backed by
//! `/api/projects` (list/create/open/close) instead of a hex map.
//!
//! Extracted to a library (roadmap 5.9, D-29) so `mapkeeper-desktop` (Tauri)
//! can embed the exact same router in-process instead of re-implementing
//! the API — "Tauri wraps the same frontend build" means it also reuses
//! this same backend, just swaps how the window is opened (native window
//! vs. `open http://localhost` instructions).

mod build;
mod hydrology_diagnostics;
mod lakes;
mod layers;
mod projects;
mod rivers;
mod state;
mod world_io;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::Router;
use mapkeeper_core::hex::MapBounds;
use mapkeeper_core::map_preset::rect_cell_count;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

use state::{ActiveWorld, AppState};

pub struct ServerConfig {
    pub world: Option<PathBuf>,
    pub port: u16,
    pub web_dist: PathBuf,
}

#[derive(serde::Serialize)]
pub(crate) struct MapBoundsResponse {
    kind: String,
    width: i32,
    height: i32,
    cell_count: u32,
}

pub(crate) fn bounds_response(bounds: &MapBounds) -> MapBoundsResponse {
    MapBoundsResponse {
        kind: "hex-rectangle".to_string(),
        width: bounds.width,
        height: bounds.height,
        cell_count: rect_cell_count(bounds.width, bounds.height),
    }
}

pub fn build_router(config: &ServerConfig) -> Result<Router> {
    let active = match &config.world {
        Some(world_path) => {
            let id = world_io::read_manifest_id(world_path)?;
            Some(ActiveWorld {
                path: world_path.clone(),
                id,
            })
        }
        None => None,
    };
    let state = Arc::new(Mutex::new(AppState { active }));

    Ok(projects::routes()
        .merge(build::routes())
        .merge(hydrology_diagnostics::routes())
        .merge(layers::routes())
        .merge(rivers::routes())
        .merge(lakes::routes())
        .with_state(state)
        .fallback_service(ServeDir::new(&config.web_dist)))
}

pub async fn bind(config: ServerConfig) -> Result<(TcpListener, Router)> {
    let app = build_router(&config)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = TcpListener::bind(addr).await?;
    Ok((listener, app))
}

pub async fn run(config: ServerConfig) -> Result<()> {
    let world = config.world.clone();
    let (listener, app) = bind(config).await?;
    let addr = listener.local_addr()?;
    match &world {
        Some(world) => println!(
            "mapkeeper-server: world '{}' at http://{addr}",
            world.display()
        ),
        None => println!("mapkeeper-server: launcher mode at http://{addr}"),
    }
    axum::serve(listener, app).await?;
    Ok(())
}
