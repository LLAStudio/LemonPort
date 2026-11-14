//! Tests d'intégration de bout en bout.
//!
//! Chaque test lance un vrai processus qui écoute sur un port TCP, puis pilote
//! le binaire `lemon-port` comme un utilisateur le ferait :
//! - `--json` doit retrouver le port, le PID, le projet et les métriques ;
//! - le mode one-shot doit tuer le processus et libérer le port ;
//! - un processus qui ignore SIGTERM doit finir par SIGKILL (Unix).
//!
//! Le processus listener est le binaire de test lui-même, relancé avec le
//! filtre `helper_listener` : aucun interpréteur externe (Python, Node…)
//! n'est requis, et le test reste cross-platform.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

/// Chemin du binaire compilé, fourni par Cargo aux tests d'intégration.
const BIN: &str = env!("CARGO_BIN_EXE_lemon-port");
const PORT_FILE_ENV: &str = "LEMONPORT_TEST_PORT_FILE";

#[test]
fn helper_listener() {
    // En temps normal, ce test ne fait rien : il ne devient un listener que
    // lorsqu'il est relancé par `spawn_listener`.
    let Some(port_file) = std::env::var_os(PORT_FILE_ENV) else {
        return;
    };

    // Sur Unix, ce listener peut volontairement ignorer SIGTERM pour tester
    // l'escalade vers SIGKILL. Le handler est armé avant le bind : dès que le
    // port est publié, il est déjà ignoré.
    #[cfg(unix)]
    if std::env::var_os("LEMONPORT_TEST_IGNORE_SIGTERM").is_some() {
        // SAFETY: SIG_IGN est un gestionnaire valide, permanent, et le handler
        // ne touche à aucun état Rust.
        unsafe {
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
        }
    }

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind du listener");
    let port = listener
        .local_addr()
        .expect("adresse locale du listener")
        .port();
    fs::write(port_file, port.to_string()).expect("écriture du port");

    // Reste en LISTEN jusqu'à ce que le test parent (ou LemonPort) tue le processus.
    for stream in listener.incoming() {
        let _ = stream;
    }
}

#[test]
fn scan_and_json_detect_listener_and_project() {
    let listener = spawn_listener("LemonPortTestProject");
    let output = run_cli(&["--json", "--all"]);
    assert_success(&output);

    let ports: Vec<Value> = serde_json::from_slice(&output.stdout).expect("JSON valide");
    let entry = ports
        .iter()
        .find(|entry| entry["port"].as_u64() == Some(u64::from(listener.port)))
        .expect("le port du listener doit apparaître dans le JSON");

    assert_eq!(entry["protocol"].as_str(), Some("TCP"));
    assert_eq!(entry["pid"].as_u64(), Some(u64::from(listener.child.id())));
    assert!(entry["process"]
        .as_str()
        .is_some_and(|process| !process.is_empty()));
    assert_eq!(
        entry["project"]["name"].as_str(),
        Some("LemonPortTestProject")
    );
    assert_eq!(entry["project"]["framework"].as_str(), Some("Next.js"));
    assert_eq!(entry["system"].as_bool(), Some(false));
    assert_eq!(entry["protected"].as_bool(), Some(false));
    assert!(entry["memory_mb"].as_f64().is_some());
    assert!(entry["uptime_seconds"].as_u64().is_some());
}

#[test]
fn one_shot_kills_listener_and_frees_port() {
    let mut listener = spawn_listener("LemonPortKillProject");
    let port = listener.port;
    let port_argument = port.to_string();

    let output = run_cli(&[port_argument.as_str()]);
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(":{port}")),
        "confirmation attendue pour :{port}, stdout: {stdout}"
    );
    assert!(
        stdout.contains("libéré"),
        "confirmation de libération attendue, stdout: {stdout}"
    );

    assert!(
        wait_for_exit(&mut listener.child, Duration::from_secs(5)),
        "le listener est toujours vivant après le one-shot"
    );

    let output = run_cli(&["--json", "--all"]);
    assert_success(&output);
    let ports: Vec<Value> = serde_json::from_slice(&output.stdout).expect("JSON valide");
    assert!(
        !ports
            .iter()
            .any(|entry| entry["port"].as_u64() == Some(u64::from(port))),
        "le port :{port} est encore en écoute après le kill"
    );
}

