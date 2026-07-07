//! mapkeeper CLI — owns filesystem + commands; delegates rules to mapkeeper-core.
//!
//! V0 flow-first slice (roadmap D-21): `init` scaffolds a world (minimal
//! wizard, 3.5); `profile get`/`list` are the agent-facing query path (3.3).
//! Profile *writing* is normally the editor's job (`mapkeeper-server` +
//! web UI); `profile set` exists here too so the flow can be exercised and
//! tested without a browser.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use mapkeeper_core::cell_id::CellId;
use mapkeeper_core::hex::{Axial, MapBounds};
use mapkeeper_core::hydro::{hydro_from_elevation, DEFAULT_LAND_ELEVATION, ELEVATION_LAYER_ID};
use mapkeeper_core::layer::{
    Bounds, DenseLayer, DenseState, LayerValue, MapManifest, ValueType, TERRAIN_LAYER_ID,
};
use mapkeeper_core::map_preset::{parse_map_preset, MapPreset, LEGACY_DEFAULT_RADIUS};
use mapkeeper_core::profile::CellProfile;
use mapkeeper_core::projects::{projects_file_path, ProjectEntry, ProjectsFile};
use mapkeeper_core::world;
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "mapkeeper", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold a new world project (minimal wizard, roadmap 3.5).
    Init(InitArgs),
    /// Query or edit per-cell profiles (roadmap 3.3).
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Query or edit the terrain map-state layer (Hex Map Model Foundation, D-36).
    Terrain {
        #[command(subcommand)]
        action: TerrainAction,
    },
    /// Query or edit elevation (hydro derives from threshold in runtime).
    Elevation {
        #[command(subcommand)]
        action: ElevationAction,
    },
    /// Generic map-state layer access by id (scale-layers, D-46).
    Layer {
        #[command(subcommand)]
        action: LayerAction,
    },
}

#[derive(Args)]
struct InitArgs {
    /// World id — lowercase alnum, `-`/`_` only (used in cell_id and filenames).
    world_id: String,
    /// Target folder for the world project (created if missing).
    #[arg(long, default_value = ".")]
    path: PathBuf,
    /// Map size preset: small (~127), medium (~1K), large (~8K), epic (~30K cells).
    #[arg(long)]
    map_preset: Option<String>,
}

#[derive(Subcommand)]
enum ProfileAction {
    /// Print a cell profile as JSON, or a blank placeholder if none exists yet.
    Get {
        /// Canonical cell_id, e.g. `main.hex.q3.r-1`.
        cell_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
    },
    /// List every profile currently saved in the world.
    List {
        #[arg(long, default_value = ".")]
        world: PathBuf,
    },
    /// Save a placeholder profile (title + notes) — mainly for flow testing.
    Set {
        cell_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        notes: String,
    },
}

#[derive(Subcommand)]
enum TerrainAction {
    /// Print a cell's terrain state as JSON (unknown / none / value).
    Get {
        /// Canonical cell_id, e.g. `main.hex.q3.r-1`.
        cell_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
    },
    /// List every cell with a stored terrain state (none or value).
    List {
        #[arg(long, default_value = ".")]
        world: PathBuf,
    },
    /// Set a cell's terrain value, or mark it explicitly absent with `--none`.
    Set {
        cell_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
        /// Terrain value (e.g. `forest`). Omit and pass `--none` for explicit absence.
        #[arg(long, conflicts_with = "none")]
        value: Option<String>,
        /// Mark the cell as explicitly absent (`none`) rather than a value.
        #[arg(long)]
        none: bool,
    },
    /// Clear a cell back to `unknown` (removes it from the layer file).
    Clear {
        cell_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
    },
}

#[derive(Subcommand)]
enum ElevationAction {
    /// Print a cell elevation and derived hydro state as JSON.
    Get {
        cell_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
    },
    /// List every stored elevation override (sparse file).
    List {
        #[arg(long, default_value = ".")]
        world: PathBuf,
    },
    /// Set cell elevation value (integer).
    Set {
        cell_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
        #[arg(long)]
        value: i16,
    },
    /// Reset cell to default land elevation (sparse clear).
    Clear {
        cell_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
    },
}

