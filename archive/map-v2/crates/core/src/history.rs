//! Authored world history — WorldState timeline + domain CoW (D-107 track A).
//! Separate from `map.manifest.revision` (technical OCC).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const HISTORY_MANIFEST_FILE: &str = "history/manifest.json";
pub const CANONICAL_DOMAIN_REF: &str = "@canonical";

pub const DOMAIN_LAND: &str = "land";
pub const DOMAIN_GEOLOGY: &str = "geology";
pub const DOMAIN_CLIMATE: &str = "climate";
pub const DOMAIN_WATER: &str = "water";

pub const ALL_DOMAINS: &[&str] = &[DOMAIN_LAND, DOMAIN_GEOLOGY, DOMAIN_CLIMATE, DOMAIN_WATER];

/// Baseline world state id at unlock.
pub const BASELINE_STATE_ID: &str = "ws-0000";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryManifest {
    pub schema_version: u32,
    pub enabled: bool,
    pub selected_state_id: String,
    pub states: Vec<WorldStateRecord>,
    /// Forked domain bundles keyed by ref id (not `@canonical`).
    #[serde(default)]
    pub domain_bundles: HashMap<String, DomainBundleMeta>,
    /// Lore / cataclysm markers (D-107 track B).
    #[serde(default)]
    pub events: Vec<HistoricalEventRecord>,
    #[serde(default)]
    pub change_sets: Vec<ChangeSetRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomainBundleMeta {
    pub domain: String,
    #[serde(default)]
    pub forked_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorldStateRecord {
    pub id: String,
    pub time_key: i64,
    pub display_date: String,
    pub name: String,
    #[serde(default)]
    pub based_on: Option<String>,
    #[serde(default)]
    pub locked: bool,
    pub domain_refs: HashMap<String, String>,
    /// Domains needing author review after ancestor fork (not ordinary stale).
    #[serde(default)]
    pub history_divergence: Vec<String>,
}

/// Dated lore entry — map optional via linked WorldState (not description alone).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalEventRecord {
    pub id: String,
    pub time_key: i64,
    pub display_date: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub anchor_state_id: Option<String>,
    #[serde(default)]
    pub change_set_id: Option<String>,
    #[serde(default)]
    pub result_state_id: Option<String>,
}

/// Structured diff between two WorldStates — summary only; Result state holds SoT.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeSetRecord {
    pub id: String,
    pub from_state_id: String,
    pub to_state_id: String,
    #[serde(default)]
    pub changed_domains: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

impl Default for HistoryManifest {
    fn default() -> Self {
        Self {
            schema_version: 1,
            enabled: false,
            selected_state_id: BASELINE_STATE_ID.to_string(),
            states: Vec::new(),
            domain_bundles: HashMap::new(),
            events: Vec::new(),
            change_sets: Vec::new(),
        }
    }
}

impl HistoryManifest {
    pub fn event_by_id(&self, id: &str) -> Option<&HistoricalEventRecord> {
        self.events.iter().find(|e| e.id == id)
    }

    pub fn change_set_by_id(&self, id: &str) -> Option<&ChangeSetRecord> {
        self.change_sets.iter().find(|c| c.id == id)
    }

    fn next_event_id(&self) -> String {
        let n = self.events.len() + 1;
        format!("evt-{n}")
    }

    fn next_changeset_id(&self) -> String {
        let n = self.change_sets.len() + 1;
        format!("cs-{n}")
    }

    pub fn baseline_state(&self) -> Option<&WorldStateRecord> {
        self.states.iter().find(|s| s.id == BASELINE_STATE_ID)
    }

    pub fn selected_state(&self) -> Option<&WorldStateRecord> {
        self.states
            .iter()
            .find(|s| s.id == self.selected_state_id)
    }

    pub fn selected_state_mut(&mut self) -> Option<&mut WorldStateRecord> {
        let id = self.selected_state_id.clone();
        self.states.iter_mut().find(|s| s.id == id)
    }

    pub fn state_by_id(&self, id: &str) -> Option<&WorldStateRecord> {
        self.states.iter().find(|s| s.id == id)
    }

    pub fn state_by_id_mut(&mut self, id: &str) -> Option<&mut WorldStateRecord> {
        self.states.iter_mut().find(|s| s.id == id)
    }

    pub fn canonical_domain_refs() -> HashMap<String, String> {
        ALL_DOMAINS
            .iter()
            .map(|d| ((*d).to_string(), CANONICAL_DOMAIN_REF.to_string()))
            .collect()
    }
}

pub fn history_manifest_path(world_path: &Path) -> PathBuf {
    world_path.join(HISTORY_MANIFEST_FILE)
}

pub fn read_history_manifest(world_path: &Path) -> HistoryManifest {
    let path = history_manifest_path(world_path);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return HistoryManifest::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn write_history_manifest(world_path: &Path, manifest: &HistoryManifest) -> Result<(), String> {
    let path = history_manifest_path(world_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
}

pub fn history_enabled(world_path: &Path) -> bool {
    read_history_manifest(world_path).enabled
}

/// Map layer id → history domain (track A: land fully wired; others reserved).
pub fn domain_for_layer(layer_id: &str) -> Option<&'static str> {
    match layer_id {
        "land_mask" | "elevation" => Some(DOMAIN_LAND),
        "geology" => Some(DOMAIN_GEOLOGY),
        "temperature" | "precipitation" | "ice" | "coast_distance" => Some(DOMAIN_CLIMATE),
        "river_id" | "lake_id" => Some(DOMAIN_WATER),
        _ => None,
    }
}

pub fn layer_ids_in_domain(domain: &str) -> &'static [&'static str] {
    match domain {
        DOMAIN_LAND => &["land_mask", "elevation"],
        DOMAIN_GEOLOGY => &["geology"],
        DOMAIN_CLIMATE => &["temperature", "precipitation", "ice", "coast_distance"],
        DOMAIN_WATER => &["river_id", "lake_id"],
        _ => &[],
    }
}

pub fn domain_bundle_dir(world_path: &Path, bundle_ref: &str) -> PathBuf {
    world_path
        .join("history")
        .join("domains")
        .join(bundle_ref)
}

pub fn canonical_layer_path(world_path: &Path, layer_id: &str) -> PathBuf {
    world_path
        .join("map")
        .join("layers")
        .join(format!("{layer_id}.json"))
}

pub fn resolve_layer_path(
    world_path: &Path,
    manifest: &HistoryManifest,
    state_id: &str,
    layer_id: &str,
) -> PathBuf {
    let Some(domain) = domain_for_layer(layer_id) else {
        return canonical_layer_path(world_path, layer_id);
    };
    let Some(state) = manifest.state_by_id(state_id) else {
        return canonical_layer_path(world_path, layer_id);
    };
    let domain_ref = state
        .domain_refs
        .get(domain)
        .map(String::as_str)
        .unwrap_or(CANONICAL_DOMAIN_REF);
    if domain_ref == CANONICAL_DOMAIN_REF {
        canonical_layer_path(world_path, layer_id)
    } else {
        domain_bundle_dir(world_path, domain_ref)
            .join("layers")
            .join(format!("{layer_id}.json"))
    }
}

fn next_bundle_id(manifest: &HistoryManifest, domain: &str) -> String {
    let n = manifest
        .domain_bundles
        .keys()
        .filter(|k| k.starts_with(&format!("{domain}-")))
        .count()
        + 1;
    format!("{domain}-{n}")
}

fn copy_file_if_exists(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.is_file() {
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(src, dst).map_err(|e| e.to_string())?;
    Ok(())
}

/// Copy all layer files for a domain from canonical map into a new bundle.
pub fn fork_domain_bundle(
    world_path: &Path,
    manifest: &mut HistoryManifest,
    state_id: &str,
    domain: &str,
) -> Result<String, String> {
    let old_ref = manifest
        .state_by_id(state_id)
        .and_then(|s| s.domain_refs.get(domain))
        .cloned()
        .unwrap_or_else(|| CANONICAL_DOMAIN_REF.to_string());

    if old_ref != CANONICAL_DOMAIN_REF {
        return Ok(old_ref);
    }

    let new_ref = next_bundle_id(manifest, domain);
    let bundle_dir = domain_bundle_dir(world_path, &new_ref).join("layers");
    for layer_id in layer_ids_in_domain(domain) {
        let src = if old_ref == CANONICAL_DOMAIN_REF {
            canonical_layer_path(world_path, layer_id)
        } else {
            domain_bundle_dir(world_path, &old_ref)
                .join("layers")
                .join(format!("{layer_id}.json"))
        };
        let dst = bundle_dir.join(format!("{layer_id}.json"));
        copy_file_if_exists(&src, &dst)?;
    }

    manifest.domain_bundles.insert(
        new_ref.clone(),
        DomainBundleMeta {
            domain: domain.to_string(),
            forked_from: Some(old_ref.clone()),
        },
    );

    if let Some(state) = manifest.state_by_id_mut(state_id) {
        state.domain_refs.insert(domain.to_string(), new_ref.clone());
    }

    mark_divergence_after_fork(manifest, state_id, domain, &old_ref);

    Ok(new_ref)
}

/// Descendants still on `old_ref` get history_divergence (D-107 2C).
pub fn mark_divergence_after_fork(
    manifest: &mut HistoryManifest,
    forked_state_id: &str,
    domain: &str,
    old_ref: &str,
) {
    if old_ref != CANONICAL_DOMAIN_REF && !old_ref.is_empty() {
        // Only canonical-shared refs trigger divergence in track A.
    }
    for state in &mut manifest.states {
        if state.id == forked_state_id {
            continue;
        }
        let still_on_old = state
            .domain_refs
            .get(domain)
            .map(String::as_str)
            .unwrap_or(CANONICAL_DOMAIN_REF)
            == old_ref;
        if still_on_old && !state.history_divergence.contains(&domain.to_string()) {
            state.history_divergence.push(domain.to_string());
        }
    }
}

pub fn unlock_history(world_path: &Path) -> Result<HistoryManifest, String> {
    let existing = read_history_manifest(world_path);
    if existing.enabled {
        return Ok(existing);
    }
    let manifest = HistoryManifest {
        schema_version: 1,
        enabled: true,
        selected_state_id: BASELINE_STATE_ID.to_string(),
        states: vec![WorldStateRecord {
            id: BASELINE_STATE_ID.to_string(),
            time_key: 0,
            display_date: "0000".to_string(),
            name: "Current Age".to_string(),
            based_on: None,
            locked: false,
            domain_refs: HistoryManifest::canonical_domain_refs(),
            history_divergence: Vec::new(),
        }],
        domain_bundles: HashMap::new(),
        events: Vec::new(),
        change_sets: Vec::new(),
    };
    write_history_manifest(world_path, &manifest)?;
    Ok(manifest)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStateInput {
    pub time_key: i64,
    pub display_date: String,
    pub name: String,
    pub based_on: String,
    pub direction: String, // "earlier" | "later"
}

pub fn add_world_state(
    manifest: &mut HistoryManifest,
    input: &CreateStateInput,
) -> Result<WorldStateRecord, String> {
    let base = manifest
        .state_by_id(&input.based_on)
        .ok_or_else(|| "based_on state not found".to_string())?
        .clone();
    let id = if input.time_key < 0 {
        format!("ws-n{}", -input.time_key)
    } else {
        format!("ws-{}", input.time_key)
    };
    if manifest.state_by_id(&id).is_some() {
        return Err(format!("state id {id} already exists"));
    }
    let state = WorldStateRecord {
        id,
        time_key: input.time_key,
        display_date: input.display_date.clone(),
        name: input.name.clone(),
        based_on: Some(input.based_on.clone()),
        locked: false,
        domain_refs: base.domain_refs.clone(),
        history_divergence: Vec::new(),
    };
    manifest.states.push(state.clone());
    manifest
        .states
        .sort_by_key(|s| (s.time_key, s.id.clone()));
    Ok(state)
}

pub fn ensure_domain_forked_for_write(
    world_path: &Path,
    manifest: &mut HistoryManifest,
    state_id: &str,
    domain: &str,
) -> Result<(), String> {
    let needs_fork = manifest
        .state_by_id(state_id)
        .and_then(|s| s.domain_refs.get(domain))
        .map(|r| r.as_str())
        == Some(CANONICAL_DOMAIN_REF);
    if needs_fork {
        fork_domain_bundle(world_path, manifest, state_id, domain)?;
    }
    Ok(())
}

pub fn selected_state_locked(manifest: &HistoryManifest) -> bool {
    manifest
        .selected_state()
        .map(|s| s.locked)
        .unwrap_or(false)
}

pub fn selected_divergence_domains(manifest: &HistoryManifest) -> Vec<String> {
    manifest
        .selected_state()
        .map(|s| s.history_divergence.clone())
        .unwrap_or_default()
}

/// Domains whose bundle refs differ between two states.
pub fn compute_changed_domains(from: &WorldStateRecord, to: &WorldStateRecord) -> Vec<String> {
    ALL_DOMAINS
        .iter()
        .filter(|d| {
            let a = from
                .domain_refs
                .get(**d)
                .map(String::as_str)
                .unwrap_or(CANONICAL_DOMAIN_REF);
            let b = to
                .domain_refs
                .get(**d)
                .map(String::as_str)
                .unwrap_or(CANONICAL_DOMAIN_REF);
            a != b
        })
        .map(|d| (*d).to_string())
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEventInput {
    pub time_key: i64,
    pub display_date: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub anchor_state_id: Option<String>,
}

/// Lore-only marker — no new WorldState (D-107 track B).
pub fn add_lore_event(
    manifest: &mut HistoryManifest,
    input: &CreateEventInput,
) -> Result<HistoricalEventRecord, String> {
    if let Some(anchor) = &input.anchor_state_id {
        if manifest.state_by_id(anchor).is_none() {
            return Err("anchor_state_id not found".to_string());
        }
    }
    let event = HistoricalEventRecord {
        id: manifest.next_event_id(),
        time_key: input.time_key,
        display_date: input.display_date.clone(),
        name: input.name.clone(),
        description: input.description.clone(),
        anchor_state_id: input.anchor_state_id.clone(),
        change_set_id: None,
        result_state_id: None,
    };
    manifest.events.push(event.clone());
    manifest
        .events
        .sort_by_key(|e| (e.time_key, e.id.clone()));
    Ok(event)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCataclysmInput {
    pub time_key: i64,
    pub display_date: String,
    pub event_name: String,
    #[serde(default)]
    pub description: String,
    pub result_state_name: String,
    pub based_on: String,
    #[serde(default)]
    pub changed_domains: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CataclysmResult {
    pub event: HistoricalEventRecord,
    pub change_set: ChangeSetRecord,
    pub result_state: WorldStateRecord,
}

/// Event + ChangeSet + Result WorldState in one step (author-facing).
pub fn create_cataclysm(
    manifest: &mut HistoryManifest,
    input: &CreateCataclysmInput,
) -> Result<CataclysmResult, String> {
    let from = manifest
        .state_by_id(&input.based_on)
        .ok_or_else(|| "based_on state not found".to_string())?
        .clone();
    let result_state = add_world_state(
        manifest,
        &CreateStateInput {
            time_key: input.time_key,
            display_date: input.display_date.clone(),
            name: input.result_state_name.clone(),
            based_on: input.based_on.clone(),
            direction: "later".to_string(),
        },
    )?;
    let computed = compute_changed_domains(&from, &result_state);
    let changed_domains = if input.changed_domains.is_empty() {
        computed
    } else {
        input.changed_domains.clone()
    };
    let change_set = ChangeSetRecord {
        id: manifest.next_changeset_id(),
        from_state_id: from.id.clone(),
        to_state_id: result_state.id.clone(),
        changed_domains,
        notes: input.notes.clone(),
    };
    manifest.change_sets.push(change_set.clone());
    let event = HistoricalEventRecord {
        id: manifest.next_event_id(),
        time_key: input.time_key,
        display_date: input.display_date.clone(),
        name: input.event_name.clone(),
        description: input.description.clone(),
        anchor_state_id: Some(from.id.clone()),
        change_set_id: Some(change_set.id.clone()),
        result_state_id: Some(result_state.id.clone()),
    };
    manifest.events.push(event.clone());
    manifest
        .events
        .sort_by_key(|e| (e.time_key, e.id.clone()));
    Ok(CataclysmResult {
        event,
        change_set,
        result_state,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomainRefSummary {
    pub domain: String,
    pub local_ref: String,
    #[serde(default)]
    pub fork_source_state_id: Option<String>,
    #[serde(default)]
    pub fork_source_state_name: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DivergenceStateReview {
    pub state_id: String,
    pub display_date: String,
    pub name: String,
    pub domains: Vec<DomainRefSummary>,
}

fn domain_ref_of(state: &WorldStateRecord, domain: &str) -> String {
    state
        .domain_refs
        .get(domain)
        .cloned()
        .unwrap_or_else(|| CANONICAL_DOMAIN_REF.to_string())
}

/// Nearest ancestor whose domain ref differs from `state` (fork source).
pub fn fork_source_for_domain<'a>(
    manifest: &'a HistoryManifest,
    state_id: &str,
    domain: &str,
) -> Option<&'a WorldStateRecord> {
    let mut current = manifest.state_by_id(state_id)?;
    let local = domain_ref_of(current, domain);
    while let Some(parent_id) = current.based_on.as_deref() {
        let parent = manifest.state_by_id(parent_id)?;
        if domain_ref_of(parent, domain) != local {
            return Some(parent);
        }
        current = parent;
    }
    None
}

pub fn domain_divergence_detail(
    manifest: &HistoryManifest,
    state_id: &str,
    domain: &str,
) -> Option<DomainRefSummary> {
    let state = manifest.state_by_id(state_id)?;
    if !state.history_divergence.contains(&domain.to_string()) {
        return None;
    }
    let local_ref = domain_ref_of(state, domain);
    let fork = fork_source_for_domain(manifest, state_id, domain);
    let message = if let Some(src) = fork {
        format!(
            "Ancestor \"{}\" forked {domain}; this state still inherits {local_ref}",
            src.name
        )
    } else {
        format!("Cross-epoch divergence on {domain} (ref {local_ref})")
    };
    Some(DomainRefSummary {
        domain: domain.to_string(),
        local_ref,
        fork_source_state_id: fork.map(|s| s.id.clone()),
        fork_source_state_name: fork.map(|s| s.name.clone()),
        message,
    })
}

/// All states with `history_divergence` markers + per-domain ref explanation.
pub fn build_divergence_review(manifest: &HistoryManifest) -> Vec<DivergenceStateReview> {
    let mut out = Vec::new();
    for state in &manifest.states {
        if state.history_divergence.is_empty() {
            continue;
        }
        let domains: Vec<DomainRefSummary> = state
            .history_divergence
            .iter()
            .filter_map(|d| domain_divergence_detail(manifest, &state.id, d))
            .collect();
        out.push(DivergenceStateReview {
            state_id: state.id.clone(),
            display_date: state.display_date.clone(),
            name: state.name.clone(),
            domains,
        });
    }
    out.sort_by(|a, b| a.display_date.cmp(&b.display_date));
    out
}

/// Author keeps inherited ref — clear review marker only (no silent cascade).
pub fn ack_divergence(
    manifest: &mut HistoryManifest,
    state_id: &str,
    domains: Option<&[String]>,
) -> Result<(), String> {
    let state = manifest
        .state_by_id_mut(state_id)
        .ok_or_else(|| "state not found".to_string())?;
    match domains {
        Some(list) => {
            state.history_divergence.retain(|d| !list.contains(d));
        }
        None => state.history_divergence.clear(),
    }
    Ok(())
}

/// Explicit rebase — copy nearest fork-source domain ref; clear marker.
pub fn rebase_domain(
    manifest: &mut HistoryManifest,
    state_id: &str,
    domain: &str,
) -> Result<(), String> {
    if !ALL_DOMAINS.contains(&domain) {
        return Err(format!("unknown domain {domain}"));
    }
    let source = fork_source_for_domain(manifest, state_id, domain)
        .ok_or_else(|| "no fork source found for rebase".to_string())?
        .clone();
    let new_ref = domain_ref_of(&source, domain);
    let state = manifest
        .state_by_id_mut(state_id)
        .ok_or_else(|| "state not found".to_string())?;
    if !state.history_divergence.contains(&domain.to_string()) {
        return Err(format!("domain {domain} is not marked diverged"));
    }
    state.domain_refs.insert(domain.to_string(), new_ref);
    state.history_divergence.retain(|d| d != domain);
    Ok(())
}

pub fn state_has_descendants(manifest: &HistoryManifest, state_id: &str) -> bool {
    manifest
        .states
        .iter()
        .any(|s| s.based_on.as_deref() == Some(state_id))
}

pub fn delete_world_state(manifest: &mut HistoryManifest, state_id: &str) -> Result<(), String> {
    if state_id == BASELINE_STATE_ID {
        return Err("cannot delete baseline world state".to_string());
    }
    if manifest.selected_state_id == state_id {
        return Err("cannot delete selected world state".to_string());
    }
    if state_has_descendants(manifest, state_id) {
        return Err("cannot delete state with descendant world states".to_string());
    }
    if manifest.change_sets.iter().any(|c| {
        c.from_state_id == state_id || c.to_state_id == state_id
    }) {
        return Err("cannot delete state referenced by a changeset".to_string());
    }
    if manifest.events.iter().any(|e| {
        e.anchor_state_id.as_deref() == Some(state_id)
            || e.result_state_id.as_deref() == Some(state_id)
    }) {
        return Err("cannot delete state referenced by a historical event".to_string());
    }
    let n = manifest.states.len();
    manifest.states.retain(|s| s.id != state_id);
    if manifest.states.len() == n {
        return Err("state not found".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEMP_N: AtomicU32 = AtomicU32::new(0);

    fn temp_world() -> (std::path::PathBuf, u32) {
        let n = TEMP_N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mk-hist-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        (dir, std::process::id())
    }

    #[test]
    fn unlock_creates_baseline_without_domain_bundles() {
        let (dir, _) = temp_world();
        let m = unlock_history(&dir).unwrap();
        assert!(m.enabled);
        assert_eq!(m.selected_state_id, BASELINE_STATE_ID);
        assert_eq!(m.states.len(), 1);
        assert_eq!(m.states[0].name, "Current Age");
        assert_eq!(m.domain_bundles.len(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fork_land_copies_only_for_one_state() {
        let (dir, _) = temp_world();
        let layer_dir = dir.join("map/layers");
        fs::create_dir_all(&layer_dir).unwrap();
        fs::write(layer_dir.join("land_mask.json"), r#"{"layer_id":"land_mask"}"#).unwrap();
        fs::write(layer_dir.join("elevation.json"), r#"{"layer_id":"elevation"}"#).unwrap();

        let mut m = unlock_history(&dir).unwrap();
        let later = add_world_state(
            &mut m,
            &CreateStateInput {
                time_key: 427,
                display_date: "0427".to_string(),
                name: "After".to_string(),
                based_on: BASELINE_STATE_ID.to_string(),
                direction: "later".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            later.domain_refs.get(DOMAIN_LAND).map(String::as_str),
            Some(CANONICAL_DOMAIN_REF)
        );

        let new_ref = fork_domain_bundle(&dir, &mut m, &later.id, DOMAIN_LAND).unwrap();
        assert!(new_ref.starts_with("land-"));
        assert_ne!(
            m.state_by_id(BASELINE_STATE_ID)
                .unwrap()
                .domain_refs
                .get(DOMAIN_LAND)
                .map(String::as_str),
            Some(new_ref.as_str())
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ancestor_fork_marks_divergence_not_stale() {
        let (dir, _) = temp_world();
        fs::create_dir_all(dir.join("map/layers")).unwrap();
        fs::write(dir.join("map/layers/land_mask.json"), "{}").unwrap();
        fs::write(dir.join("map/layers/elevation.json"), "{}").unwrap();

        let mut m = unlock_history(&dir).unwrap();
        let later_id = add_world_state(
            &mut m,
            &CreateStateInput {
                time_key: 100,
                display_date: "0100".to_string(),
                name: "Later".to_string(),
                based_on: BASELINE_STATE_ID.to_string(),
                direction: "later".to_string(),
            },
        )
        .unwrap()
        .id;

        fork_domain_bundle(&dir, &mut m, BASELINE_STATE_ID, DOMAIN_LAND).unwrap();

        let later = m.state_by_id(&later_id).unwrap();
        assert!(later.history_divergence.contains(&DOMAIN_LAND.to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lore_event_does_not_add_world_state() {
        let (dir, _) = temp_world();
        let mut m = unlock_history(&dir).unwrap();
        let n_states = m.states.len();
        let evt = add_lore_event(
            &mut m,
            &CreateEventInput {
                time_key: 112,
                display_date: "0112".to_string(),
                name: "Founding".to_string(),
                description: "Lore only".to_string(),
                anchor_state_id: Some(BASELINE_STATE_ID.to_string()),
            },
        )
        .unwrap();
        assert_eq!(m.states.len(), n_states);
        assert!(evt.change_set_id.is_none());
        assert!(evt.result_state_id.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cataclysm_links_event_changeset_and_state() {
        let (dir, _) = temp_world();
        let mut m = unlock_history(&dir).unwrap();
        let out = create_cataclysm(
            &mut m,
            &CreateCataclysmInput {
                time_key: 427,
                display_date: "0427".to_string(),
                event_name: "The Sundering".to_string(),
                description: "Cataclysm lore".to_string(),
                result_state_name: "After the Sundering".to_string(),
                based_on: BASELINE_STATE_ID.to_string(),
                changed_domains: vec![DOMAIN_LAND.to_string()],
                notes: "land reshaped".to_string(),
            },
        )
        .unwrap();
        assert_eq!(out.event.result_state_id.as_deref(), Some(out.result_state.id.as_str()));
        assert_eq!(
            out.event.change_set_id.as_deref(),
            Some(out.change_set.id.as_str())
        );
        assert_eq!(out.change_set.to_state_id, out.result_state.id);
        assert!(out.change_set.changed_domains.contains(&DOMAIN_LAND.to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ancestor_fork_does_not_change_later_domain_ref() {
        let (dir, _) = temp_world();
        fs::create_dir_all(dir.join("map/layers")).unwrap();
        fs::write(dir.join("map/layers/land_mask.json"), "{}").unwrap();
        fs::write(dir.join("map/layers/elevation.json"), "{}").unwrap();

        let mut m = unlock_history(&dir).unwrap();
        let later_id = add_world_state(
            &mut m,
            &CreateStateInput {
                time_key: 100,
                display_date: "0100".to_string(),
                name: "Later".to_string(),
                based_on: BASELINE_STATE_ID.to_string(),
                direction: "later".to_string(),
            },
        )
        .unwrap()
        .id;

        fork_domain_bundle(&dir, &mut m, BASELINE_STATE_ID, DOMAIN_LAND).unwrap();
        let later = m.state_by_id(&later_id).unwrap();
        assert_eq!(
            later.domain_refs.get(DOMAIN_LAND).map(String::as_str),
            Some(CANONICAL_DOMAIN_REF)
        );
        assert!(later.history_divergence.contains(&DOMAIN_LAND.to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn divergence_review_lists_affected_domains() {
        let (dir, _) = temp_world();
        fs::create_dir_all(dir.join("map/layers")).unwrap();
        fs::write(dir.join("map/layers/land_mask.json"), "{}").unwrap();
        fs::write(dir.join("map/layers/elevation.json"), "{}").unwrap();

        let mut m = unlock_history(&dir).unwrap();
        let later_id = add_world_state(
            &mut m,
            &CreateStateInput {
                time_key: 100,
                display_date: "0100".to_string(),
                name: "Later".to_string(),
                based_on: BASELINE_STATE_ID.to_string(),
                direction: "later".to_string(),
            },
        )
        .unwrap()
        .id;
        fork_domain_bundle(&dir, &mut m, BASELINE_STATE_ID, DOMAIN_LAND).unwrap();

        let review = build_divergence_review(&m);
        assert_eq!(review.len(), 1);
        assert_eq!(review[0].state_id, later_id);
        assert_eq!(review[0].domains.len(), 1);
        assert_eq!(review[0].domains[0].domain, DOMAIN_LAND);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ack_clears_marker_without_changing_ref() {
        let (dir, _) = temp_world();
        fs::create_dir_all(dir.join("map/layers")).unwrap();
        fs::write(dir.join("map/layers/land_mask.json"), "{}").unwrap();
        fs::write(dir.join("map/layers/elevation.json"), "{}").unwrap();

        let mut m = unlock_history(&dir).unwrap();
        let later_id = add_world_state(
            &mut m,
            &CreateStateInput {
                time_key: 100,
                display_date: "0100".to_string(),
                name: "Later".to_string(),
                based_on: BASELINE_STATE_ID.to_string(),
                direction: "later".to_string(),
            },
        )
        .unwrap()
        .id;
        fork_domain_bundle(&dir, &mut m, BASELINE_STATE_ID, DOMAIN_LAND).unwrap();
        ack_divergence(&mut m, &later_id, None).unwrap();
        let later = m.state_by_id(&later_id).unwrap();
        assert!(later.history_divergence.is_empty());
        assert_eq!(
            later.domain_refs.get(DOMAIN_LAND).map(String::as_str),
            Some(CANONICAL_DOMAIN_REF)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rebase_copies_ancestor_ref() {
        let (dir, _) = temp_world();
        fs::create_dir_all(dir.join("map/layers")).unwrap();
        fs::write(dir.join("map/layers/land_mask.json"), "{}").unwrap();
        fs::write(dir.join("map/layers/elevation.json"), "{}").unwrap();

        let mut m = unlock_history(&dir).unwrap();
        let later_id = add_world_state(
            &mut m,
            &CreateStateInput {
                time_key: 100,
                display_date: "0100".to_string(),
                name: "Later".to_string(),
                based_on: BASELINE_STATE_ID.to_string(),
                direction: "later".to_string(),
            },
        )
        .unwrap()
        .id;
        let new_ref = fork_domain_bundle(&dir, &mut m, BASELINE_STATE_ID, DOMAIN_LAND).unwrap();
        rebase_domain(&mut m, &later_id, DOMAIN_LAND).unwrap();
        let later = m.state_by_id(&later_id).unwrap();
        assert!(later.history_divergence.is_empty());
        assert_eq!(
            later.domain_refs.get(DOMAIN_LAND).map(String::as_str),
            Some(new_ref.as_str())
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_blocked_with_descendants() {
        let (dir, _) = temp_world();
        let mut m = unlock_history(&dir).unwrap();
        let _ = add_world_state(
            &mut m,
            &CreateStateInput {
                time_key: 100,
                display_date: "0100".to_string(),
                name: "Later".to_string(),
                based_on: BASELINE_STATE_ID.to_string(),
                direction: "later".to_string(),
            },
        )
        .unwrap();
        assert!(delete_world_state(&mut m, BASELINE_STATE_ID).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
