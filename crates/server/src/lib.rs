//! Local product-shell server. Map-v2 APIs are archived and are not routed.

mod atomic_io;
mod presets;
mod projects;
mod spatial;
mod state;
mod world_io;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use state::{ActiveWorld, ServerState};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

pub struct ServerConfig {
    pub world: Option<PathBuf>,
    pub port: u16,
    pub web_dist: PathBuf,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    surface: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        surface: "product-shell",
    })
}

pub fn build_router(config: &ServerConfig) -> Result<Router> {
    // Restart recovery for interrupted Delete (N-025). Observable; not tied to GET list.
    match world_io::reconcile_delete_inflights() {
        Ok(notes) => {
            for note in notes {
                eprintln!("mapkeeper: {note}");
            }
        }
        Err(error) => {
            eprintln!("mapkeeper: delete_recovery startup failed: {error}");
        }
    }

    let active = match &config.world {
        Some(path) => Some(ActiveWorld {
            id: world_io::read_manifest_id(path)?,
            path: path.clone(),
        }),
        None => None,
    };
    let state = Arc::new(ServerState::new(active));
    Ok(Router::new()
        .route("/api/health", get(health))
        .merge(projects::routes())
        .merge(presets::routes())
        .merge(spatial::routes())
        .with_state(state)
        .fallback_service(ServeDir::new(&config.web_dist)))
}

pub async fn bind(config: ServerConfig) -> Result<(TcpListener, Router)> {
    let app = build_router(&config)?;
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], config.port))).await?;
    Ok((listener, app))
}

pub async fn run(config: ServerConfig) -> Result<()> {
    let (listener, app) = bind(config).await?;
    println!("mapkeeper shell: http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_identifies_shell_surface() {
        let temp = tempfile::tempdir().unwrap();
        let app = build_router(&ServerConfig {
            world: None,
            port: 0,
            web_dist: temp.path().to_path_buf(),
        })
        .unwrap();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&body)
            .unwrap()
            .contains("\"surface\":\"product-shell\""));
    }
}