/// scale-layers (D-46): generic access to any dense layer file by id, so new
/// layers don't each need bespoke CLI subcommands.
#[derive(Subcommand)]
enum LayerAction {
    /// Print a cell's state in `<layer_id>` as JSON (unknown / none / value).
    Get {
        layer_id: String,
        /// Canonical cell_id, e.g. `main.hex.q3.r-1`.
        cell_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
    },
    /// List every stored (non-unknown) cell in `<layer_id>`.
    List {
        layer_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
    },
    /// Set a cell value: `--value <text>` (categorical), `--int <n>` (integer),
    /// or `--none` for explicit absence.
    Set {
        layer_id: String,
        cell_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
        #[arg(long, conflicts_with_all = ["int", "none"])]
        value: Option<String>,
        #[arg(long, conflicts_with_all = ["value", "none"])]
        int: Option<i32>,
        #[arg(long, conflicts_with_all = ["value", "int"])]
        none: bool,
    },
    /// Clear a cell back to `unknown`.
    Clear {
        layer_id: String,
        cell_id: String,
        #[arg(long, default_value = ".")]
        world: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init(args) => cmd_init(args),
        Command::Profile { action } => match action {
            ProfileAction::Get { cell_id, world } => cmd_profile_get(&world, &cell_id),
            ProfileAction::List { world } => cmd_profile_list(&world),
            ProfileAction::Set { cell_id, world, title, notes } => {
                cmd_profile_set(&world, &cell_id, &title, &notes)
            }
        },
        Command::Terrain { action } => match action {
            TerrainAction::Get { cell_id, world } => cmd_terrain_get(&world, &cell_id),
            TerrainAction::List { world } => cmd_terrain_list(&world),
            TerrainAction::Set { cell_id, world, value, none } => {
                cmd_terrain_set(&world, &cell_id, value, none)
            }
            TerrainAction::Clear { cell_id, world } => cmd_terrain_clear(&world, &cell_id),
        },
        Command::Elevation { action } => match action {
            ElevationAction::Get { cell_id, world } => cmd_elevation_get(&world, &cell_id),
            ElevationAction::List { world } => cmd_elevation_list(&world),
            ElevationAction::Set { cell_id, world, value } => {
                cmd_elevation_set(&world, &cell_id, value)
            }
            ElevationAction::Clear { cell_id, world } => cmd_elevation_clear(&world, &cell_id),
        },
        Command::Layer { action } => match action {
            LayerAction::Get { layer_id, cell_id, world } => {
                cmd_layer_get(&world, &layer_id, &cell_id)
            }
            LayerAction::List { layer_id, world } => cmd_layer_list(&world, &layer_id),
            LayerAction::Set { layer_id, cell_id, world, value, int, none } => {
                cmd_layer_set(&world, &layer_id, &cell_id, value, int, none)
            }
            LayerAction::Clear { layer_id, cell_id, world } => {
                cmd_layer_clear(&world, &layer_id, &cell_id)
            }
        },
    }
}

