//! mapkeeper CLI — owns filesystem + commands; delegates rules to mapkeeper-core.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mapkeeper", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Placeholder until init/query land (roadmap 3.3).
    Version,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => println!("mapkeeper {}", env!("CARGO_PKG_VERSION")),
    }
}
