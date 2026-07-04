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

    /// Minimal placeholder rules (flow-first, D-21) — real strictness is 3.4.
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if self.display_name.trim().is_empty() {
            issues.push(ValidationIssue::Warning("display_name is empty".into()));
        }
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