fn cmd_init(args: InitArgs) -> Result<()> {
    if !world::is_valid_world_id(&args.world_id) {
        bail!(
            "invalid world id '{}': use lowercase letters, digits, '-', '_' only",
            args.world_id
        );
    }
    fs::create_dir_all(&args.path)
        .with_context(|| format!("creating world folder {}", args.path.display()))?;
    for dir in world::SCAFFOLD_DIRS {
        fs::create_dir_all(args.path.join(dir))?;
    }
    let manifest = args.path.join("mapkeeper.toml");
    if manifest.exists() {
        bail!("{} already has a mapkeeper.toml — not overwriting", args.path.display());
    }
    write_scaffold_files(&args.path)?;
    let preset = match args.map_preset.as_deref() {
        None => MapPreset::Small,
        Some(raw) => parse_map_preset(raw).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid map preset '{raw}': use small, medium, large, or epic"
            )
        })?,
    };
    write_map_manifest(&args.path, preset.radius())?;
    fs::write(&manifest, world::manifest_toml(&args.world_id))?;
    println!(
        "Scaffolded world '{}' at {}",
        args.world_id,
        args.path.display()
    );

    // Best-effort: register in the shared projects list so the launcher/web
    // wizard also sees worlds created from the CLI. Never fails `init`.
    if let Err(err) = register_project(&args.world_id, &args.path) {
        eprintln!("warn: could not register in projects list: {err}");
    }
    Ok(())
}

/// Write the static scaffold files (roadmap 5.2, single source of truth —
/// same bundle the GitHub Template ships). `mapkeeper.toml` is separate,
/// written by the caller with the actual world id.
fn write_scaffold_files(root: &Path) -> Result<()> {
    for file in world::SCAFFOLD_FILES {
        let path = root.join(file.rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, file.contents)?;
    }
    Ok(())
}

fn write_map_manifest(world_path: &Path, radius: i32) -> Result<()> {
    let manifest = MapManifest::default_v0(radius);
    let path = world_path.join("map/manifest.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, manifest.to_json_pretty()?)?;
    Ok(())
}

fn projects_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").ok();
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
    PathBuf::from(projects_file_path(appdata.as_deref(), home.as_deref()))
}

fn register_project(world_id: &str, path: &Path) -> Result<()> {
    let abs_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let list_path = projects_path();
    let mut file = match fs::read_to_string(&list_path) {
        Ok(raw) => ProjectsFile::parse(&raw),
        Err(_) => ProjectsFile::default(),
    };
    file.upsert(ProjectEntry { id: world_id.to_string(), path: abs_path.display().to_string() });
    if let Some(parent) = list_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(list_path, file.to_json_pretty())?;
    Ok(())
}

fn profile_path(world: &Path, id: &CellId) -> PathBuf {
    world.join("profiles").join(id.filename())
}

fn cmd_profile_get(world: &Path, cell_id: &str) -> Result<()> {
    let id = CellId::parse(cell_id).with_context(|| format!("invalid cell_id '{cell_id}'"))?;
    let path = profile_path(world, &id);
    let profile = if path.exists() {
        let raw = fs::read_to_string(&path)?;
        serde_json::from_str(&raw)?
    } else {
        CellProfile::new(&id, "")
    };
    println!("{}", serde_json::to_string_pretty(&profile)?);
    Ok(())
}

fn cmd_profile_list(world: &Path) -> Result<()> {
    let dir = world.join("profiles");
    if !dir.exists() {
        println!("(no profiles — is {} a mapkeeper world?)", world.display());
        return Ok(());
    }
    let mut found = false;
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path)?;
        let profile: CellProfile = serde_json::from_str(&raw)?;
        println!("{}\t{}", profile.cell_id, profile.display_name);
        found = true;
    }
    if !found {
        println!("(no profiles yet)");
    }
    Ok(())
}

fn cmd_profile_set(world: &Path, cell_id: &str, title: &str, notes: &str) -> Result<()> {
    let id = CellId::parse(cell_id).with_context(|| format!("invalid cell_id '{cell_id}'"))?;
    let mut profile = CellProfile::new(&id, title);
    profile.notes = notes.to_string();
    let issues = profile.validate();
    for issue in &issues {
        eprintln!("warn: {issue:?}");
    }
    let path = profile_path(world, &id);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, serde_json::to_string_pretty(&profile)?)?;
    println!("Saved {}", path.display());
    Ok(())
}

// --- Map-state layers: dense storage + migrate-on-read (scale-layers, D-46) --
// On-disk truth is the dense `DenseLayer`; old sparse v1 files migrate on read.
// cell_id strings stay the external identity; the linear index is internal.

