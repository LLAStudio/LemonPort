//! Main `lemon-port` binary.
//!
//! Clap parsing and dispatch to the three modes live in the library, shared
//! with the `lp` alias.

use clap::Parser;
use lemon_port::{run, Cli};

fn main() -> std::process::ExitCode {
    run(Cli::parse())
}
