//! End-to-end integration tests.
//!
//! Each test starts a real process listening on a TCP port, then drives the
//! `lemon-port` binary as a user would:
//! - `--json` must find the port, PID, project, and metrics;
//! - one-shot mode must kill the process and free the port;
//! - a process ignoring SIGTERM must eventually receive SIGKILL (Unix).
//!
//! The listener process is the test binary itself, relaunched with the
//! `helper_listener` filter: no external interpreter (Python, Node…) is
//! required, and the test stays cross-platform.

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

/// Compiled binary path provided by Cargo to integration tests.
const BIN: &str = env!("CARGO_BIN_EXE_lemon-port");
const PORT_FILE_ENV: &str = "LEMONPORT_TEST_PORT_FILE";

#[test]
fn helper_listener() {
    // Normally this test does nothing: it only becomes a listener when
    // relaunched by `spawn_listener`.
    let Some(port_file) = std::env::var_os(PORT_FILE_ENV) else {
        return;
    };

    // On Unix, this listener can deliberately ignore SIGTERM to test
    // escalation to SIGKILL. The handler is armed before bind, so it is
    // already ignored as soon as the port is published.
    #[cfg(unix)]
    if std::env::var_os("LEMONPORT_TEST_IGNORE_SIGTERM").is_some() {
        // SAFETY: SIG_IGN is a valid, permanent handler and touches no Rust
        // state.
        unsafe {
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
        }
    }

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind listener");
    let port = listener
        .local_addr()
        .expect("listener local address")
        .port();
    fs::write(port_file, port.to_string()).expect("write port");

    // Stay in LISTEN until the parent test (or LemonPort) kills the process.
    for stream in listener.incoming() {
        let _ = stream;
    }
}

#[test]
fn scan_and_json_detect_listener_and_project() {
    let listener = spawn_listener("LemonPortTestProject");
    let output = run_cli(&["--json", "--all"]);
    assert_success(&output);

    let ports: Vec<Value> = serde_json::from_slice(&output.stdout).expect("valid JSON");
    let entry = ports
        .iter()
        .find(|entry| entry["port"].as_u64() == Some(u64::from(listener.port)))
        .expect("the listener port must appear in the JSON");

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

// This test opens the real port 1023: it runs in a root CI container.
#[cfg(unix)]
#[test]
fn system_ports_are_hidden_without_all_and_present_with_all() {
    if !is_root() {
        eprintln!("test skipped: port 1023 requires root (dedicated CI container)");
        return;
    }

    let _listener = std::net::TcpListener::bind(("127.0.0.1", 1023))
        .expect("bind privileged port 1023 (root required)");
    let port = 1023_u16;

    let hidden = run_cli(&["--json"]);
    assert_success(&hidden);
    let ports: Vec<Value> = serde_json::from_slice(&hidden.stdout).expect("valid JSON");
    assert!(
        !ports
            .iter()
            .any(|entry| entry["port"].as_u64() == Some(u64::from(port))),
        "the system port must be hidden without --all"
    );

    let all = run_cli(&["--json", "--all"]);
    assert_success(&all);
    let ports: Vec<Value> = serde_json::from_slice(&all.stdout).expect("valid JSON");
    let entry = ports
        .iter()
        .find(|entry| entry["port"].as_u64() == Some(u64::from(port)))
        .expect("the system port must appear with --all");
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
        "confirmation expected for :{port}, stdout: {stdout}"
    );
    assert!(
        stdout.contains("freed"),
        "free confirmation expected, stdout: {stdout}"
    );

    assert!(
        wait_for_exit(&mut listener.child, Duration::from_secs(5)),
        "the listener is still alive after one-shot"
    );

    let output = run_cli(&["--json", "--all"]);
    assert_success(&output);
    let ports: Vec<Value> = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(
        !ports
            .iter()
            .any(|entry| entry["port"].as_u64() == Some(u64::from(port))),
        "port :{port} is still listening after the kill"
    );
}