fn layer_file_path(world: &Path, layer_id: &str) -> PathBuf {
    world.join("map").join("layers").join(format!("{layer_id}.json"))
}

/// Map bounds from `map/manifest.json` (missing => legacy Small default).
fn read_bounds(world: &Path) -> MapBounds {
    let radius = match fs::read_to_string(world.join("map/manifest.json")) {
        Ok(raw) => match MapManifest::from_json(&raw) {
            Ok(m) => match m.bounds {
                Bounds::HexRadius { radius } => radius.max(0),
            },
            Err(_) => LEGACY_DEFAULT_RADIUS,
        },
        Err(_) => LEGACY_DEFAULT_RADIUS,
    };
    MapBounds::new(radius)
}

#[derive(Deserialize)]
struct WorldToml {
    world: WorldTomlSection,
}

#[derive(Deserialize)]
struct WorldTomlSection {
    id: String,
}

/// World id from `mapkeeper.toml` — needed to rebuild `cell_id` strings from
/// dense indices when listing.
fn read_world_id(world: &Path) -> Result<String> {
    let raw = fs::read_to_string(world.join("mapkeeper.toml"))
        .with_context(|| format!("reading mapkeeper.toml in {}", world.display()))?;
    let parsed: WorldToml = toml::from_str(&raw).context("parsing mapkeeper.toml")?;
    Ok(parsed.world.id)
}

fn write_dense_layer(world: &Path, layer: &DenseLayer) -> Result<()> {
    let path = layer_file_path(world, &layer.layer_id);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, layer.to_json_pretty()?)?;
    Ok(())
}

fn cell_index(bounds: &MapBounds, cell_id: &str) -> Result<usize> {
    let id = CellId::parse(cell_id).with_context(|| format!("invalid cell_id '{cell_id}'"))?;
    bounds
        .index_of(Axial::new(id.q, id.r))
        .with_context(|| format!("cell {cell_id} is outside the map bounds"))
}

fn read_terrain_dense(world: &Path, bounds: &MapBounds) -> DenseLayer {
    let raw = fs::read_to_string(layer_file_path(world, TERRAIN_LAYER_ID)).ok();
    DenseLayer::categorical_from_disk(raw.as_deref(), TERRAIN_LAYER_ID, bounds)
}

fn read_elevation_dense(world: &Path, bounds: &MapBounds) -> DenseLayer {
    let raw = fs::read_to_string(layer_file_path(world, ELEVATION_LAYER_ID)).ok();
    DenseLayer::elevation_from_disk(raw.as_deref(), bounds)
}

/// Read any layer file into dense form (migrate known sparse shapes on read).
fn read_generic_dense(world: &Path, layer_id: &str, bounds: &MapBounds) -> DenseLayer {
    match fs::read_to_string(layer_file_path(world, layer_id)).ok() {
        None => DenseLayer::new_categorical(layer_id, bounds.len()),
        Some(raw) => {
            if let Ok(dense) = DenseLayer::from_json(&raw) {
                dense
            } else if layer_id == ELEVATION_LAYER_ID {
                DenseLayer::elevation_from_disk(Some(&raw), bounds)
            } else {
                DenseLayer::categorical_from_disk(Some(&raw), layer_id, bounds)
            }
        }
    }
}

/// Keep default-land sparse (old semantics): default elevation clears the cell.
fn set_dense_elevation(dense: &mut DenseLayer, index: usize, elevation: i16) {
    if elevation == DEFAULT_LAND_ELEVATION {
        dense.set(index, DenseState::Unknown);
    } else {
        dense.set(index, DenseState::Value(LayerValue::Int(elevation as i32)));
    }
}

