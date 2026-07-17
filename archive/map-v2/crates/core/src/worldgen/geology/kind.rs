//! Geology category lookup helpers.

use crate::layer::{DenseLayer, DenseState, LayerValue};

use super::types::{
    GEOLOGY_BASIN, GEOLOGY_NONE, GEOLOGY_RIDGE, GEOLOGY_RIFT, GEOLOGY_STABLE, GEOLOGY_VOLCANIC_ARC,
};

pub(crate) fn geology_kind(geology: &DenseLayer, index: usize) -> &'static str {
    geology_kind_at(geology, index)
}

/// Geology category at index (for elevation bridge).
pub fn geology_kind_at(geology: &DenseLayer, index: usize) -> &'static str {
    match geology.state(index) {
        DenseState::Value(LayerValue::Text(ref t)) => match t.as_str() {
            GEOLOGY_STABLE => GEOLOGY_STABLE,
            GEOLOGY_BASIN => GEOLOGY_BASIN,
            GEOLOGY_RIDGE => GEOLOGY_RIDGE,
            GEOLOGY_RIFT => GEOLOGY_RIFT,
            GEOLOGY_VOLCANIC_ARC => GEOLOGY_VOLCANIC_ARC,
            _ => GEOLOGY_NONE,
        },
        _ => GEOLOGY_NONE,
    }
}

pub(crate) fn is_minor_geology(kind: &str) -> bool {
    matches!(
        kind,
        GEOLOGY_BASIN | GEOLOGY_RIDGE | GEOLOGY_RIFT | GEOLOGY_VOLCANIC_ARC
    )
}

#[cfg(test)]
pub(crate) fn is_orogenic_kind(kind: &str) -> bool {
    matches!(kind, GEOLOGY_RIDGE | GEOLOGY_RIFT | GEOLOGY_VOLCANIC_ARC)
}