/// Verify the clean sequence: SIGTERM ignored, grace period, then SIGKILL.
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
        "SIGKILL expected after the grace period, stdout: {stdout}"
    );
    assert!(
        elapsed >= Duration::from_millis(1400),
        "SIGKILL sent too early ({elapsed:?}): SIGTERM was not ignored"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "escalade anormalement lente : {elapsed:?}"
    );
    assert!(
        wait_for_exit(&mut listener.child, Duration::from_secs(5)),
        "the process ignoring SIGTERM is still alive"
    );

    let output = run_cli(&["--json", "--all"]);
    assert_success(&output);
    let ports: Vec<Value> = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert!(
        !ports
            .iter()
            .any(|entry| entry["port"].as_u64() == Some(u64::from(port))),
        "port :{port} is still listening after SIGKILL"
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
        "incomplete initial screen:\n{}",
        tui.screen()
    );

    tui.send(b"/tui-project-a");
    assert!(
        tui.wait_for(
            |screen| screen.contains("/tui-project-a") && !screen.contains("tui-project-b"),
            Duration::from_secs(5)
        ),
        "the filter did not isolate project A:\n{}",
        tui.screen()
    );

    // Search mode: kill the selected row (filtered project A).
    tui.send(b"\r");
    assert!(
        tui.wait_for(|screen| screen.contains("freed"), Duration::from_secs(8)),
        "no free toast:\n{}",
        tui.screen()
    );
    assert!(
        wait_for_exit(&mut first.child, Duration::from_secs(5)),
        "the filtered listener was not killed"
    );
    assert!(
        second.child.try_wait().ok().flatten().is_none(),
        "the wrong listener was killed"
    );

    tui.send(b"q");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "the TUI did not exit"
    );
}

#[test]
fn tui_auto_refresh_detects_new_listener() {
    let mut tui = TuiSession::spawn();
    assert!(
        tui.wait_for(
            |screen| screen.contains("LemonPort"),
            Duration::from_secs(10)
        ),
        "incomplete initial screen:\n{}",
        tui.screen()
    );

    let initial = tui.screen();
    assert!(
        !initial.contains("tui-auto-refresh-project"),
        "project must not exist before creation:\n{initial}"
    );

    let listener = spawn_listener("tui-auto-refresh-project");
    let port = format!(":{}", listener.port);
    assert!(
        tui.wait_for(
            |screen| screen.contains(&port) && screen.contains("tui-auto-refresh-project"),
            Duration::from_secs(10)
        ),
        "listener created after startup was not detected:\n{}",
        tui.screen()
    );

    tui.send(b"q");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "the TUI did not exit"
    );
}

#[test]
fn tui_auto_refresh_adds_multiple_listeners() {
    let mut tui = TuiSession::spawn();
    assert!(
        tui.wait_for(
            |screen| screen.contains("LemonPort"),
            Duration::from_secs(10)
        ),
        "incomplete initial screen:\n{}",
        tui.screen()
    );

    let names = ["tui-batch-one", "tui-batch-two", "tui-batch-three"];
    let initial = tui.screen();
    for name in names {
        assert!(
            !initial.contains(name),
            "project {name} must not exist before creation:\n{initial}"
        );
    }

    let listeners: Vec<Listener> = names.iter().map(|name| spawn_listener(name)).collect();
    let ports: Vec<String> = listeners
        .iter()
        .map(|listener| format!(":{}", listener.port))
        .collect();

    assert!(
        tui.wait_for(
            |screen| {
                names.iter().all(|name| screen.contains(*name))
                    && ports.iter().all(|port| screen.contains(port.as_str()))
            },
            Duration::from_secs(10)
        ),
        "all three new listeners must appear together:\n{}",
        tui.screen()
    );

    tui.send(b"q");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "the TUI did not exit"
    );
}

