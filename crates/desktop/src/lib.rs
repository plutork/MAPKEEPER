//! Native product shell over the shared local server and web UI.

use std::path::PathBuf;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

/// Resolve the built web UI for installed and source-run launches.
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
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![pick_folder])
        .setup(|app| {
            let handle = app.handle().clone();
            let web_dist = resolve_web_dist(&handle);
            let config = mapkeeper_server::ServerConfig {
                world: None,
                port: 0,
                web_dist,
            };

            // Bind before creating the native window.
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
            .maximized(true)
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
