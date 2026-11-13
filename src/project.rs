//! Résolution du répertoire de travail et détection intelligente du projet.
//!
//! À partir du `cwd` d'un processus, on remonte jusqu'à trois dossiers parents
//! pour trouver un manifeste connu. Le nom extrait remplace le simple nom du
//! binaire dans l'interface (`PipeCraft (Next.js)` au lieu de `node`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Un manifeste plus gros que cela n'est pas un manifeste de projet normal.
const MAX_MANIFEST_BYTES: u64 = 512 * 1024;
/// Dossiers parents explorés en plus du `cwd` lui-même.
const MAX_PARENT_DEPTH: usize = 3;
const MANIFESTS: [&str; 5] = [
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "go.mod",
    "composer.json",
];

/// Projet associé à un processus en écoute.
#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub name: String,
    /// Langage / écosystème détecté.
    pub kind: String,
    /// Framework applicatif détecté dans les dépendances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
    /// Dossier contenant le manifeste.
    pub root: String,
}

impl Project {
    /// Libellé affiché entre parenthèses : framework si connu, sinon langage.
    pub fn label(&self) -> &str {
        match &self.framework {
            Some(framework) => framework,
            None => &self.kind,
        }
    }
}

/// Résolveur avec cache `cwd -> projet` pour éviter de relire les manifestes
/// à chaque rafraîchissement (toutes les 2 secondes).
#[derive(Default)]
pub struct ProjectResolver {
    cache: HashMap<PathBuf, Option<Project>>,
}

impl ProjectResolver {
    pub fn resolve(&mut self, cwd: &Path) -> Option<Project> {
        if let Some(cached) = self.cache.get(cwd) {
            return cached.clone();
        }
        let found = detect(cwd);
        self.cache.insert(cwd.to_path_buf(), found.clone());
        found
    }

    /// Invalide le cache après un rafraîchissement manuel (`R`).
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

fn detect(cwd: &Path) -> Option<Project> {
    let mut current = Some(cwd);
    for _ in 0..=MAX_PARENT_DEPTH {
        let dir = current?;
        if let Some(project) = detect_in_dir(dir) {
            return Some(project);
        }
        current = dir.parent();
    }
    None
}

fn detect_in_dir(dir: &Path) -> Option<Project> {
    for manifest in MANIFESTS {
        let path = dir.join(manifest);
        if !path.is_file() {
            continue;
        }
        let Some(text) = read_manifest(&path) else {
            continue;
        };
        let parsed = match manifest {
            "package.json" => from_package_json(dir, &text),
            "Cargo.toml" => from_cargo_toml(dir, &text),
            "pyproject.toml" => from_pyproject(dir, &text),
            "go.mod" => from_go_mod(dir, &text),
            "composer.json" => from_composer(dir, &text),
            _ => None,
        };
        if parsed.is_some() {
            return parsed;
        }
    }
    None
}

fn read_manifest(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn fallback_name(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "projet".to_string())
}

fn clean_name(value: Option<&str>, dir: &Path) -> String {
    value
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fallback_name(dir))
}

fn build_project(dir: &Path, name: String, kind: &str, framework: Option<&str>) -> Project {
    Project {
        name,
        kind: kind.to_string(),
        framework: framework.map(str::to_string),
        root: dir.to_string_lossy().into_owned(),
    }
}

/// Retourne le premier framework dont la dépendance est présente.
fn find_framework<'a>(dependencies: &[&str], candidates: &[(&str, &'a str)]) -> Option<&'a str> {
    candidates
        .iter()
        .find(|(dependency, _)| dependencies.contains(dependency))
        .map(|(_, label)| *label)
}

