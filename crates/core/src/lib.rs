//! Platform-neutral rules: cell_id, hex geometry, profile + validation model.
//! No filesystem, UI, Tauri, or browser assumptions — see `server`/`cli`/`web` for I/O.

pub mod build_state;
pub mod cell_id;
pub mod hex;
pub mod hydro;
pub mod lakes;
pub mod layer;
pub mod map_preset;
pub mod profile;
pub mod projects;
pub mod rivers;
pub mod world;

pub mod worldgen;

// Legacy module paths (D-92) — prefer `worldgen::*` for new pipeline work.
pub mod land_mask {
    pub use crate::worldgen::land::*;
}
pub mod coast_distance {
    pub use crate::worldgen::coast::*;
}
pub mod plates {
    pub use crate::worldgen::plates::*;
}
pub mod geology {
    pub use crate::worldgen::geology::*;
}
pub mod elevation_gen {
    pub use crate::worldgen::elevation::*;
}
pub mod climate {
    pub use crate::worldgen::climate::*;
}
pub mod river_flux {
    pub use crate::worldgen::hydrology::river_flux::*;
}
