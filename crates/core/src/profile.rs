use serde::{Deserialize, Serialize};

use crate::cell_id::CellId;

/// Cell profile fields — V0 minimum; `schemas/` JSON Schema is the source of truth (roadmap 3.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellProfile {
    pub cell_id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default)]
    pub notes: String,
}

impl CellProfile {
    pub fn new(id: &CellId, display_name: impl Into<String>) -> Self {
        Self {
            cell_id: id.to_string(),
            display_name: display_name.into(),
            slug: None,
            notes: String::new(),
        }
    }

    /// Validation posture (D-23, final for V0): save never blocks. An empty
    /// `display_name` is a valid state ("painted, unnamed yet"), not an
    /// issue — do not warn on it. `cell_id` format stays defensive, ready
    /// for hand-edited files / import (Later); unreachable via the real
    /// entry points (server always builds a canonical id; CLI rejects bad
    /// ids earlier, before this runs).
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if CellId::parse(&self.cell_id).is_none() {
            issues.push(ValidationIssue::Error(format!(
                "cell_id '{}' is not canonical ({{world_id}}.hex.q{{q}}.r{{r}})",
                self.cell_id
            )));
        }
        issues
    }
}

/// Validation outcome — strictness (warn vs block) is open (roadmap 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationIssue {
    Warning(String),
    Error(String),
}
