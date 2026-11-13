//! Alias court `lp`, installé par `cargo install` en même temps que `lemon-port`.

use clap::Parser;
use lemon_port::{run, Cli};

fn main() -> std::process::ExitCode {
    run(Cli::parse())
}
