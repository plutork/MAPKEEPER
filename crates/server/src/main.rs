use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use mapkeeper_server::{run, ServerConfig};

#[derive(Parser)]
#[command(name = "mapkeeper-server", version, about)]
struct Args {
    #[arg(long)]
    world: Option<PathBuf>,
    #[arg(long, default_value_t = 4000)]
    port: u16,
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/dist"))]
    web_dist: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    run(ServerConfig {
        world: args.world,
        port: args.port,
        web_dist: args.web_dist,
    })
    .await
}
