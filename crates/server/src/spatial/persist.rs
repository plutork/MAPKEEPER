//! Durable spatial state: load, config sync, atomic write, bak restore (N-025 / N-031).

use std::path::{Path, PathBuf};
use std::time::Instant;

use mapkeeper_core::spatial::{default_spatial_state, SpatialState, SPATIAL_STATE_RELATIVE};
use mapkeeper_core::world::{self, SpatialConfig};

use crate::atomic_io;

#[derive(Debug, Default, Clone, Copy, serde::Serialize)]
pub(super) struct SpatialIoTimings {
    pub read_ms: f64,
    pub parse_ms: f64,
    pub serialize_ms: f64,
    pub atomic_write_ms: f64,
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

pub(super) fn spatial_bak_available(path: &Path) -> bool {
    atomic_io::bak_passes(path, |bytes| {
        let Ok(raw) = std::str::from_utf8(bytes) else {
            return false;
        };
        SpatialState::assert_no_screen_keys(raw).is_ok() && SpatialState::from_json(raw).is_ok()
    })
}

/// Load/init spatial state. Corrupt / interrupted on-disk state is never silently replaced with defaults.
pub fn ensure_spatial_state(world_path: &Path) -> anyhow::Result<SpatialState> {
    ensure_spatial_state_timed(world_path).map(|(state, _)| state)
}

pub(super) fn ensure_spatial_state_timed(
    world_path: &Path,
) -> anyhow::Result<(SpatialState, SpatialIoTimings)> {
    let mut timings = SpatialIoTimings::default();
    let config = ensure_spatial_config(world_path)?;
    let path = spatial_path(world_path);
    let (mut state, legacy_schema) = match atomic_io::classify_durable_open(&path) {
        atomic_io::DurableOpenKind::PrimaryPresent => {
            let read_started = Instant::now();
            let raw = std::fs::read_to_string(&path)?;
            timings.read_ms = elapsed_ms(read_started);
            let parse_started = Instant::now();
            SpatialState::assert_no_screen_keys(&raw).map_err(anyhow::Error::msg)?;
            let legacy = raw.contains("cell_size") || raw.contains("unit_scale");
            let parsed = match SpatialState::from_json(&raw) {
                Ok(state) => (state, legacy),
                Err(error) => {
                    anyhow::bail!(
                        "corrupt_spatial: {} (bak_available={})",
                        error,
                        spatial_bak_available(&path)
                    );
                }
            };
            timings.parse_ms = elapsed_ms(parse_started);
            parsed
        }
        atomic_io::DurableOpenKind::InterruptedWrite => {
            anyhow::bail!(
                "corrupt_spatial: interrupted_write (bak_available={})",
                spatial_bak_available(&path)
            );
        }
        atomic_io::DurableOpenKind::AbsentClean => (default_spatial_state(), false),
    };

    let before = state.clone();
    state.apply_spatial_config(&config);
    if (before.frame != state.frame || before.grid != state.grid)
        && state.geometry_stub.id == "probe"
    {
        state.refresh_geometry_stub_from_probe();
    }

    let needs_write = !path.is_file() || legacy_schema || before != state;
    if needs_write {
        if state.revision == 0 {
            state.revision = 1;
        }
        let write_timings = write_spatial_state_timed(world_path, &state)?;
        timings.serialize_ms += write_timings.serialize_ms;
        timings.atomic_write_ms += write_timings.atomic_write_ms;
    }
    Ok((state, timings))
}

/// Explicit recovery: quarantine corrupt primary, restore from `.bak` (N-025).
pub fn restore_spatial_from_bak(world_path: &Path) -> anyhow::Result<SpatialState> {
    let path = spatial_path(world_path);
    let bak = atomic_io::bak_path(&path);
    if !bak.is_file() {
        anyhow::bail!("corrupt_spatial: no bak available");
    }
    let bak_raw = std::fs::read_to_string(&bak)?;
    SpatialState::assert_no_screen_keys(&bak_raw).map_err(anyhow::Error::msg)?;
    let restored = SpatialState::from_json(&bak_raw)
        .map_err(|e| anyhow::anyhow!("corrupt_spatial: invalid bak: {e}"))?;

    if path.is_file() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let diag = path.with_file_name(format!("state.json.corrupt-{stamp}"));
        std::fs::rename(&path, &diag).or_else(|_| {
            std::fs::copy(&path, &diag)?;
            std::fs::remove_file(&path)?;
            Ok::<(), std::io::Error>(())
        })?;
    }

    write_spatial_state(world_path, &restored)?;
    ensure_spatial_state(world_path)
}

/// Read immutable `[spatial]` from a **map** folder (`map.toml`). N-035 / N-037.
pub(super) fn ensure_spatial_config(map_path: &Path) -> anyhow::Result<SpatialConfig> {
    let manifest_path = map_path.join("map.toml");
    match atomic_io::classify_durable_open(&manifest_path) {
        atomic_io::DurableOpenKind::InterruptedWrite => {
            let bak_available = atomic_io::bak_passes(&manifest_path, |bytes| {
                std::str::from_utf8(bytes)
                    .ok()
                    .and_then(|raw| world::parse_map_manifest(raw).ok())
                    .is_some()
            });
            anyhow::bail!("corrupt_manifest: interrupted_write (bak_available={bak_available})");
        }
        atomic_io::DurableOpenKind::AbsentClean => {
            anyhow::bail!(
                "corrupt_manifest: missing map.toml at {}",
                manifest_path.display()
            );
        }
        atomic_io::DurableOpenKind::PrimaryPresent => {}
    }

    let raw = std::fs::read_to_string(&manifest_path)?;
    let manifest = match world::parse_map_manifest(&raw) {
        Ok(m) => m,
        Err(error) => {
            let bak_available = atomic_io::bak_passes(&manifest_path, |bytes| {
                std::str::from_utf8(bytes)
                    .ok()
                    .and_then(|b| world::parse_map_manifest(b).ok())
                    .is_some()
            });
            anyhow::bail!("corrupt_manifest: {error} (bak_available={bak_available})");
        }
    };
    manifest
        .spatial
        .assert_matches_catalog()
        .map_err(anyhow::Error::msg)?;
    Ok(manifest.spatial)
}

pub(super) fn spatial_path(map_path: &Path) -> PathBuf {
    map_path.join(SPATIAL_STATE_RELATIVE)
}

pub(super) fn write_spatial_state(map_path: &Path, state: &SpatialState) -> anyhow::Result<()> {
    write_spatial_state_timed(map_path, state).map(|_| ())
}

pub(super) fn write_spatial_state_timed(
    map_path: &Path,
    state: &SpatialState,
) -> anyhow::Result<SpatialIoTimings> {
    let mut timings = SpatialIoTimings::default();
    let path = spatial_path(map_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialize_started = Instant::now();
    let raw = state.to_json_pretty()?;
    SpatialState::assert_no_screen_keys(&raw).map_err(anyhow::Error::msg)?;
    timings.serialize_ms = elapsed_ms(serialize_started);
    let write_started = Instant::now();
    atomic_io::atomic_replace(&path, raw.as_bytes())?;
    timings.atomic_write_ms = elapsed_ms(write_started);
    Ok(timings)
}
