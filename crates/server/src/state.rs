use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone)]
pub(crate) struct ActiveWorld {
    pub(crate) path: PathBuf,
    pub(crate) id: String,
}

pub(crate) struct AppState {
    pub(crate) active: Option<ActiveWorld>,
}

pub struct ServerState {
    pub(crate) app: Mutex<AppState>,
}

impl ServerState {
    pub fn new(active: Option<ActiveWorld>) -> Self {
        Self {
            app: Mutex::new(AppState { active }),
        }
    }
}
