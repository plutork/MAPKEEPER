//! Platform-neutral rules: cell_id, hex geometry, profile + validation model.
//! No filesystem, UI, Tauri, or browser assumptions — see `server`/`cli`/`web` for I/O.

pub mod build_state;
pub mod cell_id;
pub mod climate;
pub mod coast_distance;
pub mod elevation_gen;
pub mod geology;
pub mod plates;
pub mod hex;
pub mod hydro;
pub mod land_mask;
pub mod layer;
pub mod map_preset;
pub mod profile;
pub mod projects;
pub mod river_flux;
pub mod rivers;
pub mod world;
