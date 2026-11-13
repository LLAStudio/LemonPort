//! Détection des sockets TCP en écoute et enrichissement avec `sysinfo`.
//!
//! `netstat2` interroge les API natives du système (pas de parsing de
//! `netstat`), ce qui la rend rapide et cross-platform. Les processus
//! appartenant à un autre utilisateur restent visibles : on affiche alors
//! `système` / `protégé` au lieu d'échouer sur une erreur de permission.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::project::{Project, ProjectResolver};

/// Un port TCP en écoute, enrichi avec les informations du processus.
#[derive(Debug, Clone, Serialize)]
pub struct PortEntry {
    pub port: u16,
    pub protocol: &'static str,
    /// Adresse de bind (ex. `127.0.0.1`, `0.0.0.0`, `::`).
    pub address: String,
    pub pid: Option<u32>,
    /// Nom du binaire, ou `système` / `protégé` si inaccessible.
    pub process: String,
    pub project: Option<Project>,
    pub memory_mb: f64,
    pub uptime_seconds: u64,
    /// Port réservé au système (< 1024).
    pub system: bool,
    /// Le processus n'a pas pu être inspecté (droits insuffisants).
    pub protected: bool,
}

/// Résultat d'un scan, avec le nombre de ports système masqués.
pub struct Scan {
    pub ports: Vec<PortEntry>,
    pub hidden_system: usize,
}

struct ProcessInfo {
    name: String,
    memory_bytes: u64,
    uptime_seconds: u64,
    cwd: Option<PathBuf>,
}

/// Scanner réutilisable : conserve l'instance `sysinfo` et le cache des projets.
pub struct Scanner {
    system: System,
    resolver: ProjectResolver,
}

impl Scanner {
    pub fn new() -> Result<Self> {
        let system =
            System::new().map_err(|error| anyhow!("impossible d'initialiser sysinfo : {error}"))?;
        Ok(Self {
            system,
            resolver: ProjectResolver::default(),
        })
    }

    /// Invalide les noms de projets mis en cache.
    pub fn clear_project_cache(&mut self) {
        self.resolver.clear();
    }

    /// Scanne tous les ports TCP en écoute.
    ///
    /// `include_system = false` masque les ports < 1024 ; `--all` les inclut.
    pub fn scan(&mut self, include_system: bool) -> Result<Scan> {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_memory()
                .with_cwd(UpdateKind::OnlyIfNotSet),
        );
        let processes = self.snapshot_processes();

        let sockets = get_sockets_info(
            AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
            ProtocolFlags::TCP,
        )
        .map_err(|error| anyhow!("lecture de la table des sockets TCP : {error}"))?;

        // Clé (port, PID) : fusionne les binds IPv4/IPv6 du même processus.
        let mut merged: BTreeMap<(u16, Option<u32>), PortEntry> = BTreeMap::new();

        for socket in sockets {
            let ProtocolSocketInfo::Tcp(tcp) = &socket.protocol_socket_info else {
                continue;
            };
            if !matches!(tcp.state, TcpState::Listen) {
                continue;
            }

            let pids: Vec<Option<u32>> = if socket.associated_pids.is_empty() {
                vec![None]
            } else {
                socket
                    .associated_pids
                    .iter()
                    .map(|pid| Some(*pid))
                    .collect()
            };

            for pid in pids {
                let key = (tcp.local_port, pid);
                let address = tcp.local_addr.to_string();
                if let Some(existing) = merged.get_mut(&key) {
                    // Préfère une adresse précise à une adresse wildcard.
                    if is_unspecified(&existing.address) && !is_unspecified(&address) {
                        existing.address = address;
                    }
                    continue;
                }

                let info = pid.and_then(|pid| processes.get(&pid));
                let project = info
                    .and_then(|info| info.cwd.as_deref())
                    .and_then(|cwd| self.resolver.resolve(cwd));
                let protected = info.is_none();
                let process = match (info, pid) {
                    (Some(info), _) => info.name.clone(),
                    (None, Some(_)) => "système".to_string(),
                    (None, None) => "protégé".to_string(),
                };

                merged.insert(
                    key,
                    PortEntry {
                        port: tcp.local_port,
                        protocol: "TCP",
                        address,
                        pid,
                        process,
                        project,
                        memory_mb: info.map(|info| round_mb(info.memory_bytes)).unwrap_or(0.0),
                        uptime_seconds: info.map(|info| info.uptime_seconds).unwrap_or(0),
                        system: tcp.local_port < 1024,
                        protected,
                    },
                );
            }
        }

        let mut ports: Vec<PortEntry> = merged.into_values().collect();
        ports.sort_by(|a, b| {
            a.system
                .cmp(&b.system)
                .then_with(|| a.port.cmp(&b.port))
                .then_with(|| a.pid.cmp(&b.pid))
        });

        let mut hidden_system = 0;
        if !include_system {
            let before = ports.len();
            ports.retain(|entry| !entry.system);
            hidden_system = before - ports.len();
        }

        Ok(Scan {
            ports,
            hidden_system,
        })
    }

    fn snapshot_processes(&self) -> HashMap<u32, ProcessInfo> {
        let mut snapshot = HashMap::with_capacity(self.system.processes().len());
        for (pid, process) in self.system.processes() {
            let mut name = process.name().to_string_lossy().trim().to_string();
            if name.is_empty() {
                name = process
                    .exe()
                    .and_then(Path::file_name)
                    .map(|file_name| file_name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("pid {}", pid.as_u32()));
            }
            snapshot.insert(
                pid.as_u32(),
                ProcessInfo {
                    name,
                    memory_bytes: process.memory(),
                    uptime_seconds: process.run_time(),
                    cwd: process.cwd().map(Path::to_path_buf),
                },
            );
        }
        snapshot
    }
}

fn is_unspecified(address: &str) -> bool {
    address
        .parse::<IpAddr>()
        .map(|ip| ip.is_unspecified())
        .unwrap_or(false)
}

fn round_mb(bytes: u64) -> f64 {
    let megabytes = bytes as f64 / 1_048_576.0;
    (megabytes * 10.0).round() / 10.0
}
