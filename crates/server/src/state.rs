//! Shared server runtime state (D-96 S0).

use std::sync::Mutex;

use crate::world_lock::WorldLockManager;

pub(crate) struct AppState {
    pub(crate) active: Option<ActiveWorld>,
}

#[derive(Clone)]
pub(crate) struct ActiveWorld {
    pub(crate) path: std::path::PathBuf,
    pub(crate) id: String,
}

/// Router state: UI `active` under brief lock; per-world write locks are async.
pub struct ServerState {
    pub(crate) app: Mutex<AppState>,
    pub(crate) world_locks: WorldLockManager,
}

impl ServerState {
    pub fn new(active: Option<ActiveWorld>) -> Self {
        Self {
            app: Mutex::new(AppState { active }),
            world_locks: WorldLockManager::new(),
        }
    }
}
