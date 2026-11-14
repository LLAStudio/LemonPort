//! TUI application state: selection, filter, auto-refresh, toasts, and
//! asynchronous termination tracking.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::killer::{self, KillOutcome, KillReport};
use crate::scanner::{PortEntry, Scanner};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const TOAST_TTL: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    created: Instant,
}

struct PendingKill {
    pid: u32,
    receiver: Receiver<KillReport>,
}

pub struct App {
    scanner: Scanner,
    ports: Vec<PortEntry>,
    hidden_system: usize,
    selected: usize,
    filter: String,
    search_mode: bool,
    show_all: bool,
    should_quit: bool,
    toast: Option<Toast>,
    last_refresh: Instant,
    pending: Option<PendingKill>,
}

impl App {
    pub fn new(show_all: bool) -> Result<Self> {
        let mut scanner = Scanner::new()?;
        let scan = scanner.scan(show_all)?;
        Ok(Self {
            scanner,
            ports: scan.ports,
            hidden_system: scan.hidden_system,
            selected: 0,
            filter: String::new(),
            search_mode: false,
            show_all,
            should_quit: false,
            toast: None,
            last_refresh: Instant::now(),
            pending: None,
        })
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn show_all(&self) -> bool {
        self.show_all
    }

    pub fn hidden_system(&self) -> usize {
        self.hidden_system
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn search_mode(&self) -> bool {
        self.search_mode
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn pending_pid(&self) -> Option<u32> {
        self.pending.as_ref().map(|pending| pending.pid)
    }

    pub fn toast(&self) -> Option<&Toast> {
        self.toast.as_ref()
    }

    /// Visible rows after applying the filter, in sort order.
    pub fn visible(&self) -> Vec<&PortEntry> {
        if self.filter.trim().is_empty() {
            return self.ports.iter().collect();
        }
        let tokens: Vec<String> = self
            .filter
            .to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        self.ports
            .iter()
            .filter(|entry| matches_filter(entry, &tokens))
            .collect()
    }

    /// Handle a key press. Shortcuts do not apply while typing in the filter,
    /// except Escape / Enter / navigation.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        {
            self.should_quit = true;
            return;
        }

        if self.search_mode {
            self.handle_search_key(key);
            return;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Enter | KeyCode::Char('K') => self.kill_selected(),
            KeyCode::Char('o') | KeyCode::Char('O') => self.open_selected(),
            KeyCode::Char('c') | KeyCode::Char('C') => self.copy_selected(),
            KeyCode::Char('r') | KeyCode::Char('R') => self.refresh_manual(),
            KeyCode::Char('/') => {
                self.search_mode = true;
                self.filter.clear();
                self.selected = 0;
            }
            KeyCode::Char(character)
                if !key.modifiers.contains(KeyModifiers::CONTROL) && !character.is_control() =>
            {
                // Direct typing: enter filter mode with the typed character.
                self.search_mode = true;
                self.filter.clear();
                self.filter.push(character);
                self.selected = 0;
            }
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.search_mode = false;
                self.filter.clear();
                self.selected = 0;
            }
            KeyCode::Enter => {
                self.search_mode = false;
                self.kill_selected();
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.selected = 0;
            }
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::Char(character)
                if !key.modifiers.contains(KeyModifiers::CONTROL) && !character.is_control() =>
            {
                self.filter.push(character);
                self.selected = 0;
            }
            _ => {}
        }
    }

    /// Called on every loop iteration: kill results, auto-refresh, toast
    /// expiration.
    pub fn tick(&mut self) {
        if let Some(pending) = self.pending.take() {
            match pending.receiver.try_recv() {
                Ok(report) => self.on_kill_report(report),
                Err(TryRecvError::Empty) => self.pending = Some(pending),
                Err(TryRecvError::Disconnected) => {
                    self.set_toast("Termination failed (thread interrupted)", ToastKind::Error);
                }
            }
        }

        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.reload(false);
        }

