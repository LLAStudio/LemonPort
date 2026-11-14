# 🍋 LemonPort

[![Release](https://img.shields.io/github/v/release/LLAStudio/LemonPort?color=FACC15&label=release)](https://github.com/LLAStudio/LemonPort/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-84CC16.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-FACC15.svg)](https://www.rust-lang.org)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-84CC16.svg)](#compatibility--limits)

> **The `kill -9` for your ports, lemon-style.**
> A tiny LLAStudio utility: zero configuration, instant startup, clean terminal UI.

LemonPort lists listening TCP ports, finds the **real project name** behind each process
(`PipeCraft (Next.js)` instead of `node`), and frees a port with one key. Three modes:
interactive TUI, scriptable one-shot, and JSON.

```text
🍋 LemonPort  by LLAStudio                                        3 active ports

TCP · LISTEN · auto-scan 2 s                                    / filter
────────────────────────────────────────────────────────────────────────────
╭─ LISTENING PORTS 3 ──────────────────────────────────────────────────────╮
│ PORT      PROJECT / PROCESS                 PID     RAM      STATUS      │
│ ▸ :3000   PipeCraft (Next.js) · node       18234    92 MB    ACTIVE 2h14  │
│   :5432   api (FastAPI) · python3           9123   118 MB    ACTIVE 6h02  │
│   :8080   blog (Rust) · cargo-watch         4410    24 MB    ACTIVE 12m   │
╰──────────────────────────────────────────────────────────────────────────╯

 ● Stopping — node (:3000, PID 18234)…
 ↑↓ navigate  Enter/K kill  / filter  O open  C copy  R refresh  Q quit
```

---

## Table of contents

- [Features](#features)
- [Installation](#installation)
  - [Script — Linux & macOS](#script--linux--macos)
  - [Script — Windows](#script--windows)
  - [From source](#from-source)
  - [From the repository, without cloning](#from-the-repository-without-cloning)
  - [`lp` alias](#lp-alias)
- [Usage](#usage)
- [TUI shortcuts](#tui-shortcuts)
- [Project detection](#project-detection)
- [JSON output](#json-output)
- [Building release binaries](#building-release-binaries)
- [Architecture](#architecture)
- [Compatibility & limits](#compatibility--limits)
- [License](#license)

---

## Features

- **Instant TUI** (`ratatui` + `crossterm`): listening TCP ports, with development ports
  sorted first.
- **One-shot**: `lemon-port 3000` kills the process, prints one confirmation line, and
  returns a script-friendly exit code.
- **JSON**: `lemon-port --json` for dashboards, Makefiles, CI, and custom tools.
- **Real project names**: resolve the process `cwd` and walk up to 3 parent directories
  to find `package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, or `composer.json`.
  Frameworks are detected in the dependencies.
- **Enrichment**: PID, binary, resident RAM, uptime, bind address.
- **Clean termination**: `SIGTERM`, a 1.5 s wait, then `SIGKILL` if needed
  (`TerminateProcess` on Windows). The TUI stays responsive while waiting.
- **Zero configuration**, zero config files, zero accounts. One standalone binary.
- **Cross-platform**: Linux (x86_64/aarch64), macOS (Intel/Apple Silicon), Windows (x64).
- **Open source**, MIT license, no telemetry.

## Installation

> Script commands download a binary from
> [GitHub releases](https://github.com/LLAStudio/LemonPort/releases).
> If no release is published yet, install from source (`--source` or `cargo install --git`).

### Script — Linux & macOS

```bash
curl -fsSL https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.sh | sh
```

The script detects the OS and architecture, verifies the SHA-256, installs `lemon-port` and
the `lp` alias in `~/.local/bin`, then prints the line to add to `PATH` if needed.

Options (pass them after `sh -s --`):

```bash
# From source (requires Rust)
curl -fsSL https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.sh | sh -s -- --source

# Specific version or directory
curl -fsSL https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.sh | sh -s -- --version 0.1.0 --dir "$HOME/bin"
```

| Option | Purpose |
| --- | --- |
| `--source` | Build and install from the repository with Cargo |
| `--version <v>` | Install a specific version (e.g. `0.1.0` or `v0.1.0`) |
| `--dir <path>` | Installation directory (default: `~/.local/bin`) |
| `--help` | Show help |

Equivalent variables: `LEMONPORT_VERSION`, `LEMONPORT_INSTALL_DIR`, `LEMONPORT_REPO`.

### Script — Windows

In PowerShell:

```powershell
irm https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.ps1 | iex
```

The script installs `lemon-port.exe` and `lp.exe` in
`%LOCALAPPDATA%\LemonPort\bin`, verifies the SHA-256, and adds that directory to the user
`PATH` (a new terminal is required).

```powershell
# From source
$env:LEMONPORT_FROM_SOURCE = "1"; irm https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.ps1 | iex

# Specific version or directory via environment variables
$env:LEMONPORT_VERSION = "0.1.0"
$env:LEMONPORT_INSTALL_DIR = "$HOME\bin"
irm https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.ps1 | iex
```

### From source

Common prerequisite: **Rust 1.95+** via [rustup](https://rustup.rs).

<table>
<tr><th>Linux</th><th>macOS</th><th>Windows</th></tr>
<tr><td>

```bash
# Debian / Ubuntu
sudo apt install build-essential

# Fedora
sudo dnf group install "Development Tools"

# Arch
sudo pacman -S base-devel
```

</td><td>

```bash
xcode-select --install
```

</td><td>

Install **Visual Studio Build Tools** with the
“Desktop development with C++” workload, then:

```powershell
rustup default stable-msvc
```

</td></tr>
</table>

Then, on all three platforms:

```bash
git clone https://github.com/LLAStudio/LemonPort.git
cd LemonPort
cargo install --path .
```

`cargo install` installs **both binaries**: `lemon-port` and `lp`.
The binary lands in `~/.cargo/bin` (Linux/macOS) or `%USERPROFILE%\.cargo\bin`
(Windows), already on `PATH` when rustup is used.

### From the repository, without cloning

```bash
cargo install --git https://github.com/LLAStudio/LemonPort --locked
```

Same result as a local installation, without cloning the repository.

### `lp` alias

The alias is provided automatically by `cargo install` and by the install scripts.
If you copy the binary manually:

```bash
# Linux / macOS
ln -s lemon-port /usr/local/bin/lp
```

```powershell
# Windows
Copy-Item "$env:LOCALAPPDATA\LemonPort\bin\lemon-port.exe" "$env:LOCALAPPDATA\LemonPort\bin\lp.exe"
```

## Usage

### Interactive TUI (default)

```bash
lemon-port
```

Lists listening TCP ports with an auto-refresh every 2 seconds.
System ports (< 1024) are hidden by default; `--all` shows them.

```bash
lemon-port --all
```

### One-shot

```bash
lemon-port 3000
# 🍋 :3000 freed — node (PID 18234) · PipeCraft (Next.js)
```

- Exit code `0`: port freed (SIGTERM or SIGKILL).
- Exit code `1`: no process, protected PID, permission denied, or process resisting.
- Exit code `2`: invalid arguments (clap).

Termination is clean: `SIGTERM`, then `SIGKILL` after 1.5 s if the process resists.

### JSON

```bash
lemon-port --json
lemon-port --json --all
```

### Help

```bash
lemon-port --help
lemon-port --version
```

## TUI shortcuts

| Key | Action |
| --- | --- |
| `↑` / `↓`, `k` / `j` | Navigate the list |
| `Enter` or `K` | Kill the selected process (`SIGTERM` → `SIGKILL` after 1.5 s) |
| `O` | Open `http://localhost:<PORT>` in the default browser |
| `C` | Copy `http://localhost:<PORT>` to the clipboard |
| `R` | Refresh immediately (in addition to the 2 s auto-refresh) |
| `/` | Enable the live filter (name, project, port, PID) |
| Direct typing | Start the filter with the first typed character |
| `Escape` | Clear the filter; outside the filter, quit |
| `Q` | Quit cleanly (terminal restored) |
| `Ctrl+C` | Quit cleanly |

The clipboard uses `pbcopy` (macOS), `clip` (Windows), then `wl-copy`, `xclip`, or
`xsel` (Linux). If no tool is found, a toast says so and nothing crashes.

## Project detection

For each port, LemonPort starts from the process working directory and walks up to
**3 parent directories** looking for a manifest:

| Manifest | Name extracted | Recognized frameworks (extract) |
| --- | --- | --- |
| `package.json` | `name` | Next.js, Nuxt, SvelteKit, Astro, Remix, Angular, NestJS, Fastify, Hono, Express, Expo, Electron, Vue, Svelte, Solid, React, Vite |
| `Cargo.toml` | `package.name` | Axum, Actix Web, Rocket, Warp, Poem, Salvo, Tide |
| `pyproject.toml` | `project.name` or `tool.poetry.name` | FastAPI, Django, Flask, Streamlit, Gradio, Jupyter, Uvicorn |
| `go.mod` | last segment of `module` | Gin, Fiber, Echo, Chi, Gorilla, Beego |
| `composer.json` | `name` (without the vendor) | Laravel, Symfony, Slim |

A malformed, oversized (> 512 KiB), or unreadable manifest is ignored: detection moves
to the next level and never panics. Processes owned by another user are shown as
`system` / `PROTECTED`, with `—` for inaccessible information.

## JSON output

Array of objects, sorted with development ports first:

```json
[
  {
    "port": 3000,
    "protocol": "TCP",
    "address": "127.0.0.1",
    "pid": 18234,
    "process": "node",
    "project": {
      "name": "PipeCraft",
      "kind": "Node.js",
      "framework": "Next.js",
      "root": "/home/lla/dev/pipecraft"
    },
    "memory_mb": 92.4,
    "uptime_seconds": 8041,
    "system": false,
    "protected": false
  }
]
```

With `--all`, system ports are included. When no process can be identified, `pid` is
`null`, `protected` is `true`, and `project` is omitted.

## Tests

The integration suite starts a real listener process, then verifies scanning, JSON
enrichment (PID, project, framework, RAM, uptime), and port release in one-shot mode:

```bash
cargo test --test integration -- --test-threads=1
```

A test also verifies that system ports (< 1024) are hidden by default and reappear with
`--all`. It opens the real port `1023`: it is skipped outside root, and CI runs it in a
dedicated root Linux container.

An additional Unix test verifies that a process ignoring `SIGTERM` is killed by `SIGKILL`
after the 1.5 s grace period.

A TUI test launches LemonPort in a **real pseudo-terminal** (`portable-pty`), interprets
the screen with `vt100`, filters on a project, then presses `Enter` to kill the selected
row. It verifies the free toast and that the second listener stays alive.
A second PTY test verifies that a listener created after startup appears through the
2 s automatic refresh.
A third PTY test covers `↑`/`↓` and `j`/`k` navigation, plus the `O` (open), `C` (copy),
and `R` (refresh) shortcuts.
A fourth PTY test verifies that a stopped listener disappears from the TUI via auto-refresh.
A fifth PTY test verifies that auto-refresh respects the active filter: a new matching
listener appears while an out-of-filter listener stays hidden.
A sixth PTY test verifies that multiple listeners created between two scans appear
together on the next refresh.
A seventh PTY test verifies the error toasts for `O` and `C` when external actions fail.
An eighth PTY test covers the uppercase `O`, `C`, and `R` variants.
A ninth PTY test verifies that `Ctrl+C` exits the TUI.
A tenth PTY test verifies that Escape clears the filter and restores the full list.
An eleventh PTY test verifies that the header shows the number of hidden system ports and
increments it when a new system port appears. Because it also opens port `1023`, it runs
in the same root container.

Unit tests cover detection for all five manifests (`package.json`, `Cargo.toml`,
`pyproject.toml`, `go.mod`, `composer.json`), including malformed files:

```bash
cargo test --lib
```

CI also publishes an HTML coverage report generated by `cargo llvm-cov` in the
`coverage-report` artifact.

No external interpreter is required: the listener is the test binary itself, relaunched
in helper mode. GitHub CI runs these suites on Linux, macOS, and Windows, plus a Linux root
container job for the tests that open the privileged port `1023`.

## Building release binaries

### Locally

```bash
cargo build --release
./target/release/lemon-port --version
```

On Windows: `target\release\lemon-port.exe`. The release profile enables LTO,
`codegen-units = 1`, and `strip` for a compact binary.

### Automated GitHub releases

The workflow [`.github/workflows/release.yml`](.github/workflows/release.yml) builds and
publishes archives for:

| Target | Archive |
| --- | --- |
| Linux x86_64 (static musl) | `lemon-port-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz` |
| Linux aarch64 | `lemon-port-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `lemon-port-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `lemon-port-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| Windows x64 | `lemon-port-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

Each archive contains the binary, `README.md`, and `LICENSE`. A `SHA256SUMS` file is
attached to the release.

Three ways to publish:

```bash
# 1. Push a tag
git tag v0.1.0
git push origin v0.1.0
```

```text
# 2. GitHub → Actions → Release → Run workflow → version "0.1.0"
```

```bash
# 3. Push a release branch (useful in CI or without tag permissions)
git push origin main:release/v0.1.0
```

The workflow creates (or completes) the GitHub release with generated notes and binaries.
The install scripts point to `releases/latest`.

## Architecture

```text
lemon-port/
├── Cargo.toml
├── Cargo.lock
├── LICENSE
├── README.md
├── install.sh              # Linux / macOS installer
├── install.ps1             # Windows installer
├── .github/workflows/
│   └── release.yml         # Multi-OS build + GitHub release
└── src/
    ├── lib.rs              # CLI (clap), TUI / one-shot / JSON dispatch
    ├── main.rs             # `lemon-port` binary
    ├── bin/lp.rs           # `lp` alias binary
    ├── scanner.rs          # TCP LISTEN sockets + sysinfo enrichment
    ├── project.rs          # cwd → manifest → name + framework
    ├── killer.rs           # SIGTERM → SIGKILL / TerminateProcess
    ├── app.rs              # TUI state, filter, toasts, clipboard
    └── ui.rs               # Ratatui rendering (lemon & lime), event loop
```

Technical choices:

- `netstat2` reads socket tables through native APIs (no `netstat` parsing).
- `sysinfo` provides `cwd`, name, RAM, and uptime without privileges for the user's
  processes.
- Projects are cached by `cwd` so auto-refresh stays lightweight.
- Termination runs in a dedicated thread: the TUI is never frozen by the 1.5 s wait.

## Compatibility & limits

| Platform | Status | Notes |
| --- | --- | --- |
| Linux x86_64 | ✅ | Static musl binary available |
| Linux aarch64 | ✅ | glibc binary |
| macOS Intel / Apple Silicon | ✅ | macOS may deny `cwd` for some protected processes |
| Windows x64 | ✅ | No console `SIGTERM`: direct `TerminateProcess` |
| BSD / others | ⚠️ | May compile with Cargo, not tested |

- Processes owned by `root` (or another user) appear as `PROTECTED`; rerun with `sudo` /
  as administrator to kill them.
- A port shared by multiple processes (`SO_REUSEPORT`) produces one row per PID.
- `docker-proxy` is shown as-is: LemonPort does not resolve container names.
- No special privileges are required for your own processes.

## License

[MIT](LICENSE) © 2026 LLAStudio.