fn state_json(state: &DenseState) -> serde_json::Value {
    match state {
        DenseState::Unknown => serde_json::json!({ "state": "unknown" }),
        DenseState::None => serde_json::json!({ "state": "none" }),
        DenseState::Value(LayerValue::Text(v)) => {
            serde_json::json!({ "state": "value", "value": v })
        }
        DenseState::Value(LayerValue::Int(i)) => {
            serde_json::json!({ "state": "value", "value": i })
        }
    }
}

/// Display label for a stored cell in `list` output (`None` => unknown).
fn value_label(state: &DenseState) -> Option<String> {
    match state {
        DenseState::Unknown => None,
        DenseState::None => Some("none".to_string()),
        DenseState::Value(LayerValue::Text(v)) => Some(v.clone()),
        DenseState::Value(LayerValue::Int(i)) => Some(i.to_string()),
    }
}

fn print_layer_list(world: &Path, dense: &DenseLayer, bounds: &MapBounds) -> Result<()> {
    let world_id = read_world_id(world)?;
    for index in 0..dense.len() {
        if let Some(label) = value_label(&dense.state(index)) {
            let cell = bounds.from_index(index).expect("index within bounds");
            let cell_id = CellId::new(&world_id, cell.q, cell.r).to_string();
            println!("{cell_id}\t{label}");
        }
    }
    Ok(())
}

fn cmd_terrain_get(world: &Path, cell_id: &str) -> Result<()> {
    let bounds = read_bounds(world);
    let index = cell_index(&bounds, cell_id)?;
    let dense = read_terrain_dense(world, &bounds);
    println!("{}", serde_json::to_string_pretty(&state_json(&dense.state(index)))?);
    Ok(())
}

fn cmd_terrain_list(world: &Path) -> Result<()> {
    let bounds = read_bounds(world);
    let dense = read_terrain_dense(world, &bounds);
    if (0..dense.len()).all(|i| matches!(dense.state(i), DenseState::Unknown)) {
        println!("(no terrain set — every cell is unknown)");
        return Ok(());
    }
    print_layer_list(world, &dense, &bounds)
}

fn cmd_terrain_set(world: &Path, cell_id: &str, value: Option<String>, none: bool) -> Result<()> {
    let bounds = read_bounds(world);
    let index = cell_index(&bounds, cell_id)?;
    let state = match (value, none) {
        (Some(v), false) => DenseState::Value(LayerValue::Text(v)),
        (None, true) => DenseState::None,
        (None, false) => bail!("pass either --value <TERRAIN> or --none"),
        (Some(_), true) => unreachable!("clap conflicts_with prevents --value + --none"),
    };
    let mut dense = read_terrain_dense(world, &bounds);
    dense.set(index, state);
    write_dense_layer(world, &dense)?;
    println!("Saved terrain for {cell_id}");
    Ok(())
}

fn cmd_terrain_clear(world: &Path, cell_id: &str) -> Result<()> {
    let bounds = read_bounds(world);
    let index = cell_index(&bounds, cell_id)?;
    let mut dense = read_terrain_dense(world, &bounds);
    dense.set(index, DenseState::Unknown);
    write_dense_layer(world, &dense)?;
    println!("Cleared terrain for {cell_id} (now unknown)");
    Ok(())
}

