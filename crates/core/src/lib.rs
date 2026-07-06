//! Platform-neutral rules: cell_id, hex geometry, profile + validation model.
//! No filesystem, UI, Tauri, or browser assumptions — see `server`/`cli`/`web` for I/O.

pub mod cell_id;
pub mod hex;
pub mod layer;
pub mod map_preset;
pub mod profile;
pub mod projects;
pub mod world;
