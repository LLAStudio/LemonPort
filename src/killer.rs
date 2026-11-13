//! Terminaison propre et cross-platform des processus.
//!
//! Sur Unix : `SIGTERM`, attente de 1,5 s, puis `SIGKILL` si le processus
//! résiste. Sur Windows, `TerminateProcess` est utilisé directement car il
//! n'existe pas de `SIGTERM` pour les applications console.

use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use sysinfo::Signal;
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

/// Délai laissé au processus pour s'arrêter proprement avant `SIGKILL`.
#[cfg(unix)]
const GRACE_PERIOD: Duration = Duration::from_millis(1500);
/// Délai d'attente après la terminaison forcée.
const FORCE_WAIT: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub enum KillOutcome {
    /// Le processus s'est arrêté (`escalated` : SIGKILL a été nécessaire).
    Terminated {
        escalated: bool,
    },
    /// Le PID n'existait déjà plus entre le scan et la terminaison.
    AlreadyGone,
    Failed(String),
}

#[derive(Debug)]
pub struct KillReport {
    pub pid: u32,
    pub port: u16,
    pub process: String,
    pub outcome: KillOutcome,
}

/// Termine le processus `pid` de manière synchrone.
pub fn terminate(pid: u32, port: u16, process: &str) -> KillReport {
    KillReport {
        pid,
        port,
        process: process.to_string(),
        outcome: terminate_inner(pid),
    }
}

/// Lance la terminaison dans un thread pour ne pas figer la TUI pendant
/// l'attente de 1,5 s. Le résultat arrive sur le canal retourné.
pub fn spawn_terminate(
    pid: u32,
    port: u16,
    process: String,
) -> std::io::Result<Receiver<KillReport>> {
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("lemon-port-kill".to_string())
        .spawn(move || {
            let report = terminate(pid, port, &process);
            let _ = sender.send(report);
        })?;
    Ok(receiver)
}

fn terminate_inner(raw_pid: u32) -> KillOutcome {
    let pid = Pid::from_u32(raw_pid);
    let mut system = match System::new() {
        Ok(system) => system,
        Err(error) => return KillOutcome::Failed(format!("sysinfo indisponible : {error}")),
    };

    refresh(&mut system, pid);
    match system.process(pid) {
        None => return KillOutcome::AlreadyGone,
        Some(process) if !is_running(process.status()) => return KillOutcome::AlreadyGone,
        Some(_) => {}
    }

    #[cfg(unix)]
    {
        let signal_sent = system
            .process(pid)
            .and_then(|process| process.kill_with(Signal::Term));
        match signal_sent {
            Some(true) => {}
            Some(false) => {
                return KillOutcome::Failed("permission refusée — relancez avec sudo".to_string())
            }
            None => {
                return KillOutcome::Failed(
                    "SIGTERM n'est pas disponible sur cette plateforme".to_string(),
                )
            }
        }

        if wait_for_exit(&mut system, pid, GRACE_PERIOD) {
            return KillOutcome::Terminated { escalated: false };
        }
    }

    force_kill(&mut system, pid)
}

fn force_kill(system: &mut System, pid: Pid) -> KillOutcome {
    let killed = system
        .process(pid)
        .map(|process| process.kill())
        .unwrap_or(false);

    // Le processus a pu disparaître entre l'attente et l'appel : ce n'est pas
    // une erreur, le port est bien libéré.
    if !killed {
        if wait_for_exit(system, pid, Duration::from_millis(200)) {
            return KillOutcome::Terminated { escalated: true };
        }
        return KillOutcome::Failed(
            "impossible de tuer le processus (permission refusée ?)".to_string(),
        );
    }

    if wait_for_exit(system, pid, FORCE_WAIT) {
        KillOutcome::Terminated { escalated: true }
    } else {
        KillOutcome::Failed("le processus résiste à la terminaison forcée".to_string())
    }
}

fn refresh(system: &mut System, pid: Pid) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
}

/// Attend la disparition (ou l'état zombie) du PID, avec un timeout borné.
fn wait_for_exit(system: &mut System, pid: Pid, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        refresh(system, pid);
        match system.process(pid) {
            None => return true,
            Some(process) if !is_running(process.status()) => return true,
            Some(_) => {}
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn is_running(status: ProcessStatus) -> bool {
    !matches!(status, ProcessStatus::Zombie | ProcessStatus::Dead)
}
