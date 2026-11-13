//! LemonPort — la micro-utilité qui libère vos ports TCP en un instant.
//!
//! Trois modes partagent le même moteur de scan :
//! - `lemon-port` : interface TUI interactive ;
//! - `lemon-port <PORT>` : libération immédiate d'un port ;
//! - `lemon-port --json` : liste machine-readable pour les scripts.
//!
//! Le crate expose aussi le binaire `lp`, alias court installé en même temps.

mod app;
mod killer;
mod project;
mod scanner;
mod ui;

use std::io::IsTerminal;
use std::process::ExitCode;

use clap::Parser;

/// Arguments de la ligne de commande.
#[derive(Debug, Parser)]
#[command(
    name = "lemon-port",
    version,
    about = "🍋 LemonPort — libérez vos ports TCP en un instant",
    long_about = None,
    after_help = "Exemples :\n  lemon-port            Ouvre l'interface interactive\n  lemon-port 3000       Tue le processus qui écoute sur :3000\n  lemon-port --json     Liste les ports en JSON\n  lemon-port --all      Inclut les ports système (< 1024)"
)]
pub struct Cli {
    /// Port à libérer immédiatement (mode one-shot).
    #[arg(value_name = "PORT", value_parser = clap::value_parser!(u16).range(1..))]
    pub port: Option<u16>,

    /// Affiche les ports en écoute au format JSON puis quitte.
    #[arg(long)]
    pub json: bool,

    /// Inclut les ports système (< 1024) et les processus protégés.
    #[arg(long)]
    pub all: bool,
}

/// Point d'entrée commun aux binaires `lemon-port` et `lp`.
pub fn run(cli: Cli) -> ExitCode {
    let result = match cli.port {
        Some(port) => one_shot(port),
        None if cli.json => print_json(cli.all),
        None => interactive(cli.all),
    };

    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("🍋 lemon-port : {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Mode one-shot : libère le port demandé et rend un code de sortie scriptable.
fn one_shot(port: u16) -> anyhow::Result<ExitCode> {
    let mut scanner = scanner::Scanner::new()?;
    // En mode explicite, on ne filtre rien : l'utilisateur sait ce qu'il fait.
    let scan = scanner.scan(true)?;
    let listeners: Vec<&scanner::PortEntry> = scan
        .ports
        .iter()
        .filter(|entry| entry.port == port)
        .collect();

    if listeners.is_empty() {
        println!("🍋 Aucun processus n'écoute sur le port :{port}");
        return Ok(ExitCode::FAILURE);
    }

    let mut pids: Vec<u32> = listeners.iter().filter_map(|entry| entry.pid).collect();
    pids.sort_unstable();
    pids.dedup();

    if pids.is_empty() {
        eprintln!("✗ :{port} est détenu par un processus système protégé (PID introuvable)");
        return Ok(ExitCode::FAILURE);
    }

    let mut failed = false;
    for pid in pids {
        let entry = listeners.iter().find(|entry| entry.pid == Some(pid));
        let process = entry
            .map(|entry| entry.process.clone())
            .unwrap_or_else(|| "processus".to_string());
        let project = entry
            .and_then(|entry| entry.project.as_ref())
            .map(|project| format!(" · {} ({})", project.name, project.label()));

        match killer::terminate(pid, port, &process).outcome {
            killer::KillOutcome::Terminated { escalated } => {
                let method = if escalated { " (SIGKILL)" } else { "" };
                println!(
                    "🍋 :{port} libéré — {process} (PID {pid}){}{method}",
                    project.as_deref().unwrap_or("")
                );
            }
            killer::KillOutcome::AlreadyGone => {
                eprintln!("✗ :{port} — {process} (PID {pid}) s'est arrêté avant la terminaison");
                failed = true;
            }
            killer::KillOutcome::Failed(message) => {
                eprintln!("✗ :{port} — {process} (PID {pid}) : {message}");
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

/// Mode JSON : tableau d'objets, une entrée par couple port/processus.
fn print_json(all: bool) -> anyhow::Result<ExitCode> {
    let mut scanner = scanner::Scanner::new()?;
    let scan = scanner.scan(all)?;
    println!("{}", serde_json::to_string_pretty(&scan.ports)?);
    Ok(ExitCode::SUCCESS)
}

/// Mode interactif : refuse proprement d'écrire une TUI dans un pipe.
fn interactive(all: bool) -> anyhow::Result<ExitCode> {
    if !std::io::stdout().is_terminal() {
        anyhow::bail!(
            "le mode interactif nécessite un terminal — utilisez `lemon-port --json` dans un script"
        );
    }

    let mut app = app::App::new(all)?;
    ui::run(&mut app)?;
    Ok(ExitCode::SUCCESS)
}
