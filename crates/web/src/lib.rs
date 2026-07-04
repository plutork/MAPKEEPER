//! WASM UI — calls mapkeeper-core for rules; filesystem goes through the
//! server adapter (V0) or a future Tauri adapter, never direct FS access.
//! WASM framework choice (Leptos/Yew/Dioxus) is open — kept as skeleton.

pub fn placeholder() -> &'static str {
    "mapkeeper-web: skeleton only"
}