fn cmd_elevation_get(world: &Path, cell_id: &str) -> Result<()> {
    let bounds = read_bounds(world);
    let index = cell_index(&bounds, cell_id)?;
    let dense = read_elevation_dense(world, &bounds);
    let elevation = dense.int_or(index, DEFAULT_LAND_ELEVATION as i32) as i16;
    let payload = serde_json::json!({
        "cell_id": cell_id,
        "elevation": elevation,
        "hydro": hydro_from_elevation(elevation),
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn cmd_elevation_list(world: &Path) -> Result<()> {
    let bounds = read_bounds(world);
    let dense = read_elevation_dense(world, &bounds);
    if (0..dense.len()).all(|i| matches!(dense.state(i), DenseState::Unknown)) {
        println!("(no elevation overrides — default land everywhere)");
        return Ok(());
    }
    print_layer_list(world, &dense, &bounds)
}

fn cmd_elevation_set(world: &Path, cell_id: &str, value: i16) -> Result<()> {
    let bounds = read_bounds(world);
    let index = cell_index(&bounds, cell_id)?;
    let mut dense = read_elevation_dense(world, &bounds);
    set_dense_elevation(&mut dense, index, value);
    write_dense_layer(world, &dense)?;
    println!("Saved elevation for {cell_id} = {value}");
    Ok(())
}

fn cmd_elevation_clear(world: &Path, cell_id: &str) -> Result<()> {
    let bounds = read_bounds(world);
    let index = cell_index(&bounds, cell_id)?;
    let mut dense = read_elevation_dense(world, &bounds);
    dense.set(index, DenseState::Unknown);
    write_dense_layer(world, &dense)?;
    println!("Cleared elevation override for {cell_id}");
    Ok(())
}

fn cmd_layer_get(world: &Path, layer_id: &str, cell_id: &str) -> Result<()> {
    let bounds = read_bounds(world);
    let index = cell_index(&bounds, cell_id)?;
    let dense = read_generic_dense(world, layer_id, &bounds);
    println!("{}", serde_json::to_string_pretty(&state_json(&dense.state(index)))?);
    Ok(())
}

fn cmd_layer_list(world: &Path, layer_id: &str) -> Result<()> {
    let bounds = read_bounds(world);
    let dense = read_generic_dense(world, layer_id, &bounds);
    if (0..dense.len()).all(|i| matches!(dense.state(i), DenseState::Unknown)) {
        println!("(layer {layer_id} has no stored cells)");
        return Ok(());
    }
    print_layer_list(world, &dense, &bounds)
}

fn cmd_layer_set(
    world: &Path,
    layer_id: &str,
    cell_id: &str,
    value: Option<String>,
    int: Option<i32>,
    none: bool,
) -> Result<()> {
    let bounds = read_bounds(world);
    let index = cell_index(&bounds, cell_id)?;
    let state = match (value, int, none) {
        (Some(v), None, false) => DenseState::Value(LayerValue::Text(v)),
        (None, Some(i), false) => DenseState::Value(LayerValue::Int(i)),
        (None, None, true) => DenseState::None,
        (None, None, false) => bail!("pass --value <text>, --int <n>, or --none"),
        _ => unreachable!("clap conflicts prevent value/int/none combos"),
    };
    // On a brand-new layer pick the column kind from the flag used.
    let mut dense = if layer_file_path(world, layer_id).exists() {
        read_generic_dense(world, layer_id, &bounds)
    } else if matches!(state, DenseState::Value(LayerValue::Int(_))) {
        DenseLayer::new_integer(layer_id, bounds.len())
    } else {
        DenseLayer::new_categorical(layer_id, bounds.len())
    };
    if let DenseState::Value(ref v) = state {
        let ok = matches!(
            (v, dense.value_type),
            (LayerValue::Text(_), ValueType::Categorical) | (LayerValue::Int(_), ValueType::Integer)
        );
        if !ok {
            bail!(
                "layer '{layer_id}' is {:?}; value kind does not match (use {})",
                dense.value_type,
                match dense.value_type {
                    ValueType::Categorical => "--value/--none",
                    ValueType::Integer => "--int/--none",
                }
            );
        }
    }
    dense.set(index, state);
    write_dense_layer(world, &dense)?;
    println!("Saved {layer_id} for {cell_id}");
    Ok(())
}

fn cmd_layer_clear(world: &Path, layer_id: &str, cell_id: &str) -> Result<()> {
    let bounds = read_bounds(world);
    let index = cell_index(&bounds, cell_id)?;
    let mut dense = read_generic_dense(world, layer_id, &bounds);
    dense.set(index, DenseState::Unknown);
    write_dense_layer(world, &dense)?;
    println!("Cleared {layer_id} for {cell_id} (now unknown)");
    Ok(())
}
