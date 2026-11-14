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
use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::process::{self, Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child as PtyChild, CommandBuilder, MasterPty, PtySize};
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

// Le hook de classification n'existe que dans les builds debug.
#[cfg(debug_assertions)]
#[test]
fn system_ports_are_hidden_without_all_and_present_with_all() {
    // Les runners non privilégiés ne peuvent pas ouvrir 1023. On essaie un vrai
    // port bas, puis on marque un port éphémère comme système via le hook debug.
    // La limite réelle 1023/1024 est couverte unitairement dans scanner.rs.
    let (_listener, port, override_port) = match std::net::TcpListener::bind(("127.0.0.1", 1023)) {
        Ok(listener) => {
            let port = listener
                .local_addr()
                .expect("adresse du listener bas")
                .port();
            (listener, port, None)
        }
        Err(_) => {
            let listener =
                std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind d'un port éphémère");
            let port = listener.local_addr().expect("adresse du listener").port();
            (listener, port, Some(port))
        }
    };
    assert_eq!(
        override_port.is_some(),
        port >= 1024,
        "le port doit être réellement privilégié ou explicitement marqué comme système"
    );

    let override_text = override_port.map(|port| port.to_string());
    let variables: Vec<(&str, &str)> = override_text
        .as_deref()
        .map(|value| vec![("LEMONPORT_TEST_SYSTEM_PORT", value)])
        .unwrap_or_default();

    let hidden = run_cli_with_env(&["--json"], &variables);
    assert_success(&hidden);
    let ports: Vec<Value> = serde_json::from_slice(&hidden.stdout).expect("JSON valide");
    assert!(
        !ports
            .iter()
            .any(|entry| entry["port"].as_u64() == Some(u64::from(port))),
        "le port système doit être masqué sans --all"
    );

    let all = run_cli_with_env(&["--json", "--all"], &variables);
    assert_success(&all);
    let ports: Vec<Value> = serde_json::from_slice(&all.stdout).expect("JSON valide");
    let entry = ports
        .iter()
        .find(|entry| entry["port"].as_u64() == Some(u64::from(port)))
        .expect("le port système doit apparaître avec --all");
    assert_eq!(entry["system"].as_bool(), Some(true));
    assert_eq!(entry["port"].as_u64(), Some(u64::from(port)));
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

#[test]
fn tui_filter_and_kill_shortcut() {
    let mut first = spawn_listener("tui-project-a");
    let mut second = spawn_listener("tui-project-b");
    let mut tui = TuiSession::spawn();

    let first_port = format!(":{}", first.port);
    let second_port = format!(":{}", second.port);
    assert!(
        tui.wait_for(
            |screen| screen.contains("LemonPort")
                && screen.contains(&first_port)
                && screen.contains(&second_port),
            Duration::from_secs(10)
        ),
        "écran initial incomplet:\n{}",
        tui.screen()
    );

    tui.send(b"/tui-project-a");
    assert!(
        tui.wait_for(
            |screen| screen.contains("/tui-project-a") && !screen.contains("tui-project-b"),
            Duration::from_secs(5)
        ),
        "le filtre n'a pas isolé le projet A:\n{}",
        tui.screen()
    );

    // Entrée en mode recherche : tue la ligne sélectionnée (projet A filtré).
    tui.send(b"\r");
    assert!(
        tui.wait_for(|screen| screen.contains("libéré"), Duration::from_secs(8)),
        "aucun toast de libération:\n{}",
        tui.screen()
    );
    assert!(
        wait_for_exit(&mut first.child, Duration::from_secs(5)),
        "le listener filtré n'a pas été tué"
    );
    assert!(
        second.child.try_wait().ok().flatten().is_none(),
        "le mauvais listener a été tué"
    );

    tui.send(b"q");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "la TUI ne s'est pas arrêtée"
    );
}

/// TUI lancée dans un vrai pseudo-terminal, avec écran vt100.
struct TuiSession {
    child: Box<dyn PtyChild + Send>,
    writer: Box<dyn Write + Send>,
    _master: Box<dyn MasterPty + Send>,
    output: Receiver<Vec<u8>>,
    parser: vt100::Parser,
}

impl TuiSession {
    fn spawn() -> Self {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 32,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("ouverture du pseudo-terminal");

        let mut command = CommandBuilder::new(BIN);
        command.arg("--all");
        command.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(command)
            .expect("lancement de la TUI dans le pty");

        let mut reader = pair
            .master
            .try_clone_reader()
            .expect("lecture du pseudo-terminal");
        let writer = pair
            .master
            .take_writer()
            .expect("écriture vers le pseudo-terminal");
        drop(pair.slave);
        let master = pair.master;

        let (sender, output) = mpsc::channel();
        thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if sender.send(buffer[..count].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let rows = NonZeroU16::new(32).expect("32 lignes");
        let cols = NonZeroU16::new(120).expect("120 colonnes");
        Self {
            child,
            writer,
            _master: master,
            output,
            parser: vt100::Parser::new(rows, cols, 0),
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("écriture pty");
        self.writer.flush().expect("flush pty");
    }

    fn pump(&mut self) {
        while let Ok(chunk) = self.output.try_recv() {
            self.parser.process(&chunk);
        }
    }

    fn screen(&mut self) -> String {
        self.pump();
        self.parser.screen().contents()
    }

    fn wait_for(&mut self, predicate: impl Fn(&str) -> bool, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            self.pump();
            if predicate(&self.parser.screen().contents()) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait() {
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
}

impl Drop for TuiSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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
    run_cli_with_env(arguments, &[])
}

fn run_cli_with_env(arguments: &[&str], variables: &[(&str, &str)]) -> Output {
    let mut command = Command::new(BIN);
    command.args(arguments);
    for (key, value) in variables {
        command.env(key, value);
    }
    command.output().expect("exécution de lemon-port")
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
