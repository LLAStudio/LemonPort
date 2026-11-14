//! LemonPort — the tiny utility that frees your TCP ports instantly.
//!
//! Three modes share the same scan engine:
//! - `lemon-port`: interactive TUI;
//! - `lemon-port <PORT>`: immediately free one port;
//! - `lemon-port --json`: machine-readable list for scripts.
//!
//! The crate also exposes the `lp` binary, a short alias installed with it.

mod app;
mod killer;
mod project;
mod scanner;
mod ui;

use std::io::IsTerminal;
use std::process::ExitCode;

use clap::Parser;

/// Command-line arguments.
#[derive(Debug, Parser)]
#[command(
    name = "lemon-port",
    version,
    about = "🍋 LemonPort — free your TCP ports instantly",
    long_about = None,
    after_help = "Examples:\n  lemon-port            Open the interactive UI\n  lemon-port 3000       Kill the process listening on :3000\n  lemon-port --json     List ports as JSON\n  lemon-port --all      Include system ports (< 1024)"
)]
pub struct Cli {
    /// Port to free immediately (one-shot mode).
    #[arg(value_name = "PORT", value_parser = clap::value_parser!(u16).range(1..))]
    pub port: Option<u16>,

    /// Print listening ports as JSON and exit.
    #[arg(long)]
    pub json: bool,

    /// Include system ports (< 1024) and protected processes.
    #[arg(long)]
    pub all: bool,
}

/// Shared entry point for the `lemon-port` and `lp` binaries.
pub fn run(cli: Cli) -> ExitCode {
    let result = match cli.port {
        Some(port) => one_shot(port),
        None if cli.json => print_json(cli.all),
        None => interactive(cli.all),
    };

    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("🍋 lemon-port: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// One-shot mode: free the requested port and return a script-friendly exit code.
fn one_shot(port: u16) -> anyhow::Result<ExitCode> {
    let mut scanner = scanner::Scanner::new()?;
    // In explicit mode nothing is filtered: the user knows what they are doing.
    let scan = scanner.scan(true)?;
    let listeners: Vec<&scanner::PortEntry> = scan
        .ports
        .iter()
        .filter(|entry| entry.port == port)
        .collect();

    if listeners.is_empty() {
        println!("🍋 No process is listening on port :{port}");
        return Ok(ExitCode::FAILURE);
    }

    let mut pids: Vec<u32> = listeners.iter().filter_map(|entry| entry.pid).collect();
    pids.sort_unstable();
    pids.dedup();

    if pids.is_empty() {
        eprintln!("✗ :{port} is held by a protected system process (PID not found)");
        return Ok(ExitCode::FAILURE);
    }

    let mut failed = false;
    for pid in pids {
        let entry = listeners.iter().find(|entry| entry.pid == Some(pid));
        let process = entry
            .map(|entry| entry.process.clone())
            .unwrap_or_else(|| "process".to_string());
        let project = entry
            .and_then(|entry| entry.project.as_ref())
            .map(|project| format!(" · {} ({})", project.name, project.label()));

        match killer::terminate(pid, port, &process).outcome {
            killer::KillOutcome::Terminated { escalated } => {
                let method = if escalated { " (SIGKILL)" } else { "" };
                println!(
                    "🍋 :{port} freed — {process} (PID {pid}){}{method}",
                    project.as_deref().unwrap_or("")
                );
            }
            killer::KillOutcome::AlreadyGone => {
                eprintln!("✗ :{port} — {process} (PID {pid}) stopped before termination");
                failed = true;
            }
            killer::KillOutcome::Failed(message) => {
                eprintln!("✗ :{port} — {process} (PID {pid}): {message}");
                failed = true;
            }
        }
    }

    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// JSON mode: array of objects, one entry per port/process pair.
fn print_json(all: bool) -> anyhow::Result<ExitCode> {
    let mut scanner = scanner::Scanner::new()?;
    let scan = scanner.scan(all)?;
    println!("{}", serde_json::to_string_pretty(&scan.ports)?);
    Ok(ExitCode::SUCCESS)
}

/// Interactive mode: refuse to draw a TUI into a pipe.
fn interactive(all: bool) -> anyhow::Result<ExitCode> {
    if !std::io::stdout().is_terminal() {
        anyhow::bail!("interactive mode requires a terminal — use `lemon-port --json` in a script");
    }

    let mut app = app::App::new(all)?;
    ui::run(&mut app)?;
    Ok(ExitCode::SUCCESS)
}