/// Vérifie la séquence propre : SIGTERM ignoré, délai de grâce, puis SIGKILL.
#[cfg(unix)]
#[test]
fn one_shot_escalates_to_sigkill_for_stubborn_process() {
    let mut listener = spawn_listener_with("LemonPortStubbornProject", true);
    let port = listener.port;
    let port_argument = port.to_string();

    let started = Instant::now();
    let output = run_cli(&[port_argument.as_str()]);
    let elapsed = started.elapsed();
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("(SIGKILL)"),
        "SIGKILL attendu après le délai de grâce, stdout: {stdout}"
    );
    assert!(
        elapsed >= Duration::from_millis(1400),
        "SIGKILL envoyé trop tôt ({elapsed:?}) : SIGTERM n'a pas été ignoré"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "escalade anormalement lente : {elapsed:?}"
    );
    assert!(
        wait_for_exit(&mut listener.child, Duration::from_secs(5)),
        "le processus qui ignore SIGTERM est toujours vivant"
    );

    let output = run_cli(&["--json", "--all"]);
    assert_success(&output);
    let ports: Vec<Value> = serde_json::from_slice(&output.stdout).expect("JSON valide");
    assert!(
        !ports
            .iter()
            .any(|entry| entry["port"].as_u64() == Some(u64::from(port))),
        "le port :{port} est encore en écoute après SIGKILL"
    );
}

/// Processus listener jetable, avec nettoyage garanti.
struct Listener {
    child: Child,
    port: u16,
    // Déclaré en dernier pour être supprimé après l'arrêt du processus,
    // dont ce dossier est le répertoire de travail.
    _project: TempDir,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_listener(project_name: &str) -> Listener {
    spawn_listener_with(project_name, false)
}

fn spawn_listener_with(project_name: &str, ignore_sigterm: bool) -> Listener {
    let project = TempDir::new("listener");
    let manifest = format!(
        r#"{{"name":"{project_name}","dependencies":{{"next":"15.0.0","react":"19.0.0"}}}}"#
    );
    fs::write(project.path().join("package.json"), manifest).expect("écriture du package.json");

    let port_file = project.path().join("port.txt");
    let executable = std::env::current_exe().expect("chemin du binaire de test");

    let mut command = Command::new(executable);
    command
        .args(["--exact", "helper_listener", "--nocapture"])
        .current_dir(project.path())
        .env(PORT_FILE_ENV, &port_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    if ignore_sigterm {
        command.env("LEMONPORT_TEST_IGNORE_SIGTERM", "1");
    }
    #[cfg(not(unix))]
    let _ = ignore_sigterm;

    let mut child = command.spawn().expect("lancement du listener de test");

    let deadline = Instant::now() + Duration::from_secs(10);
    let port = loop {
        if let Ok(contents) = fs::read_to_string(&port_file) {
            if let Ok(port) = contents.trim().parse::<u16>() {
                break port;
            }
        }

        if let Ok(Some(status)) = child.try_wait() {
            panic!("le listener de test s'est arrêté prématurément : {status}");
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("le listener de test n'a pas écrit son port dans les 10 s");
        }

        thread::sleep(Duration::from_millis(50));
    };

    Listener {
        child,
        port,
        _project: project,
    }
}

fn run_cli(arguments: &[&str]) -> Output {
    Command::new(BIN)
        .args(arguments)
        .output()
        .expect("exécution de lemon-port")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "lemon-port a échoué (code {:?})\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            Err(_) => return false,
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Dossier temporaire supprimé automatiquement.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("lemon-port-test-{label}-{}-{id}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("création du dossier temporaire");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
