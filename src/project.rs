//! Working-directory resolution and smart project detection.
//!
//! Starting from a process's `cwd`, walk up to three parent directories to
//! find a known manifest. The extracted name replaces the plain binary name
//! in the UI (`PipeCraft (Next.js)` instead of `node`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// A manifest larger than this is not a normal project manifest.
const MAX_MANIFEST_BYTES: u64 = 512 * 1024;
/// Parent directories explored in addition to the `cwd` itself.
const MAX_PARENT_DEPTH: usize = 3;
const MANIFESTS: [&str; 5] = [
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "go.mod",
    "composer.json",
];

/// Project associated with a listening process.
#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub name: String,
    /// Detected language / ecosystem.
    pub kind: String,
    /// Application framework detected in the dependencies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
    /// Directory containing the manifest.
    pub root: String,
}

impl Project {
    /// Label shown in parentheses: framework when known, otherwise language.
    pub fn label(&self) -> &str {
        match &self.framework {
            Some(framework) => framework,
            None => &self.kind,
        }
    }
}

/// Resolver with a `cwd -> project` cache to avoid re-reading manifests on
/// every refresh (every 2 seconds).
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

    /// Invalidate the cache after a manual refresh (`R`).
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
        .unwrap_or_else(|| "project".to_string())
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

/// Return the first framework whose dependency is present.
fn find_framework<'a>(dependencies: &[&str], candidates: &[(&str, &'a str)]) -> Option<&'a str> {
    candidates
        .iter()
        .find(|(dependency, _)| dependencies.contains(dependency))
        .map(|(_, label)| *label)
}

fn from_package_json(dir: &Path, text: &str) -> Option<Project> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    // A valid package.json is always a JSON object.
    let value = value.as_object()?;
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
            (Some("module"), Some(path)) => {
                let path = path.trim_matches('"');
                (!path.is_empty()).then(|| path.to_string())
            }
            _ => None,
        }
    })?;

    // A go.mod without a `module` directive is not a valid manifest.
    let name = clean_name(module.rsplit('/').next(), dir);

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
    let value = value.as_object()?;
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

#[cfg(test)]
mod tests {
    use super::{detect, detect_in_dir, Project};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Isolated temporary directory, removed even when an assertion fails.
    struct TempProject {
        root: PathBuf,
    }

    impl TempProject {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "lemon-port-project-test-{}-{id}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("create the test directory");
            Self { root }
        }

        fn path(&self) -> &Path {
            &self.root
        }

        fn write(&self, manifest: &str, contents: &str) {
            fs::write(self.root.join(manifest), contents).expect("write the manifest");
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn assert_project(
        project: &Project,
        dir: &Path,
        name: &str,
        kind: &str,
        framework: Option<&str>,
    ) {
        assert_eq!(project.name.as_str(), name);
        assert_eq!(project.kind.as_str(), kind);
        assert_eq!(project.framework.as_deref(), framework);
        assert_eq!(Path::new(&project.root), dir);
    }

    #[test]
    fn detects_package_json_manifest() {
        let project = TempProject::new();
        project.write(
            "package.json",
            r#"{
  "name": "web-app",
  "dependencies": { "next": "15.0.0" }
}"#,
        );

        let detected = detect_in_dir(project.path()).expect("valid package.json detected");
        assert_project(
            &detected,
            project.path(),
            "web-app",
            "Node.js",
            Some("Next.js"),
        );
    }

    #[test]
    fn detects_cargo_toml_manifest() {
        let project = TempProject::new();
        project.write(
            "Cargo.toml",
            r#"[package]
name = "rust-api"

[dependencies]
axum = "0.8"
"#,
        );

        let detected = detect_in_dir(project.path()).expect("valid Cargo.toml detected");
        assert_project(&detected, project.path(), "rust-api", "Rust", Some("Axum"));
    }

    #[test]
    fn detects_pyproject_manifest() {
        let project = TempProject::new();
        project.write(
            "pyproject.toml",
            r#"[project]
name = "py-service"
dependencies = ["fastapi>=0.115", "uvicorn>=0.32"]
"#,
        );

        let detected = detect_in_dir(project.path()).expect("valid pyproject.toml detected");
        assert_project(
            &detected,
            project.path(),
            "py-service",
            "Python",
            Some("FastAPI"),
        );
    }

    #[test]
    fn detects_go_mod_manifest() {
        let project = TempProject::new();
        project.write(
            "go.mod",
            r#"module example.com/acme/port-service

go 1.23

require github.com/gin-gonic/gin v1.10.0
"#,
        );

        let detected = detect_in_dir(project.path()).expect("valid go.mod detected");
        assert_project(&detected, project.path(), "port-service", "Go", Some("Gin"));
    }

    #[test]
    fn detects_composer_manifest() {
        let project = TempProject::new();
        project.write(
            "composer.json",
            r#"{
  "name": "acme/storefront",
  "require": { "laravel/framework": "^11.0" }
}"#,
        );

        let detected = detect_in_dir(project.path()).expect("valid composer.json detected");
        assert_project(
            &detected,
            project.path(),
            "storefront",
            "PHP",
            Some("Laravel"),
        );
    }

    #[test]
    fn ignores_malformed_json_and_toml_manifests() {
        let cases = [
            ("package.json", "{ \"name\": \"broken\","),
            ("Cargo.toml", "[package\nname = \"broken\""),
            ("pyproject.toml", "[project\nname = \"broken\""),
            ("composer.json", "{ \"name\": \"broken\","),
        ];

        for (manifest, contents) in cases {
            let project = TempProject::new();
            project.write(manifest, contents);
            assert!(
                detect_in_dir(project.path()).is_none(),
                "malformed {manifest} must not be used"
            );
        }
    }

    #[test]
    fn ignores_json_manifests_with_non_object_root() {
        for manifest in ["package.json", "composer.json"] {
            let project = TempProject::new();
            project.write(manifest, "[]");
            assert!(
                detect_in_dir(project.path()).is_none(),
                "{manifest} must be a JSON object"
            );
        }
    }

    #[test]
    fn ignores_go_mod_without_module_directive() {
        let project = TempProject::new();
        project.write(
            "go.mod",
            "go 1.23\n\nrequire example.com/dependency v1.0.0\n",
        );
        assert!(
            detect_in_dir(project.path()).is_none(),
            "a go.mod without a module directive is invalid"
        );
    }

    #[test]
    fn malformed_manifest_falls_back_to_next_candidate() {
        let project = TempProject::new();
        project.write("package.json", "{ broken");
        project.write("Cargo.toml", "[package]\nname = \"fallback-app\"\n");

        let detected = detect_in_dir(project.path()).expect("Cargo.toml must be used instead");
        assert_project(&detected, project.path(), "fallback-app", "Rust", None);
    }

    #[test]
    fn detects_manifest_in_parent_directory() {
        let project = TempProject::new();
        project.write("package.json", r#"{"name": "nested-app"}"#);
        let cwd = project
            .path()
            .join("src")
            .join("features")
            .join("components");
        fs::create_dir_all(&cwd).expect("create nested directories");

        let detected = detect(&cwd).expect("parent manifest detected");
        assert_eq!(detected.name.as_str(), "nested-app");
        assert_eq!(Path::new(&detected.root), project.path());
    }
}
