//! World generation pipeline (D-92).
//!
//! Dependency order — each step may read outputs from earlier steps only:
//! **land** → **coast** / **plates** → **geology** → **elevation** → **climate** → **hydrology**.
//!
//! Spatial primitives (`hex`, `cell_id`, `layer`, …) and I/O adapters (`server`, `web`) stay outside
//! this category.

pub mod climate;
pub mod coast;
pub mod elevation;
pub mod geology;
pub mod hydrology;
pub mod land;
pub mod plates;
