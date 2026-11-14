//! Short `lp` alias, installed by `cargo install` alongside `lemon-port`.

use clap::Parser;
use lemon_port::{run, Cli};

fn main() -> std::process::ExitCode {
    run(Cli::parse())
}