        if let Some(toast) = &self.toast {
            if toast.created.elapsed() >= TOAST_TTL {
                self.toast = None;
            }
        }
    }

    fn selected_entry(&self) -> Option<&PortEntry> {
        let visible = self.visible();
        visible.get(self.selected).copied()
    }

    fn move_selection(&mut self, delta: isize) {
        let length = self.visible().len();
        if length == 0 {
            self.selected = 0;
            return;
        }
        let next = (self.selected as isize + delta).clamp(0, length as isize - 1);
        self.selected = next as usize;
    }

    fn kill_selected(&mut self) {
        if self.pending.is_some() {
            self.set_toast("A termination is already in progress…", ToastKind::Info);
            return;
        }

        let Some((pid, port, process)) = self.selected_entry().and_then(|entry| {
            entry
                .pid
                .map(|pid| (pid, entry.port, entry.process.clone()))
        }) else {
            self.set_toast("PID not found — protected process", ToastKind::Error);
            return;
        };

        match killer::spawn_terminate(pid, port, process.clone()) {
            Ok(receiver) => {
                self.pending = Some(PendingKill { pid, receiver });
                self.set_toast(
                    format!("Stopping — {process} (:{port}, PID {pid})…"),
                    ToastKind::Info,
                );
            }
            Err(error) => self.set_toast(
                format!("Could not start termination: {error}"),
                ToastKind::Error,
            ),
        }
    }

    fn on_kill_report(&mut self, report: KillReport) {
        match report.outcome {
            KillOutcome::Terminated { escalated } => {
                let method = if escalated { " (SIGKILL)" } else { "" };
                self.set_toast(
                    format!(
                        ":{} freed — {} (PID {}){method}",
                        report.port, report.process, report.pid
                    ),
                    ToastKind::Success,
                );
            }
            KillOutcome::AlreadyGone => self.set_toast(
                format!("Process {} had already stopped", report.pid),
                ToastKind::Info,
            ),
            KillOutcome::Failed(message) => self.set_toast(
                format!("Failed on :{} — {message}", report.port),
                ToastKind::Error,
            ),
        }
        // Immediately reflect whether the port disappeared.
        self.reload(false);
    }

    fn open_selected(&mut self) {
        let Some(port) = self.selected_entry().map(|entry| entry.port) else {
            self.set_toast("No port selected", ToastKind::Info);
            return;
        };
        let url = format!("http://localhost:{port}");
        if let Some(error) = external_action_error() {
            self.set_toast(format!("Could not open browser: {error}"), ToastKind::Error);
            return;
        }
        if external_actions_disabled() {
            self.set_toast(format!("Opened {url}"), ToastKind::Success);
            return;
        }
        match open::that(&url) {
            Ok(()) => self.set_toast(format!("Opened {url}"), ToastKind::Success),
            Err(error) => {
                self.set_toast(format!("Could not open browser: {error}"), ToastKind::Error)
            }
        }
    }

    fn copy_selected(&mut self) {
        let Some(port) = self.selected_entry().map(|entry| entry.port) else {
            self.set_toast("No port selected", ToastKind::Info);
            return;
        };
        let url = format!("http://localhost:{port}");
        if let Some(error) = external_action_error() {
            self.set_toast(format!("Clipboard: {error}"), ToastKind::Error);
            return;
        }
        if external_actions_disabled() {
            self.set_toast(format!("URL copied: {url}"), ToastKind::Success);
            return;
        }
        match copy_to_clipboard(&url) {
            Ok(()) => self.set_toast(format!("URL copied: {url}"), ToastKind::Success),
            Err(error) => self.set_toast(format!("Clipboard: {error}"), ToastKind::Error),
        }
    }

    fn refresh_manual(&mut self) {
        self.reload(true);
        if self.toast.is_none() {
            self.set_toast("Ports refreshed", ToastKind::Info);
        }
    }

    fn reload(&mut self, clear_cache: bool) {
        if clear_cache {
            self.scanner.clear_project_cache();
        }

        let selected_key = self.selected_entry().map(|entry| (entry.port, entry.pid));

        match self.scanner.scan(self.show_all) {
            Ok(scan) => {
                self.ports = scan.ports;
                self.hidden_system = scan.hidden_system;

                let visible = self.visible();
                let position = selected_key.and_then(|key| {
                    visible
                        .iter()
                        .position(|entry| (entry.port, entry.pid) == key)
                });
                self.selected = position.unwrap_or(0).min(visible.len().saturating_sub(1));
            }
            Err(error) => {
                self.set_toast(format!("Scan failed: {error}"), ToastKind::Error);
            }
        }

        self.last_refresh = Instant::now();
    }

    fn set_toast(&mut self, message: impl Into<String>, kind: ToastKind) {
        self.toast = Some(Toast {
            message: message.into(),
            kind,
            created: Instant::now(),
        });
    }
}

/// Bypass external actions in PTY tests, which must stay hermetic (no browser
/// or clipboard on CI runners).
fn external_actions_disabled() -> bool {
    #[cfg(debug_assertions)]
    {
        std::env::var_os("LEMONPORT_TEST_DISABLE_EXTERNAL").is_some()
    }
    #[cfg(not(debug_assertions))]
    {
        false
    }
}

#[cfg(debug_assertions)]
fn external_action_error() -> Option<&'static str> {
    std::env::var_os("LEMONPORT_TEST_FAIL_EXTERNAL").map(|_| "simulated failure")
}

#[cfg(not(debug_assertions))]
fn external_action_error() -> Option<&'static str> {
    None
}

fn matches_filter(entry: &PortEntry, tokens: &[String]) -> bool {
    let mut haystack = format!(":{} {} {}", entry.port, entry.process, entry.address);
    if let Some(pid) = entry.pid {
        haystack.push(' ');
        haystack.push_str(&pid.to_string());
    }
    if let Some(project) = &entry.project {
        haystack.push(' ');
        haystack.push_str(&project.name);
        haystack.push(' ');
        haystack.push_str(project.label());
    }
    let haystack = haystack.to_lowercase();
    tokens.iter().all(|token| haystack.contains(token))
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        pipe_to("pbcopy", &[], text)
    }

    #[cfg(target_os = "windows")]
    {
        pipe_to("clip", &[], text)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let candidates: [(&str, &[&str]); 3] = [
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ];
        let mut last_error = "no clipboard tool found (install wl-clipboard or xclip)".to_string();
        for (command, args) in candidates {
            match pipe_to(command, args, text) {
                Ok(()) => return Ok(()),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = text;
        Err("clipboard not supported on this platform".to_string())
    }
}

fn pipe_to(command: &str, args: &[&str], text: &str) -> Result<(), String> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("{command} indisponible : {error}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| format!("write to {command}: {error}"))?;
    }

    let status = child
        .wait()
        .map_err(|error| format!("{command} : {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command} failed"))
    }
}