#[test]
fn tui_auto_refresh_respects_active_filter() {
    let hidden = spawn_listener("tui-filter-drop");
    let hidden_port = format!(":{}", hidden.port);
    let mut tui = TuiSession::spawn();

    assert!(
        tui.wait_for(
            |screen| screen.contains("LemonPort") && screen.contains(&hidden_port),
            Duration::from_secs(10)
        ),
        "the out-of-filter listener must be visible before filtering:\n{}",
        tui.screen()
    );

    tui.send(b"/tui-filter-keep");
    assert!(
        tui.wait_for(
            |screen| screen.contains("/tui-filter-keep"),
            Duration::from_secs(3)
        ),
        "the filter must be active:\n{}",
        tui.screen()
    );

    let filtered = tui.screen();
    assert!(
        !filtered.contains(&hidden_port) && !filtered.contains("tui-filter-drop"),
        "the out-of-filter listener must be hidden:\n{filtered}"
    );

    let kept = spawn_listener("tui-filter-keep");
    let kept_port = format!(":{}", kept.port);
    assert!(
        tui.wait_for(
            |screen| screen.contains(&kept_port) && screen.contains("tui-filter-keep"),
            Duration::from_secs(10)
        ),
        "the new matching listener must appear:\n{}",
        tui.screen()
    );

    let screen = tui.screen();
    assert!(
        screen.contains("/tui-filter-keep"),
        "the filter must remain active after auto-refresh:\n{screen}"
    );
    assert!(
        !screen.contains(&hidden_port) && !screen.contains("tui-filter-drop"),
        "the out-of-filter listener must stay hidden after auto-refresh:\n{screen}"
    );

    // Ctrl+C exits even while filter input is active.
    tui.send(b"\x03");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "the TUI did not exit"
    );
}

#[test]
fn tui_ctrl_c_quits() {
    let mut tui = TuiSession::spawn();
    assert!(
        tui.wait_for(
            |screen| screen.contains("LemonPort"),
            Duration::from_secs(10)
        ),
        "incomplete initial screen:\n{}",
        tui.screen()
    );

    tui.send(b"\x03");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "Ctrl+C must exit the TUI"
    );
}

#[test]
fn tui_escape_clears_filter() {
    let first = spawn_listener("tui-esc-first");
    let second = spawn_listener("tui-esc-second");
    let first_port = format!(":{}", first.port);
    let second_port = format!(":{}", second.port);
    let mut tui = TuiSession::spawn();

    assert!(
        tui.wait_for(
            |screen| screen.contains(&first_port) && screen.contains(&second_port),
            Duration::from_secs(10)
        ),
        "both listeners must be visible:\n{}",
        tui.screen()
    );

    tui.send(b"/tui-esc-first");
    assert!(
        tui.wait_for(
            |screen| screen.contains("/tui-esc-first") && !screen.contains(&second_port),
            Duration::from_secs(3)
        ),
        "the filter must hide the second listener:\n{}",
        tui.screen()
    );

    tui.send(b"\x1b");
    assert!(
        tui.wait_for(
            |screen| screen.contains(&first_port)
                && screen.contains(&second_port)
                && screen.contains("tui-esc-first")
                && screen.contains("tui-esc-second")
                && !screen.contains("/tui-esc-first"),
            Duration::from_secs(3)
        ),
        "Escape must clear the filter and restore the full list:\n{}",
        tui.screen()
    );

    tui.send(b"q");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "the TUI did not exit"
    );
}

#[cfg(unix)]
#[test]
fn tui_header_shows_hidden_system_port_count() {
    if !is_root() {
        eprintln!("test skipped: port 1023 requires root (dedicated CI container)");
        return;
    }

    let mut tui = TuiSession::spawn_with_args_env(&[], &[]);
    assert!(
        tui.wait_for(
            |screen| screen.contains("LemonPort"),
            Duration::from_secs(10)
        ),
        "incomplete initial screen:\n{}",
        tui.screen()
    );

    let baseline = hidden_system_count(&tui.screen()).unwrap_or(0);
    let expected = baseline + 1;

    // The listener stays in the test process: it must only increment the
    // hidden counter without appearing in the table.
    let _hidden = std::net::TcpListener::bind(("127.0.0.1", 1023))
        .expect("bind privileged port 1023 (root required)");

    assert!(
        tui.wait_for(
            |screen| hidden_system_count(screen) == Some(expected) && !screen.contains(":1023"),
            Duration::from_secs(10)
        ),
        "the header must show {expected} hidden system port(s):\n{}",
        tui.screen()
    );
    assert!(
        tui.screen().contains("--all to show everything"),
        "the header must suggest using --all:\n{}",
        tui.screen()
    );

    tui.send(b"q");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "the TUI did not exit"
    );
}

#[cfg(unix)]
fn hidden_system_count(screen: &str) -> Option<usize> {
    let marker = " system port";
    let index = screen.find(marker)?;
    screen[..index].split_whitespace().last()?.parse().ok()
}

#[cfg(unix)]
fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions and modifies no state.
    unsafe { libc::geteuid() == 0 }
}

