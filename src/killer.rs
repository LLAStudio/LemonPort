//! Clean, cross-platform process termination.
//!
//! On Unix: `SIGTERM`, a 1.5 s wait, then `SIGKILL` if the process resists.
//! On Windows, `TerminateProcess` is used directly because console
//! applications have no `SIGTERM`.

use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use sysinfo::Signal;
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

/// Time given to the process to stop cleanly before `SIGKILL`.
#[cfg(unix)]
const GRACE_PERIOD: Duration = Duration::from_millis(1500);
/// Wait after forced termination.
const FORCE_WAIT: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub enum KillOutcome {
    /// The process stopped (`escalated`: SIGKILL was required).
    Terminated {
        escalated: bool,
    },
    /// The PID no longer existed between the scan and termination.
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

/// Terminate the `pid` process synchronously.
pub fn terminate(pid: u32, port: u16, process: &str) -> KillReport {
    KillReport {
        pid,
        port,
        process: process.to_string(),
        outcome: terminate_inner(pid),
    }
}

/// Run termination in a thread so the TUI never freezes during the 1.5 s
/// wait. The result arrives on the returned channel.
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
        Err(error) => return KillOutcome::Failed(format!("sysinfo unavailable: {error}")),
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
                return KillOutcome::Failed("permission denied — rerun with sudo".to_string())
            }
            None => {
                return KillOutcome::Failed("SIGTERM is not available on this platform".to_string())
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

    // The process may have disappeared between the wait and this call: that
    // is not an error, the port is free.
    if !killed {
        if wait_for_exit(system, pid, Duration::from_millis(200)) {
            return KillOutcome::Terminated { escalated: true };
        }
        return KillOutcome::Failed("could not kill the process (permission denied?)".to_string());
    }

    if wait_for_exit(system, pid, FORCE_WAIT) {
        KillOutcome::Terminated { escalated: true }
    } else {
        KillOutcome::Failed("the process resists forced termination".to_string())
    }
}

fn refresh(system: &mut System, pid: Pid) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
}

/// Wait for the PID to disappear (or become a zombie), with a bounded timeout.
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