fn from_package_json(dir: &Path, text: &str) -> Option<Project> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let name = clean_name(value.get("name").and_then(serde_json::Value::as_str), dir);

    let mut dependencies: Vec<&str> = Vec::new();
    for section in ["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(map) = value.get(section).and_then(serde_json::Value::as_object) {
            dependencies.extend(map.keys().map(String::as_str));
        }
    }

    const CANDIDATES: [(&str, &str); 19] = [
        ("next", "Next.js"),
        ("nuxt", "Nuxt"),
        ("@sveltejs/kit", "SvelteKit"),
        ("astro", "Astro"),
        ("@remix-run/dev", "Remix"),
        ("@angular/core", "Angular"),
        ("react-scripts", "React"),
        ("@nestjs/core", "NestJS"),
        ("fastify", "Fastify"),
        ("hono", "Hono"),
        ("express", "Express"),
        ("expo", "Expo"),
        ("react-native", "React Native"),
        ("electron", "Electron"),
        ("vue", "Vue"),
        ("svelte", "Svelte"),
        ("solid-js", "Solid"),
        ("react", "React"),
        ("vite", "Vite"),
    ];

    Some(build_project(
        dir,
        name,
        "Node.js",
        find_framework(&dependencies, &CANDIDATES),
    ))
}

fn from_cargo_toml(dir: &Path, text: &str) -> Option<Project> {
    let value: toml::Value = toml::from_str(text).ok()?;
    let name = clean_name(
        value
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str),
        dir,
    );

    let mut dependencies: Vec<&str> = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = value.get(section).and_then(toml::Value::as_table) {
            dependencies.extend(table.keys().map(String::as_str));
        }
    }

    const CANDIDATES: [(&str, &str); 7] = [
        ("axum", "Axum"),
        ("actix-web", "Actix Web"),
        ("rocket", "Rocket"),
        ("warp", "Warp"),
        ("poem", "Poem"),
        ("salvo", "Salvo"),
        ("tide", "Tide"),
    ];

    Some(build_project(
        dir,
        name,
        "Rust",
        find_framework(&dependencies, &CANDIDATES),
    ))
}

fn from_pyproject(dir: &Path, text: &str) -> Option<Project> {
    let value: toml::Value = toml::from_str(text).ok()?;
    let raw_name = value
        .get("project")
        .and_then(|project| project.get("name"))
        .and_then(toml::Value::as_str)
        .or_else(|| {
            value
                .get("tool")
                .and_then(|tool| tool.get("poetry"))
                .and_then(|poetry| poetry.get("name"))
                .and_then(toml::Value::as_str)
        });
    let name = clean_name(raw_name, dir);

    let lower = text.to_ascii_lowercase();
    const CANDIDATES: [(&str, &str); 7] = [
        ("fastapi", "FastAPI"),
        ("django", "Django"),
        ("flask", "Flask"),
        ("streamlit", "Streamlit"),
        ("gradio", "Gradio"),
        ("jupyter", "Jupyter"),
        ("uvicorn", "Uvicorn"),
    ];
    let framework = CANDIDATES
        .iter()
        .find(|(needle, _)| lower.contains(needle))
        .map(|(_, label)| *label);

    Some(build_project(dir, name, "Python", framework))
}

fn from_go_mod(dir: &Path, text: &str) -> Option<Project> {
    let module = text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some("module"), Some(path)) => Some(path.trim_matches('"').to_string()),
            _ => None,
        }
    });

    let name = module
        .as_deref()
        .and_then(|path| path.rsplit('/').next())
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fallback_name(dir));

    const CANDIDATES: [(&str, &str); 6] = [
        ("gin-gonic/gin", "Gin"),
        ("gofiber/fiber", "Fiber"),
        ("labstack/echo", "Echo"),
        ("go-chi/chi", "Chi"),
        ("gorilla/mux", "Gorilla"),
        ("beego", "Beego"),
    ];
    let framework = CANDIDATES
        .iter()
        .find(|(needle, _)| text.contains(needle))
        .map(|(_, label)| *label);

    Some(build_project(dir, name, "Go", framework))
}

fn from_composer(dir: &Path, text: &str) -> Option<Project> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let raw_name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(|name| name.rsplit('/').next().unwrap_or(name));
    let name = clean_name(raw_name, dir);

    let mut dependencies: Vec<&str> = Vec::new();
    if let Some(require) = value.get("require").and_then(serde_json::Value::as_object) {
        dependencies.extend(require.keys().map(String::as_str));
    }
    const CANDIDATES: [(&str, &str); 3] = [
        ("laravel/framework", "Laravel"),
        ("symfony/framework-bundle", "Symfony"),
        ("slim/slim", "Slim"),
    ];

    Some(build_project(
        dir,
        name,
        "PHP",
        find_framework(&dependencies, &CANDIDATES),
    ))
}
