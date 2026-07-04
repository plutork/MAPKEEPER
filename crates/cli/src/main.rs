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
use mapkeeper_core::profile::CellProfile;
use mapkeeper_core::projects::{projects_file_path, ProjectEntry, ProjectsFile};
use mapkeeper_core::world;

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
}

#[derive(Args)]
struct InitArgs {
    /// World id — lowercase alnum, `-`/`_` only (used in cell_id and filenames).
    world_id: String,
    /// Target folder for the world project (created if missing).
    #[arg(long, default_value = ".")]
    path: PathBuf,
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