#[test]
fn tui_auto_refresh_removes_stopped_listener() {
    let mut listener = spawn_listener("tui-stopped-project");
    let port = format!(":{}", listener.port);
    let mut tui = TuiSession::spawn();

    assert!(
        tui.wait_for(
            |screen| screen.contains(&port) && screen.contains("tui-stopped-project"),
            Duration::from_secs(10)
        ),
        "the initial listener must be visible:\n{}",
        tui.screen()
    );

    listener.child.kill().expect("stop listener");
    listener.child.wait().expect("wait for listener stop");

    assert!(
        tui.wait_for(
            |screen| !screen.contains(&port) && !screen.contains("tui-stopped-project"),
            Duration::from_secs(10)
        ),
        "the stopped listener is still shown after auto-refresh:\n{}",
        tui.screen()
    );

    tui.send(b"q");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "the TUI did not exit"
    );
}

#[cfg(debug_assertions)]
#[test]
fn tui_shortcuts_and_keyboard_navigation() {
    let first = spawn_listener("tui-nav-first");
    let second = spawn_listener("tui-nav-second");
    let mut tui = TuiSession::spawn();

    let first_port = format!(":{}", first.port);
    let second_port = format!(":{}", second.port);
    assert!(
        tui.wait_for(
            |screen| screen.contains(&first_port) && screen.contains(&second_port),
            Duration::from_secs(10)
        ),
        "both listeners must be visible:\n{}",
        tui.screen()
    );

    let initial_port = selected_port(&tui.screen()).expect("a port must be selected");

    tui.send(b"j");
    assert!(
        tui.wait_for(
            |screen| selected_port(screen) != Some(initial_port),
            Duration::from_secs(3)
        ),
        "j must move the selection down:\n{}",
        tui.screen()
    );
    tui.send(b"k");
    assert!(
        tui.wait_for(
            |screen| selected_port(screen) == Some(initial_port),
            Duration::from_secs(3)
        ),
        "k must move the selection up:\n{}",
        tui.screen()
    );

    tui.send(b"\x1b[B");
    assert!(
        tui.wait_for(
            |screen| selected_port(screen) != Some(initial_port),
            Duration::from_secs(3)
        ),
        "↓ must move the selection:\n{}",
        tui.screen()
    );
    tui.send(b"\x1b[A");
    assert!(
        tui.wait_for(
            |screen| selected_port(screen) == Some(initial_port),
            Duration::from_secs(3)
        ),
        "↑ must move the selection up:\n{}",
        tui.screen()
    );

    tui.send(b"r");
    assert!(
        tui.wait_for(
            |screen| screen.contains("Ports refreshed"),
            Duration::from_secs(3)
        ),
        "R must show the refresh toast:\n{}",
        tui.screen()
    );

    let url = format!("http://localhost:{initial_port}");
    tui.send(b"o");
    assert!(
        tui.wait_for(
            |screen| screen.contains("Opened") && screen.contains(&url),
            Duration::from_secs(3)
        ),
        "O must open the selected port:\n{}",
        tui.screen()
    );

    tui.send(b"c");
    assert!(
        tui.wait_for(
            |screen| screen.contains("URL copied") && screen.contains(&url),
            Duration::from_secs(3)
        ),
        "C must copy the selected port URL:\n{}",
        tui.screen()
    );

    tui.send(b"q");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "the TUI did not exit"
    );
}

#[cfg(debug_assertions)]
#[test]
fn tui_uppercase_shortcuts() {
    let listener = spawn_listener("tui-uppercase-shortcuts");
    let port = format!(":{}", listener.port);
    let mut tui = TuiSession::spawn();

    assert!(
        tui.wait_for(
            |screen| screen.contains(&port) && screen.contains("tui-uppercase-shortcuts"),
            Duration::from_secs(10)
        ),
        "the listener must be visible:\n{}",
        tui.screen()
    );

    let selected = selected_port(&tui.screen()).expect("a port must be selected");
    let url = format!("http://localhost:{selected}");

    tui.send(b"R");
    assert!(
        tui.wait_for(
            |screen| screen.contains("Ports refreshed"),
            Duration::from_secs(3)
        ),
        "uppercase R must refresh the ports:\n{}",
        tui.screen()
    );

    tui.send(b"O");
    assert!(
        tui.wait_for(
            |screen| screen.contains("Opened") && screen.contains(&url),
            Duration::from_secs(3)
        ),
        "uppercase O must open the selected port:\n{}",
        tui.screen()
    );

    tui.send(b"C");
    assert!(
        tui.wait_for(
            |screen| screen.contains("URL copied") && screen.contains(&url),
            Duration::from_secs(3)
        ),
        "uppercase C must copy the selected port URL:\n{}",
        tui.screen()
    );

    tui.send(b"q");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "the TUI did not exit"
    );
}

