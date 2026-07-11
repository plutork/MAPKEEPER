//! Shared server runtime state (D-96 S0).

use std::path::PathBuf;

pub(crate) struct AppState {
    pub(crate) active: Option<ActiveWorld>,
}

pub(crate) struct ActiveWorld {
    pub(crate) path: PathBuf,
    pub(crate) id: String,
}
