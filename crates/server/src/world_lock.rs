//! Per-world write serialization (agent-reliability world-lock).

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use axum::http::{HeaderMap, StatusCode};
use tokio::sync::Mutex as AsyncMutex;

use crate::state::ServerState;
use crate::world_scope::{self, ResolvedWorld, ScopeMode};

/// Lock ordering when a handler touches multiple artifacts for one world:
///
/// 1. `WorldWriteGuard` for `world_id` (covers the whole handler RMW).
/// 2. Inside `world_io` bundles: manifest/catalog JSON before derived dense layers
///    (existing `persist_*` write order — do not interleave other worlds).
///
/// Global `projects.json` updates run outside world guard except create/open/delete
/// which take the target world's guard before folder RMW.
#[derive(Clone)]
pub struct WorldLockManager {
    inner: Arc<Inner>,
}

struct Inner {
    entries: AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    hold_depth: StdMutex<HashMap<String, usize>>,
    #[cfg(test)]
    peak_depth: StdMutex<HashMap<String, usize>>,
}

pub struct WorldWriteGuard {
    world_id: String,
    lock_arc: Arc<AsyncMutex<()>>,
    _guard: tokio::sync::OwnedMutexGuard<()>,
    locks: WorldLockManager,
}

impl WorldLockManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                entries: AsyncMutex::new(HashMap::new()),
                hold_depth: StdMutex::new(HashMap::new()),
                #[cfg(test)]
                peak_depth: StdMutex::new(HashMap::new()),
            }),
        }
    }

    /// Canonical entry: acquire before any world-scoped filesystem RMW.
    pub async fn acquire_write(&self, world_id: &str) -> WorldWriteGuard {
        let lock_arc = {
            let mut entries = self.inner.entries.lock().await;
            entries
                .entry(world_id.to_string())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        let guard = Arc::clone(&lock_arc).lock_owned().await;
        self.record_hold_start(world_id);
        maybe_failpoint_hold().await;
        WorldWriteGuard {
            world_id: world_id.to_string(),
            lock_arc,
            _guard: guard,
            locks: self.clone(),
        }
    }

    fn record_hold_start(&self, world_id: &str) {
        let mut depth = self.inner.hold_depth.lock().unwrap();
        let count = depth.entry(world_id.to_string()).or_insert(0);
        *count += 1;
        #[cfg(test)]
        {
            let mut peak = self.inner.peak_depth.lock().unwrap();
            let peak_count = peak.entry(world_id.to_string()).or_insert(0);
            *peak_count = (*peak_count).max(*count);
        }
    }

    fn record_hold_end(&self, world_id: &str) {
        let mut depth = self.inner.hold_depth.lock().unwrap();
        if let Some(count) = depth.get_mut(world_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                depth.remove(world_id);
            }
        }
    }

    async fn try_cleanup_entry(&self, world_id: &str, lock_arc: &Arc<AsyncMutex<()>>) {
        if Arc::strong_count(lock_arc) != 1 {
            return;
        }
        if lock_arc.try_lock().is_err() {
            return;
        }
        let mut entries = self.inner.entries.lock().await;
        if entries.get(world_id).map(Arc::as_ptr) == Some(Arc::as_ptr(lock_arc))
            && Arc::strong_count(lock_arc) == 1
        {
            entries.remove(world_id);
        }
    }

    async fn cleanup_idle_entries(&self) {
        let mut entries = self.inner.entries.lock().await;
        entries.retain(|_, arc| !(Arc::strong_count(arc) == 1 && arc.try_lock().is_ok()));
    }

    /// Test hook: concurrent write depth for a world (0 when no guard held).
    #[cfg(test)]
    pub fn hold_depth(&self, world_id: &str) -> usize {
        *self
            .inner
            .hold_depth
            .lock()
            .unwrap()
            .get(world_id)
            .unwrap_or(&0)
    }

    #[cfg(test)]
    pub fn peak_hold_depth(&self, world_id: &str) -> usize {
        *self
            .inner
            .peak_depth
            .lock()
            .unwrap()
            .get(world_id)
            .unwrap_or(&0)
    }

    #[cfg(test)]
    pub fn reset_peak_depth(&self, world_id: &str) {
        self.inner.peak_depth.lock().unwrap().remove(world_id);
    }
}

impl Drop for WorldWriteGuard {
    fn drop(&mut self) {
        self.locks.record_hold_end(&self.world_id);
        let locks = self.locks.clone();
        let world_id = self.world_id.clone();
        let lock_arc = Arc::clone(&self.lock_arc);
        tokio::spawn(async move {
            locks.try_cleanup_entry(&world_id, &lock_arc).await;
            locks.cleanup_idle_entries().await;
        });
    }
}

/// Resolve mutate scope and acquire the world write guard (no `app` mutex during I/O).
pub async fn resolve_mutate_and_guard(
    server: &Arc<ServerState>,
    headers: &HeaderMap,
) -> Result<(ResolvedWorld, WorldWriteGuard), (StatusCode, String)> {
    let world = world_scope::resolve_world(&server.app, headers, ScopeMode::Mutate)?;
    let guard = server.world_locks.acquire_write(&world.id).await;
    Ok((world, guard))
}

async fn maybe_failpoint_hold() {
    if std::env::var("MAPKEEPER_FAILPOINT").ok().as_deref() != Some("world_write_hold") {
        return;
    }
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn same_world_writes_do_not_overlap_in_critical_section() {
        let manager = WorldLockManager::new();
        let world_id = "lock-test-a";
        manager.reset_peak_depth(world_id);

        let m1 = manager.clone();
        let m2 = manager.clone();
        let id_a = world_id.to_string();
        let id_b = world_id.to_string();
        let h1 = tokio::spawn(async move {
            let _g = m1.acquire_write(&id_a).await;
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        });
        let h2 = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            let _g = m2.acquire_write(&id_b).await;
        });
        let _ = tokio::join!(h1, h2);
        assert_eq!(manager.peak_hold_depth(world_id), 1);
    }

    #[tokio::test]
    async fn different_worlds_write_in_parallel() {
        let manager = WorldLockManager::new();
        let started = std::time::Instant::now();
        let m1 = manager.clone();
        let m2 = manager.clone();
        let h1 = tokio::spawn(async move {
            let _g = m1.acquire_write("lock-par-a").await;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        });
        let h2 = tokio::spawn(async move {
            let _g = m2.acquire_write("lock-par-b").await;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        });
        let _ = tokio::join!(h1, h2);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(90),
            "different worlds should not serialize writes"
        );
    }

    #[tokio::test]
    async fn guard_releases_after_drop_on_error_path() {
        let manager = WorldLockManager::new();
        let world_id = "lock-release";
        {
            let _g = manager.acquire_write(world_id).await;
            assert_eq!(manager.hold_depth(world_id), 1);
        }
        tokio::task::yield_now().await;
        assert_eq!(manager.hold_depth(world_id), 0);
        let _g2 = manager.acquire_write(world_id).await;
        assert_eq!(manager.hold_depth(world_id), 1);
    }
}
