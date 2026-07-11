//! River mouth invariant validation (D-100 rivers-mouth-tracing-v2).

use std::collections::{HashMap, HashSet};

use crate::hex::MapBounds;
use crate::hydro::SEA_LEVEL;
use crate::lakes::LakeCatalog;
use crate::rivers::{River, RiverCatalog};

/// Computed terminal — not persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiverTerminal {
    Sea { mouth_cell: usize },
    Lake { mouth_cell: usize, lake_id: u32 },
    Parent {
        mouth_cell: usize,
        parent_id: u32,
        join_cell: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiverValidationCode {
    DuplicateId,
    InvalidCell,
    EmptyRiver,
    NonContiguousPath,
    DuplicateCellInRiver,
    CellOccupied,
    MissingParent,
    SelfParent,
    ParentCycle,
    InvalidRootTerminal,
    InvalidConfluenceGeometry,
    InvalidParentChain,
    OceanCellInPath,
    LakeCellInPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiverValidationDiagnostic {
    pub river_id: u32,
    pub code: RiverValidationCode,
}

#[derive(Debug, Clone, Default)]
pub struct RiverValidationReport {
    pub diagnostics: Vec<RiverValidationDiagnostic>,
}

impl RiverValidationReport {
    pub fn is_ok(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Routing / terminal context for one validation pass.
#[derive(Debug, Clone)]
pub struct RiverValidationContext {
    pub heights: Vec<i32>,
    pub bounds: MapBounds,
    pub lake_cell_to_id: HashMap<usize, u32>,
    pub lake_cells: HashSet<usize>,
}

impl RiverValidationContext {
    pub fn new(heights: &[i32], bounds: &MapBounds, lakes: Option<&LakeCatalog>) -> Self {
        let mut lake_cell_to_id = HashMap::new();
        let mut lake_cells = HashSet::new();
        if let Some(catalog) = lakes {
            for lake in &catalog.lakes {
                for &c in &lake.cells {
                    lake_cells.insert(c);
                    lake_cell_to_id.insert(c, lake.id);
                }
            }
        }
        Self {
            heights: heights.to_vec(),
            bounds: *bounds,
            lake_cell_to_id,
            lake_cells,
        }
    }
}

fn neighbors(bounds: &MapBounds, index: usize) -> impl Iterator<Item = usize> + '_ {
    bounds
        .from_index(index)
        .into_iter()
        .flat_map(|cell| cell.neighbors())
        .filter_map(|n| bounds.index_of(n))
}

pub fn mouth_touches_sea(index: usize, heights: &[i32], bounds: &MapBounds) -> bool {
    neighbors(bounds, index).any(|n| heights[n] <= SEA_LEVEL)
}

pub fn mouth_touches_lake(
    index: usize,
    lake_cells: &HashSet<usize>,
    bounds: &MapBounds,
) -> bool {
    neighbors(bounds, index).any(|n| lake_cells.contains(&n))
}

pub fn join_target(
    mouth: usize,
    parent: &River,
    heights: &[i32],
    bounds: &MapBounds,
) -> Option<usize> {
    let mut cands: Vec<usize> = parent
        .cells
        .iter()
        .copied()
        .filter(|&c| neighbors(bounds, mouth).any(|n| n == c))
        .collect();
    cands.sort_by_key(|&c| (heights[c], c));
    cands.first().copied()
}

fn is_root(river: &River) -> bool {
    river.parent == 0 || river.parent == river.id
}

pub fn find_root_id(river_id: u32, catalog: &RiverCatalog) -> Option<u32> {
    let mut current = river_id;
    for _ in 0..=catalog.rivers.len() {
        let river = catalog.rivers.iter().find(|r| r.id == current)?;
        if is_root(river) {
            return Some(current);
        }
        current = river.parent;
    }
    None
}

pub fn classify_terminal(
    river: &River,
    catalog: &RiverCatalog,
    ctx: &RiverValidationContext,
) -> Option<RiverTerminal> {
    let mouth = *river.cells.last()?;
    if mouth >= ctx.bounds.len() || ctx.heights[mouth] <= SEA_LEVEL {
        return None;
    }
    if mouth_touches_sea(mouth, &ctx.heights, &ctx.bounds) {
        return Some(RiverTerminal::Sea { mouth_cell: mouth });
    }
    for n in neighbors(&ctx.bounds, mouth) {
        if let Some(&lake_id) = ctx.lake_cell_to_id.get(&n) {
            return Some(RiverTerminal::Lake {
                mouth_cell: mouth,
                lake_id,
            });
        }
    }
    if !is_root(river) {
        let parent = catalog.rivers.iter().find(|r| r.id == river.parent)?;
        let join_cell = join_target(mouth, parent, &ctx.heights, &ctx.bounds)?;
        return Some(RiverTerminal::Parent {
            mouth_cell: mouth,
            parent_id: parent.id,
            join_cell,
        });
    }
    None
}

fn validate_structural_one(
    river: &River,
    catalog: &RiverCatalog,
    ctx: &RiverValidationContext,
) -> Vec<RiverValidationDiagnostic> {
    let mut out = Vec::new();
    let push = |out: &mut Vec<_>, code: RiverValidationCode| {
        out.push(RiverValidationDiagnostic {
            river_id: river.id,
            code,
        });
    };

    if river.cells.is_empty() {
        push(&mut out, RiverValidationCode::EmptyRiver);
        return out;
    }
    for &idx in &river.cells {
        if idx >= ctx.bounds.len() {
            push(&mut out, RiverValidationCode::InvalidCell);
        } else if ctx.heights[idx] <= SEA_LEVEL {
            push(&mut out, RiverValidationCode::OceanCellInPath);
        } else if ctx.lake_cells.contains(&idx) {
            push(&mut out, RiverValidationCode::LakeCellInPath);
        }
    }
    let mut seen = HashSet::new();
    for &idx in &river.cells {
        if !seen.insert(idx) {
            push(&mut out, RiverValidationCode::DuplicateCellInRiver);
        }
    }
    for w in river.cells.windows(2) {
        let a = w[0];
        let b = w[1];
        if !neighbors(&ctx.bounds, a).any(|n| n == b) {
            push(&mut out, RiverValidationCode::NonContiguousPath);
            break;
        }
    }
    for &idx in &river.cells {
        if let Some(other) = catalog.rivers.iter().find_map(|r| {
            if r.id == river.id {
                return None;
            }
            r.cells.contains(&idx).then_some(r.id)
        }) {
            push(
                &mut out,
                RiverValidationCode::CellOccupied,
            );
            let _ = other;
            break;
        }
    }
    if river.parent == river.id {
        // stem — ok
    } else if catalog.rivers.iter().all(|r| r.id != river.parent) {
        push(&mut out, RiverValidationCode::MissingParent);
    }
    out
}

fn detect_parent_cycle(river_id: u32, catalog: &RiverCatalog) -> bool {
    let mut slow = river_id;
    let mut fast = river_id;
    loop {
        let Some(rs) = catalog.rivers.iter().find(|r| r.id == slow) else {
            return false;
        };
        let Some(rf) = catalog.rivers.iter().find(|r| r.id == fast) else {
            return false;
        };
        if rs.parent == rs.id {
            return false;
        }
        slow = rs.parent;
        fast = match catalog.rivers.iter().find(|r| r.id == rf.parent) {
            Some(r2) if r2.parent != r2.id => r2.parent,
            _ => return false,
        };
        if slow == fast {
            return true;
        }
    }
}

/// Structural + soft mouth diagnostics (manual save).
pub fn validate_catalog(
    catalog: &RiverCatalog,
    ctx: &RiverValidationContext,
) -> RiverValidationReport {
    let mut diagnostics = Vec::new();
    let mut ids = HashSet::new();
    for river in &catalog.rivers {
        if !ids.insert(river.id) {
            diagnostics.push(RiverValidationDiagnostic {
                river_id: river.id,
                code: RiverValidationCode::DuplicateId,
            });
        }
        diagnostics.extend(validate_structural_one(river, catalog, ctx));
        if detect_parent_cycle(river.id, catalog) {
            diagnostics.push(RiverValidationDiagnostic {
                river_id: river.id,
                code: RiverValidationCode::ParentCycle,
            });
        }
    }
    RiverValidationReport { diagnostics }
}

/// Mouth diagnostics for manual rivers (warnings only at adapter layer).
pub fn mouth_diagnostics(
    catalog: &RiverCatalog,
    ctx: &RiverValidationContext,
) -> Vec<RiverValidationDiagnostic> {
    let mut out = Vec::new();
    for river in &catalog.rivers {
        if classify_terminal(river, catalog, ctx).is_none() {
            if is_root(river) {
                out.push(RiverValidationDiagnostic {
                    river_id: river.id,
                    code: RiverValidationCode::InvalidRootTerminal,
                });
            } else {
                out.push(RiverValidationDiagnostic {
                    river_id: river.id,
                    code: RiverValidationCode::InvalidConfluenceGeometry,
                });
            }
        } else if !is_root(river) {
            let root = find_root_id(river.id, catalog);
            if root.is_none()
                || root
                    .and_then(|rid| catalog.rivers.iter().find(|r| r.id == rid))
                    .and_then(|r| classify_terminal(r, catalog, ctx))
                    .map(|t| matches!(t, RiverTerminal::Sea { .. } | RiverTerminal::Lake { .. }))
                    != Some(true)
            {
                out.push(RiverValidationDiagnostic {
                    river_id: river.id,
                    code: RiverValidationCode::InvalidParentChain,
                });
            }
        }
    }
    out
}

/// Strict gate for auto-generated catalogs.
pub fn validate_generated_catalog_strict(
    catalog: &RiverCatalog,
    ctx: &RiverValidationContext,
) -> RiverValidationReport {
    let mut report = validate_catalog(catalog, ctx);
    for river in &catalog.rivers {
        let terminal = classify_terminal(river, catalog, ctx);
        if is_root(river) {
            match terminal {
                Some(RiverTerminal::Sea { .. }) | Some(RiverTerminal::Lake { .. }) => {}
                _ => report.diagnostics.push(RiverValidationDiagnostic {
                    river_id: river.id,
                    code: RiverValidationCode::InvalidRootTerminal,
                }),
            }
        } else {
            match terminal {
                Some(RiverTerminal::Parent { .. }) => {
                    if let Some(root) = find_root_id(river.id, catalog) {
                        let root_river = catalog.rivers.iter().find(|r| r.id == root);
                        let root_ok = root_river
                            .and_then(|r| classify_terminal(r, catalog, ctx))
                            .map(|t| {
                                matches!(t, RiverTerminal::Sea { .. } | RiverTerminal::Lake { .. })
                            })
                            == Some(true);
                        if !root_ok {
                            report.diagnostics.push(RiverValidationDiagnostic {
                                river_id: river.id,
                                code: RiverValidationCode::InvalidParentChain,
                            });
                        }
                    } else {
                        report.diagnostics.push(RiverValidationDiagnostic {
                            river_id: river.id,
                            code: RiverValidationCode::InvalidParentChain,
                        });
                    }
                }
                _ => report.diagnostics.push(RiverValidationDiagnostic {
                    river_id: river.id,
                    code: RiverValidationCode::InvalidConfluenceGeometry,
                }),
            }
        }
    }
    report
}

/// True if assigning `parent_id` to `river_id` would close a parent loop.
pub fn would_assign_parent_cycle(river_id: u32, parent_id: u32, catalog: &RiverCatalog) -> bool {
    if parent_id == river_id {
        return true;
    }
    let mut cur = parent_id;
    for _ in 0..=catalog.rivers.len() {
        if cur == river_id {
            return true;
        }
        let Some(r) = catalog.rivers.iter().find(|r| r.id == cur) else {
            return false;
        };
        if is_root(r) {
            return false;
        }
        cur = r.parent;
    }
    false
}

fn expand_reject_trees(catalog: &RiverCatalog, seed: &HashSet<u32>) -> HashSet<u32> {
    let mut reject = seed.clone();
    loop {
        let before = reject.len();
        for r in &catalog.rivers {
            if detect_parent_cycle(r.id, catalog) {
                reject.insert(r.id);
            }
            if find_root_id(r.id, catalog).is_none() {
                reject.insert(r.id);
            }
            if r.parent != r.id && reject.contains(&r.parent) {
                reject.insert(r.id);
            }
        }
        if reject.len() == before {
            break;
        }
    }
    reject
}

/// D-100 auto gate: prune until strict validate passes or progress stops.
pub fn enforce_strict_generated_catalog(
    catalog: &mut RiverCatalog,
    ctx: &RiverValidationContext,
) -> u32 {
    let mut rejected = 0u32;
    let cap = catalog.rivers.len().saturating_add(8);
    for _ in 0..cap {
        rejected += prune_invalid_river_trees(catalog, ctx);
        let report = validate_generated_catalog_strict(catalog, ctx);
        if report.is_ok() {
            return rejected;
        }
        let seed: HashSet<u32> = report.diagnostics.iter().map(|d| d.river_id).collect();
        let reject = expand_reject_trees(catalog, &seed);
        let before = catalog.rivers.len();
        catalog.rivers.retain(|r| !reject.contains(&r.id));
        rejected += (before - catalog.rivers.len()) as u32;
        if before == catalog.rivers.len() {
            break;
        }
    }
    rejected
}

/// Remove rivers whose root tree lacks a valid sea/lake terminal.
pub fn prune_invalid_river_trees(
    catalog: &mut RiverCatalog,
    ctx: &RiverValidationContext,
) -> u32 {
    let mut rejected = 0u32;
    loop {
        let invalid_roots: HashSet<u32> = catalog
            .rivers
            .iter()
            .filter(|r| is_root(r))
            .filter(|r| {
                classify_terminal(r, catalog, ctx)
                    .is_none_or(|t| !matches!(t, RiverTerminal::Sea { .. } | RiverTerminal::Lake { .. }))
            })
            .map(|r| r.id)
            .collect();
        if invalid_roots.is_empty() {
            break;
        }
        let before = catalog.rivers.len();
        let keep: HashSet<u32> = catalog
            .rivers
            .iter()
            .filter_map(|r| {
                find_root_id(r.id, catalog).filter(|root| !invalid_roots.contains(root))
            })
            .collect();
        catalog.rivers.retain(|r| keep.contains(&r.id));
        rejected += (before - catalog.rivers.len()) as u32;
        if before == catalog.rivers.len() {
            break;
        }
    }
    loop {
        let before = catalog.rivers.len();
        let keep: HashSet<u32> = catalog
            .rivers
            .iter()
            .filter(|r| {
                if is_root(r) {
                    return true;
                }
                catalog.rivers.iter().any(|p| p.id == r.parent)
                    && classify_terminal(r, catalog, ctx)
                        .is_some_and(|t| matches!(t, RiverTerminal::Parent { .. }))
            })
            .map(|r| r.id)
            .collect();
        catalog.rivers.retain(|r| keep.contains(&r.id));
        rejected += (before - catalog.rivers.len()) as u32;
        if before == catalog.rivers.len() {
            break;
        }
    }
    rejected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::Axial;
    use crate::rivers::River;

    fn grid_bounds() -> MapBounds {
        MapBounds::new(8, 6)
    }

    fn land_idx(bounds: &MapBounds) -> usize {
        bounds
            .from_index(0)
            .map(|_| 0)
            .unwrap_or(0)
    }

    fn idx(bounds: &MapBounds, q: i32, r: i32) -> usize {
        bounds
            .index_of(Axial::new(q, r))
            .unwrap_or_else(|| land_idx(bounds))
    }

    #[test]
    fn classifies_sea_mouth() {
        let bounds = grid_bounds();
        let mut heights = vec![0i32; bounds.len()];
        let land = idx(&bounds, 3, 2);
        heights[land] = 10;
        for n in neighbors(&bounds, land) {
            heights[n] = 0;
        }
        let catalog = RiverCatalog {
            schema_version: 1,
            next_id: 2,
            rivers: vec![River {
                id: 1,
                cells: vec![land],
                source: land,
                mouth: land,
                parent: 1,
                basin: 1,
                name: None,
            }],
        };
        let ctx = RiverValidationContext::new(&heights, &bounds, None);
        assert!(matches!(
            classify_terminal(&catalog.rivers[0], &catalog, &ctx),
            Some(RiverTerminal::Sea { .. })
        ));
    }

    #[test]
    fn rejects_broken_confluence_geometry() {
        let bounds = grid_bounds();
        let heights = vec![10i32; bounds.len()];
        let a = idx(&bounds, 2, 2);
        let b = idx(&bounds, 5, 2);
        let catalog = RiverCatalog {
            schema_version: 1,
            next_id: 3,
            rivers: vec![
                River {
                    id: 1,
                    cells: vec![a],
                    source: a,
                    mouth: a,
                    parent: 1,
                    basin: 1,
                    name: None,
                },
                River {
                    id: 2,
                    cells: vec![b],
                    source: b,
                    mouth: b,
                    parent: 1,
                    basin: 1,
                    name: None,
                },
            ],
        };
        let ctx = RiverValidationContext::new(&heights, &bounds, None);
        let report = validate_generated_catalog_strict(&catalog, &ctx);
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == RiverValidationCode::InvalidConfluenceGeometry));
    }

    #[test]
    fn enforce_strict_prunes_parent_cycle() {
        let bounds = grid_bounds();
        let heights = vec![10i32; bounds.len()];
        let a = idx(&bounds, 2, 2);
        let b = idx(&bounds, 5, 2);
        let mut catalog = RiverCatalog {
            schema_version: 1,
            next_id: 3,
            rivers: vec![
                River {
                    id: 1,
                    cells: vec![a],
                    source: a,
                    mouth: a,
                    parent: 2,
                    basin: 1,
                    name: None,
                },
                River {
                    id: 2,
                    cells: vec![b],
                    source: b,
                    mouth: b,
                    parent: 1,
                    basin: 1,
                    name: None,
                },
            ],
        };
        let ctx = RiverValidationContext::new(&heights, &bounds, None);
        let rejected = enforce_strict_generated_catalog(&mut catalog, &ctx);
        assert!(rejected >= 2);
        assert!(validate_generated_catalog_strict(&catalog, &ctx).is_ok());
    }
}