#[cfg(debug_assertions)]
#[test]
fn tui_shortcuts_report_external_action_failures() {
    let listener = spawn_listener("tui-external-failure");
    let port = format!(":{}", listener.port);
    let mut tui = TuiSession::spawn_with_env(&[("LEMONPORT_TEST_FAIL_EXTERNAL", "1")]);

    assert!(
        tui.wait_for(|screen| screen.contains(&port), Duration::from_secs(10)),
        "the listener must be visible:\n{}",
        tui.screen()
    );

    tui.send(b"o");
    assert!(
        tui.wait_for(
            |screen| screen.contains("Could not open browser"),
            Duration::from_secs(3)
        ),
        "O must show the open error:\n{}",
        tui.screen()
    );

    tui.send(b"c");
    assert!(
        tui.wait_for(
            |screen| screen.contains("Clipboard:"),
            Duration::from_secs(3)
        ),
        "C must show the clipboard error:\n{}",
        tui.screen()
    );

    tui.send(b"q");
    assert!(
        tui.wait_for_exit(Duration::from_secs(5)),
        "the TUI did not exit"
    );
}

#[cfg(debug_assertions)]
fn selected_port(screen: &str) -> Option<u16> {
    let marker = "▸ :";
    let start = screen.find(marker)? + marker.len();
    let digits: String = screen[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// TUI launched in a real pseudo-terminal, with a vt100 screen.
struct TuiSession {
    child: Box<dyn PtyChild + Send>,
    writer: Box<dyn Write + Send>,
    _master: Box<dyn MasterPty + Send>,
    output: Receiver<Vec<u8>>,
    parser: vt100::Parser,
}

impl TuiSession {
    fn spawn() -> Self {
        Self::spawn_with_args_env(&["--all"], &[])
    }

    fn spawn_with_env(extra_env: &[(&str, &str)]) -> Self {
        Self::spawn_with_args_env(&["--all"], extra_env)
    }

    fn spawn_with_args_env(arguments: &[&str], extra_env: &[(&str, &str)]) -> Self {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 32,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open the pseudo-terminal");

        let mut command = CommandBuilder::new(BIN);
        for argument in arguments {
            command.arg(argument);
        }
        command.env("TERM", "xterm-256color");
        command.env("LEMONPORT_TEST_DISABLE_EXTERNAL", "1");
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let child = pair
            .slave
            .spawn_command(command)
            .expect("launch the TUI in the pty");

        let mut reader = pair
            .master
            .try_clone_reader()
            .expect("read the pseudo-terminal");
        let writer = pair
            .master
            .take_writer()
            .expect("write to the pseudo-terminal");
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
        self.writer.write_all(bytes).expect("write pty");
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

/// Disposable listener process with guaranteed cleanup.
struct Listener {
    child: Child,
    port: u16,
    // Declared last so it is removed after the process stops; this folder is
    // the process working directory.
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
    fs::write(project.path().join("package.json"), manifest).expect("write package.json");

    let port_file = project.path().join("port.txt");
    let executable = std::env::current_exe().expect("test binary path");

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

    let mut child = command.spawn().expect("launch the test listener");

    let deadline = Instant::now() + Duration::from_secs(10);
    let port = loop {
        if let Ok(contents) = fs::read_to_string(&port_file) {
            if let Ok(port) = contents.trim().parse::<u16>() {
                break port;
            }
        }

        if let Ok(Some(status)) = child.try_wait() {
            panic!("the test listener exited early: {status}");
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the test listener did not write its port within 10 s");
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
    command.output().expect("run lemon-port")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "lemon-port failed (code {:?})\nstdout: {}\nstderr: {}",
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

/// Temporary directory removed automatically.
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
        fs::create_dir_all(&path).expect("create temporary directory");
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
