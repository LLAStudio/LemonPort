# 🍋 LemonPort

[![Release](https://img.shields.io/github/v/release/LLAStudio/LemonPort?color=FACC15&label=release)](https://github.com/LLAStudio/LemonPort/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-84CC16.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-FACC15.svg)](https://www.rust-lang.org)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-84CC16.svg)](#compatibilité--limites)

> **Le `kill -9` de vos ports, en version citron.**
> Une micro-utilité LLAStudio : zéro configuration, démarrage instantané, interface terminal nette.

LemonPort liste les ports TCP en écoute, retrouve **le vrai nom du projet** derrière chaque
processus (`PipeCraft (Next.js)` plutôt que `node`), et libère un port en une touche.
Trois modes : TUI interactive, one-shot scriptable, JSON.

```text
🍋 LemonPort  by LLAStudio                                        3 ports actifs

TCP · LISTEN · scan auto 2 s                                    / filtrer
────────────────────────────────────────────────────────────────────────────
╭─ PORTS EN ÉCOUTE 3 ──────────────────────────────────────────────────────╮
│ PORT      PROJET / PROCESSUS                PID     RAM      STATUT      │
│ ▸ :3000   PipeCraft (Next.js) · node       18234    92 Mo    ACTIF 2h14  │
│   :5432   api (FastAPI) · python3           9123   118 Mo    ACTIF 6h02  │
│   :8080   blog (Rust) · cargo-watch         4410    24 Mo    ACTIF 12m   │
╰──────────────────────────────────────────────────────────────────────────╯

 ● Arrêt en cours — node (:3000, PID 18234)…
 ↑↓ naviguer  Entrée/K tuer  / filtrer  O ouvrir  C copier  R rafraîchir  Q quitter
```

---

## Sommaire

- [Fonctionnalités](#fonctionnalités)
- [Installation](#installation)
  - [Script — Linux & macOS](#script--linux--macos)
  - [Script — Windows](#script--windows)
  - [Depuis les sources](#depuis-les-sources)
  - [Depuis le dépôt, sans clone](#depuis-le-dépôt-sans-clone)
  - [Alias `lp`](#alias-lp)
- [Utilisation](#utilisation)
- [Raccourcis TUI](#raccourcis-tui)
- [Détection des projets](#détection-des-projets)
- [Sortie JSON](#sortie-json)
- [Construire les binaires de release](#construire-les-binaires-de-release)
- [Architecture](#architecture)
- [Compatibilité & limites](#compatibilité--limites)
- [Licence](#licence)

---

## Fonctionnalités

- **TUI instantanée** (`ratatui` + `crossterm`) : ports TCP en `LISTEN`, triés avec les ports
  de développement en premier.
- **One-shot** : `lemon-port 3000` tue le processus, affiche une ligne de confirmation et
  sort avec un code exploitable dans un script.
- **JSON** : `lemon-port --json` pour les dashboards, Makefiles, CI et outils maison.
- **Vrai nom de projet** : résolution du `cwd` du processus et remontée de 3 niveaux de
  dossiers pour trouver `package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod` ou
  `composer.json`. Framework détecté dans les dépendances.
- **Enrichissement** : PID, binaire, RAM résidente, uptime, adresse de bind.
- **Terminaison propre** : `SIGTERM`, attente de 1,5 s, puis `SIGKILL` si nécessaire
  (`TerminateProcess` sur Windows). La TUI reste fluide pendant l'attente.
- **Zéro configuration**, zéro fichier de config, zéro compte. Un binaire autonome.
- **Cross-platform** : Linux (x86_64/aarch64), macOS (Intel/Apple Silicon), Windows (x64).
- **Open-source**, licence MIT, aucune télémétrie.

## Installation

> Les commandes par script téléchargent un binaire depuis
> [les releases GitHub](https://github.com/LLAStudio/LemonPort/releases).
> Si aucune release n'est encore publiée, utilisez l'installation depuis les sources
> (`--source` ou `cargo install --git`).

### Script — Linux & macOS

```bash
curl -fsSL https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.sh | sh
```

Le script détecte l'OS et l'architecture, vérifie le SHA-256, installe `lemon-port` et
l'alias `lp` dans `~/.local/bin`, puis affiche la ligne à ajouter au `PATH` si nécessaire.

Options (à passer après `sh -s --`) :

```bash
# Depuis les sources (nécessite Rust)
curl -fsSL https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.sh | sh -s -- --source

# Version ou dossier précis
curl -fsSL https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.sh | sh -s -- --version 0.1.0 --dir "$HOME/bin"
```

| Option | Rôle |
| --- | --- |
| `--source` | Compile et installe depuis le dépôt avec Cargo |
| `--version <v>` | Installe une version précise (ex. `0.1.0` ou `v0.1.0`) |
| `--dir <chemin>` | Dossier d'installation (défaut : `~/.local/bin`) |
| `--help` | Affiche l'aide |

Variables équivalentes : `LEMONPORT_VERSION`, `LEMONPORT_INSTALL_DIR`, `LEMONPORT_REPO`.

### Script — Windows

Dans PowerShell :

```powershell
irm https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.ps1 | iex
```

Le script installe `lemon-port.exe` et `lp.exe` dans
`%LOCALAPPDATA%\LemonPort\bin`, vérifie le SHA-256 et ajoute ce dossier au `PATH`
utilisateur (nouveau terminal nécessaire).

```powershell
# Depuis les sources
$env:LEMONPORT_FROM_SOURCE = "1"; irm https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.ps1 | iex

# Version ou dossier précis via variables d'environnement
$env:LEMONPORT_VERSION = "0.1.0"
$env:LEMONPORT_INSTALL_DIR = "$HOME\bin"
irm https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.ps1 | iex
```

### Depuis les sources

Prérequis commun : **Rust 1.95+** via [rustup](https://rustup.rs).

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

Installez **Visual Studio Build Tools** avec la charge
« Développement Desktop en C++ », puis :

```powershell
rustup default stable-msvc
```

</td></tr>
</table>

Ensuite, sur les trois OS :

```bash
git clone https://github.com/LLAStudio/LemonPort.git
cd LemonPort
cargo install --path .
```

`cargo install` installe **les deux binaires** : `lemon-port` et `lp`.
Le binaire arrive dans `~/.cargo/bin` (Linux/macOS) ou `%USERPROFILE%\.cargo\bin`
(Windows), déjà dans le `PATH` si rustup a été utilisé.

### Depuis le dépôt, sans clone

```bash
cargo install --git https://github.com/LLAStudio/LemonPort --locked
```

Même résultat que l'installation locale, sans cloner le dépôt.

### Alias `lp`

L'alias est fourni automatiquement par `cargo install` et par les scripts d'installation.
Si vous copiez le binaire à la main :

```bash
# Linux / macOS
ln -s lemon-port /usr/local/bin/lp
```

```powershell
# Windows
Copy-Item "$env:LOCALAPPDATA\LemonPort\bin\lemon-port.exe" "$env:LOCALAPPDATA\LemonPort\bin\lp.exe"
```

## Utilisation

### TUI interactive (par défaut)

```bash
lemon-port
```

Liste les ports TCP en écoute, avec auto-refresh toutes les 2 secondes.
Les ports système (< 1024) sont masqués par défaut ; `--all` les affiche.

```bash
lemon-port --all
```

### One-shot

```bash
lemon-port 3000
# 🍋 :3000 libéré — node (PID 18234) · PipeCraft (Next.js)
```

- Code de sortie `0` : port libéré (SIGTERM ou SIGKILL).
- Code de sortie `1` : aucun processus, PID protégé, permission refusée ou processus résistant.
- Code de sortie `2` : arguments invalides (clap).

La terminaison est propre : `SIGTERM`, puis `SIGKILL` après 1,5 s si le processus résiste.

### JSON

```bash
lemon-port --json
lemon-port --json --all
```

### Aide

```bash
lemon-port --help
lemon-port --version
```

## Raccourcis TUI

| Touche | Action |
| --- | --- |
| `↑` / `↓`, `k` / `j` | Naviguer dans la liste |
| `Entrée` ou `K` | Tuer le processus sélectionné (`SIGTERM` → `SIGKILL` après 1,5 s) |
| `O` | Ouvrir `http://localhost:<PORT>` dans le navigateur par défaut |
| `C` | Copier l'URL `http://localhost:<PORT>` dans le presse-papier |
| `R` | Rafraîchir immédiatement (en plus de l'auto-refresh 2 s) |
| `/` | Activer le filtre en temps réel (nom, projet, port, PID) |
| Texte direct | Lance le filtre avec le premier caractère tapé |
| `Échap` | Efface le filtre ; hors filtre, quitte |
| `Q` | Quitter proprement (terminal restauré) |
| `Ctrl+C` | Quitter proprement |

Le presse-papier utilise `pbcopy` (macOS), `clip` (Windows), puis `wl-copy`, `xclip` ou
`xsel` (Linux). Aucun outil trouvé : un toast l'indique, rien ne plante.

## Détection des projets

Pour chaque port, LemonPort part du répertoire de travail du processus et remonte jusqu'à
**3 dossiers parents** à la recherche d'un manifeste :

| Manifeste | Nom extrait | Frameworks reconnus (extrait) |
| --- | --- | --- |
| `package.json` | `name` | Next.js, Nuxt, SvelteKit, Astro, Remix, Angular, NestJS, Fastify, Hono, Express, Expo, Electron, Vue, Svelte, Solid, React, Vite |
| `Cargo.toml` | `package.name` | Axum, Actix Web, Rocket, Warp, Poem, Salvo, Tide |
| `pyproject.toml` | `project.name` ou `tool.poetry.name` | FastAPI, Django, Flask, Streamlit, Gradio, Jupyter, Uvicorn |
| `go.mod` | dernier segment du `module` | Gin, Fiber, Echo, Chi, Gorilla, Beego |
| `composer.json` | `name` (sans le vendor) | Laravel, Symfony, Slim |

Un manifeste malformé, trop gros (> 512 Kio) ou illisible est ignoré : on passe au niveau
suivant, jamais de panique. Les processus d'un autre utilisateur affichent
`système` / `PROTÉGÉ`, avec `—` pour les informations inaccessibles.

## Sortie JSON

Tableau d'objets, trié ports de développement d'abord :

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

Avec `--all`, les ports système sont inclus. Sans processus identifiable, `pid` vaut `null`,
`protected` vaut `true`, et `project` est absent.

## Tests

La suite d'intégration lance un vrai processus listener, puis vérifie le scan, l'enrichissement
JSON (PID, projet, framework, RAM, uptime) et la libération du port par le mode one-shot :

```bash
cargo test --test integration -- --test-threads=1
```

Aucun interpréteur externe n'est requis : le listener est le binaire de test lui-même,
relancé en mode helper. La CI GitHub exécute ces tests sur Linux, macOS et Windows.

## Construire les binaires de release

### En local

```bash
cargo build --release
./target/release/lemon-port --version
```

Sous Windows : `target\release\lemon-port.exe`. La profile release active LTO,
`codegen-units = 1` et `strip` pour un binaire compact.

### Releases GitHub automatisées

Le workflow [`.github/workflows/release.yml`](.github/workflows/release.yml) compile et
publie les archives pour :

| Cible | Archive |
| --- | --- |
| Linux x86_64 (musl statique) | `lemon-port-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz` |
| Linux aarch64 | `lemon-port-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `lemon-port-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `lemon-port-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| Windows x64 | `lemon-port-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

Chaque archive contient le binaire, `README.md` et `LICENSE`. Un fichier `SHA256SUMS`
est joint à la release.

Trois façons de publier :

```bash
# 1. Pousser un tag
git tag v0.1.0
git push origin v0.1.0
```

```text
# 2. GitHub → Actions → Release → Run workflow → version "0.1.0"
```

```bash
# 3. Pousser une branche de release (pratique en CI ou sans droit de créer un tag)
git push origin main:release/v0.1.0
```

Le workflow crée (ou complète) la release GitHub avec les notes générées et les binaires.
Les scripts d'installation pointent vers `releases/latest`.

## Architecture

```text
lemon-port/
├── Cargo.toml
├── Cargo.lock
├── LICENSE
├── README.md
├── install.sh              # Installeur Linux / macOS
├── install.ps1             # Installeur Windows
├── .github/workflows/
│   └── release.yml         # Build multi-OS + release GitHub
└── src/
    ├── lib.rs              # CLI (clap), dispatch TUI / one-shot / JSON
    ├── main.rs             # Binaire `lemon-port`
    ├── bin/lp.rs           # Binaire alias `lp`
    ├── scanner.rs          # Sockets TCP LISTEN + enrichissement sysinfo
    ├── project.rs          # cwd → manifeste → nom + framework
    ├── killer.rs           # SIGTERM → SIGKILL / TerminateProcess
    ├── app.rs              # État TUI, filtre, toasts, presse-papier
    └── ui.rs               # Rendu Ratatui (citron & lime), boucle d'événements
```

Choix techniques :

- `netstat2` lit les tables de sockets via les API natives (pas de `netstat` à parser).
- `sysinfo` fournit `cwd`, nom, RAM et uptime sans privilèges pour les processus de
  l'utilisateur.
- Les projets sont mis en cache par `cwd` pour que l'auto-refresh reste léger.
- La terminaison s'exécute dans un thread dédié : la TUI n'est jamais figée par l'attente
  de 1,5 s.

## Compatibilité & limites

| Plateforme | Statut | Notes |
| --- | --- | --- |
| Linux x86_64 | ✅ | Binaire musl statique disponible |
| Linux aarch64 | ✅ | Binaire glibc |
| macOS Intel / Apple Silicon | ✅ | `cwd` peut être refusé par macOS pour certains processus protégés |
| Windows x64 | ✅ | Pas de `SIGTERM` console : `TerminateProcess` direct |
| BSD / autres | ⚠️ | Compilation possible via Cargo, non testée |

- Les processus appartenant à `root` (ou à un autre utilisateur) apparaissent en
  `PROTÉGÉ` ; relancez avec `sudo` / en administrateur pour les tuer.
- Un même port partagé par plusieurs processus (`SO_REUSEPORT`) produit une ligne par PID.
- `docker-proxy` est affiché tel quel : LemonPort ne résout pas les noms de conteneurs.
- Aucun privilège particulier n'est requis pour ses propres processus.

## Licence

[MIT](LICENSE) © 2026 LLAStudio.
