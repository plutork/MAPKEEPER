use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mapkeeper_core::spatial::Axial;

#[derive(Clone)]
pub(crate) struct ActiveWorld {
    /// World folder (registry / transfer unit). N-035.
    pub(crate) path: PathBuf,
    pub(crate) id: String,
    /// Active map folder (`maps/<id>`). Spatial I/O targets this path.
    pub(crate) map_path: PathBuf,
    pub(crate) map_id: String,
}

pub(crate) struct AppState {
    pub(crate) active: Option<ActiveWorld>,
}

/// In-memory stroke staging — chunks never touch disk (N-025).
pub(crate) struct StrokeStaging {
    pub(crate) world_key: String,
    pub(crate) base_revision: u64,
    /// Last write wins per axial cell.
    pub(crate) cells: HashMap<Axial, i32>,
    pub(crate) chunk_ids: HashSet<String>,
    pub(crate) created_at: Instant,
}

/// Process-local stroke_id replay guard (not durable; not a revision).
pub(crate) struct RecentCommittedStroke {
    pub(crate) world_key: String,
    pub(crate) recorded_at: Instant,
}

pub struct ServerState {
    pub(crate) app: Mutex<AppState>,
    /// Per-world mutation locks (process-local; N-025).
    world_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    pub(crate) strokes: Mutex<HashMap<String, StrokeStaging>>,
    /// Process-local idempotent commit replay: stroke_id → world key.
    pub(crate) recent_committed_strokes: Mutex<HashMap<String, RecentCommittedStroke>>,
}

pub(crate) const STROKE_STAGING_TTL: Duration = Duration::from_secs(600);
pub(crate) const COMMITTED_STROKE_TTL: Duration = Duration::from_secs(600);

impl ServerState {
    pub fn new(active: Option<ActiveWorld>) -> Self {
        Self {
            app: Mutex::new(AppState { active }),
            world_locks: Mutex::new(HashMap::new()),
            strokes: Mutex::new(HashMap::new()),
            recent_committed_strokes: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn world_lock(&self, world_key: &str) -> Arc<Mutex<()>> {
        let mut map = self.world_locks.lock().unwrap();
        map.entry(world_key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub(crate) fn purge_stale_strokes(&self) {
        let mut strokes = self.strokes.lock().unwrap();
        strokes.retain(|_, s| s.created_at.elapsed() < STROKE_STAGING_TTL);
        let mut committed = self.recent_committed_strokes.lock().unwrap();
        committed.retain(|_, c| c.recorded_at.elapsed() < COMMITTED_STROKE_TTL);
    }
}
