//! mapkeeper desktop shell (roadmap 5.9, D-29) — Tauri wraps the exact same
//! `mapkeeper-server` router and `mapkeeper-web` WASM build; the only thing
//! that changes vs. the browser V0 flow is *how the window opens* (native
//! window here, `http://localhost` instructions there).
//!
//! No sidecar process, no separate binary: the embedded server runs
//! in-process on an OS-assigned ephemeral port (`port: 0`) so it never
//! clashes with a dev server or another mapkeeper instance — an
//! improvement over the fixed dev port, made possible by binding before
//! creating the window (see `setup`).

use std::path::PathBuf;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

/// Resolve the built web UI (wasm-bindgen output) to serve. Installed
/// builds ship it as a bundled resource (`tauri.conf.json` `bundle.resources`);
/// running from the workspace during development falls back to the crate's
/// own build output so `cargo run -p mapkeeper-desktop` works without
/// installing anything first (still requires `crates/web/build.ps1` once).
fn resolve_web_dist(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(resource_path) = app
        .path()
        .resolve("dist", tauri::path::BaseDirectory::Resource)
    {
        if resource_path.exists() {
            return resource_path;
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../web/dist"))
}

#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });
    rx.recv().ok().flatten().map(|path| path.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![pick_folder])
        .setup(|app| {
            let handle = app.handle().clone();
            let web_dist = resolve_web_dist(&handle);
            let config = mapkeeper_server::ServerConfig {
                world: None,
                port: 0,
                web_dist,
            };

            // Bind synchronously (fast — just opens a TCP listener) so the
            // window can be created on the main thread below; only the
            // long-running `axum::serve` loop is spawned to the background.
            let (listener, router) = tauri::async_runtime::block_on(mapkeeper_server::bind(config))
                .expect("failed to bind embedded mapkeeper-server");
            let addr = listener
                .local_addr()
                .expect("bound listener has a local address");
            let url = format!("http://{addr}");

            WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External(url.parse().expect("embedded server URL is always valid")),
            )
            .title("mapkeeper")
            .inner_size(1100.0, 720.0)
            .min_inner_size(720.0, 480.0)
            .build()
            .expect("failed to create the main window");

            tauri::async_runtime::spawn(async move {
                if let Err(err) = axum::serve(listener, router).await {
                    eprintln!("mapkeeper-server (embedded) exited: {err}");
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running mapkeeper-desktop");
}
