//! CLI/dev entry point — thin wrapper over the `mapkeeper-server` library
//! (see `lib.rs`). The desktop shell (`mapkeeper-desktop`, Tauri) embeds the
//! same library directly instead of spawning this binary.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use mapkeeper_server::{run, ServerConfig};

#[derive(Parser)]
#[command(name = "mapkeeper-server", version, about)]
struct Args {
    /// World folder to open immediately — must contain `mapkeeper.toml`.
    /// Omit to start in launcher mode (Home screen picks/creates a world).
    #[arg(long)]
    world: Option<PathBuf>,
    #[arg(long, default_value_t = 4000)]
    port: u16,
    /// Built web UI (wasm-bindgen output) to serve as static files.
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/dist"))]
    web_dist: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    run(ServerConfig { world: args.world, port: args.port, web_dist: args.web_dist }).await
}
