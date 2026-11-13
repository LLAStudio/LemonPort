//! Binaire principal `lemon-port`.
//!
//! Le parsing clap et le dispatch vers les trois modes vivent dans la
//! bibliothèque, partagée avec l'alias `lp`.

use clap::Parser;
use lemon_port::{run, Cli};

fn main() -> std::process::ExitCode {
    run(Cli::parse())
}
