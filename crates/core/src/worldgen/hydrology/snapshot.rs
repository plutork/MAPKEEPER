//! Versioned atomic persistence boundary for generated hydrology.

use serde::{Deserialize, Serialize};

use super::catalog::HydrologyCatalog;
use super::channel_graph::ChannelGraph;
use super::drainage_graph::DrainageGraph;

pub const HYDROLOGY_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const HYDROLOGY_SNAPSHOT_FILE: &str = "hydrology-v2.json";
pub const CHANNEL_NODE_LAYER_ID: &str = "hydrology_channel_node_id";
pub const CHANNEL_SEGMENT_LAYER_ID: &str = "hydrology_channel_segment_id";
pub const HYDROLOGY_GENERATOR_VERSION: &str = "hydrology-v2";
pub const HYDROLOGY_CHANNEL_POLICY_BASE: &str = "channel-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HydrologySnapshot {
    pub schema_version: u32,
    pub base_revision: u64,
    pub fingerprint: String,
    pub generator_version: String,
    pub policy_version: String,
    /// Stable topology identity from base inputs + channel policy — not author RNG.
    pub effective_seed: u64,
    pub drainage: DrainageGraph,
    pub channels: ChannelGraph,
    #[serde(default)]
    pub catalog: HydrologyCatalog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HydrologySnapshotError {
    SchemaVersion { found: u32 },
    StaleBaseRevision { expected: u64, found: u64 },
    StaleFingerprint,
    InvalidChannelProjection,
}

impl HydrologySnapshot {
    pub fn new(
        base_revision: u64,
        fingerprint: String,
        generator_version: String,
        policy_version: String,
        effective_seed: u64,
        drainage: DrainageGraph,
        channels: ChannelGraph,
    ) -> Self {
        Self {
            schema_version: HYDROLOGY_SNAPSHOT_SCHEMA_VERSION,
            base_revision,
            fingerprint,
            generator_version,
            policy_version,
            effective_seed,
            drainage,
            channels,
            catalog: HydrologyCatalog::default(),
        }
    }

    pub fn with_catalog(mut self, catalog: HydrologyCatalog) -> Self {
        self.catalog = catalog;
        self
    }

    pub fn validate_current(
        &self,
        base_revision: u64,
        fingerprint: &str,
    ) -> Result<(), HydrologySnapshotError> {
        if self.schema_version != HYDROLOGY_SNAPSHOT_SCHEMA_VERSION {
            return Err(HydrologySnapshotError::SchemaVersion {
                found: self.schema_version,
            });
        }
        if self.base_revision != base_revision {
            return Err(HydrologySnapshotError::StaleBaseRevision {
                expected: base_revision,
                found: self.base_revision,
            });
        }
        if self.fingerprint != fingerprint {
            return Err(HydrologySnapshotError::StaleFingerprint);
        }
        let rivers = &self.channels.river_graph;
        if rivers.channel_mask.len() != rivers.channel_node_id.len()
            || rivers.channel_mask.len() != rivers.channel_segment_id.len()
        {
            return Err(HydrologySnapshotError::InvalidChannelProjection);
        }
        Ok(())
    }

    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(raw: &str) -> serde_json::Result<Self> {
        serde_json::from_str(raw)
    }
}

/// Channel policy id including river density preset (topology-affecting).
pub fn hydrology_policy_version(river_density: &str) -> String {
    format!("{HYDROLOGY_CHANNEL_POLICY_BASE}/{river_density}")
}

/// Derive stable seed from base revision + policy — UI nonce must not affect topology.
pub fn derive_effective_seed(base_revision: u64, policy_version: &str) -> u64 {
    let mut hash = base_revision;
    for byte in policy_version.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

pub fn is_derived_hydrology_layer_id(layer_id: &str) -> bool {
    matches!(layer_id, CHANNEL_NODE_LAYER_ID | CHANNEL_SEGMENT_LAYER_ID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_layers_are_not_generic_layers() {
        assert!(is_derived_hydrology_layer_id(CHANNEL_NODE_LAYER_ID));
        assert!(is_derived_hydrology_layer_id(CHANNEL_SEGMENT_LAYER_ID));
        assert!(!is_derived_hydrology_layer_id("elevation"));
    }

    #[test]
    fn effective_seed_is_stable_for_same_policy() {
        let a = derive_effective_seed(0xabc, "channel-v1/balanced");
        let b = derive_effective_seed(0xabc, "channel-v1/balanced");
        assert_eq!(a, b);
        assert_ne!(a, derive_effective_seed(0xabc, "channel-v1/many"));
    }
}
