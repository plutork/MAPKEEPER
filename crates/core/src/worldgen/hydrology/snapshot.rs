//! Versioned atomic persistence boundary for generated hydrology.

use serde::{Deserialize, Serialize};

use super::catalog::HydrologyCatalog;
use super::channel_graph::ChannelGraph;
use super::drainage_graph::DrainageGraph;

pub const HYDROLOGY_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const HYDROLOGY_SNAPSHOT_FILE: &str = "hydrology-v2.json";
pub const CHANNEL_NODE_LAYER_ID: &str = "hydrology_channel_node_id";
pub const CHANNEL_SEGMENT_LAYER_ID: &str = "hydrology_channel_segment_id";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HydrologySnapshot {
    pub schema_version: u32,
    pub base_revision: u64,
    pub fingerprint: String,
    pub generator_version: String,
    pub policy_version: String,
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
}
