use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use walkdir::{DirEntry, WalkDir};

const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    ".cache",
    ".cargo",
    ".gradle",
    ".idea",
    ".vscode",
];

#[derive(Debug, Clone, Serialize)]
pub struct Detection {
    pub name: String,
    pub language: String,
    pub build_system: String,
    pub root: String,
    pub manifest: Option<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DetectionReport {
    pub source: String,
    pub project_count: usize,
    pub projects: Vec<Detection>,
}

pub fn detect(source: &Path, max_depth: usize) -> Result<DetectionReport> {
    let source = source
        .canonicalize()
        .with_context(|| format!("cannot access source directory {}", source.display()))?;

    let mut found: Vec<Detection> = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut flint_dirs: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();

    let walker = WalkDir::new(&source)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !is_skipped(entry));

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let dir = path.parent().unwrap_or(&source);

        match file_name {
            "Cargo.toml" => {
                let name = cargo_name(path).unwrap_or_else(|| dir_name(dir));
                add(
                    &mut found,
                    &mut seen,
                    dir,
                    &source,
                    name,
                    "Rust",
                    "Cargo",
                    Some(path),
                    vec![rel(&source, path)],
                );
            }
            "CMakeLists.txt" => {
                let name = cmake_name(path).unwrap_or_else(|| dir_name(dir));
                add(
                    &mut found,
                    &mut seen,
                    dir,
                    &source,
                    name,
                    "C/C++",
                    "CMake",
                    Some(path),
                    vec![rel(&source, path)],
                );
            }
            "go.mod" => {
                let name = go_name(path).unwrap_or_else(|| dir_name(dir));
                add(
                    &mut found,
                    &mut seen,
                    dir,
                    &source,
                    name,
                    "Go",
                    "Go",
                    Some(path),
                    vec![rel(&source, path)],
                );
            }
            "package.json" => {
                let name = json_name(path).unwrap_or_else(|| dir_name(dir));
                add(
                    &mut found,
                    &mut seen,
                    dir,
                    &source,
                    name,
                    "JavaScript",
                    "npm",
                    Some(path),
                    vec![rel(&source, path)],
                );
            }
            "pom.xml" => {
                let name = xml_tag_name(path, "artifactId").unwrap_or_else(|| dir_name(dir));
                add(
                    &mut found,
                    &mut seen,
                    dir,
                    &source,
                    name,
                    "Java",
                    "Maven",
                    Some(path),
                    vec![rel(&source, path)],
                );
            }
            "build.gradle" | "build.gradle.kts" => {
                add(
                    &mut found,
                    &mut seen,
                    dir,
                    &source,
                    dir_name(dir),
                    "Java",
                    "Gradle",
                    Some(path),
                    vec![rel(&source, path)],
                );
            }
            "Package.swift" => {
                add(
                    &mut found,
                    &mut seen,
                    dir,
                    &source,
                    dir_name(dir),
                    "Swift",
                    "SwiftPM",
                    Some(path),
                    vec![rel(&source, path)],
                );
            }
            name if name.ends_with(".csproj") => {
                let project_name = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| dir_name(dir));
                add(
                    &mut found,
                    &mut seen,
                    dir,
                    &source,
                    project_name,
                    "C#",
                    "dotnet",
                    Some(path),
                    vec![rel(&source, path)],
                );
            }
            name if name.ends_with(".fl") => {
                flint_dirs
                    .entry(dir.to_path_buf())
                    .or_default()
                    .push(path.to_path_buf());
            }
            _ => {}
        }
    }

    for (dir, files) in flint_dirs {
        let evidence = files.iter().map(|file| rel(&source, file)).collect();
        let manifest = files.first().map(PathBuf::as_path);
        add(
            &mut found,
            &mut seen,
            &dir,
            &source,
            dir_name(&dir),
            "Flint",
            "Flint",
            manifest,
            evidence,
        );
    }

    found.sort_by(|left, right| {
        root_depth(&left.root)
            .cmp(&root_depth(&right.root))
            .then_with(|| left.root.cmp(&right.root))
            .then_with(|| left.build_system.cmp(&right.build_system))
    });

    Ok(DetectionReport {
        source: source.display().to_string(),
        project_count: found.len(),
        projects: found,
    })
}

fn is_skipped(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    entry.file_type().is_dir() && SKIP_DIRS.contains(&name.as_ref())
}

#[allow(clippy::too_many_arguments)]
fn add(
    found: &mut Vec<Detection>,
    seen: &mut BTreeSet<(String, String)>,
    dir: &Path,
    source: &Path,
    name: String,
    language: &str,
    build_system: &str,
    manifest: Option<&Path>,
    evidence: Vec<String>,
) {
    let root = rel_dir(source, dir);
    if !seen.insert((root.clone(), build_system.to_string())) {
        return;
    }
    found.push(Detection {
        name,
        language: language.to_string(),
        build_system: build_system.to_string(),
        root,
        manifest: manifest.map(|path| rel(source, path)),
        evidence,
    });
}

fn rel(source: &Path, path: &Path) -> String {
    path.strip_prefix(source)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string())
}

fn rel_dir(source: &Path, dir: &Path) -> String {
    let relative = rel(source, dir);
    if relative.is_empty() {
        ".".to_string()
    } else {
        relative
    }
}

fn root_depth(root: &str) -> usize {
    if root == "." {
        0
    } else {
        root.split('/').count()
    }
}

fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| dir.display().to_string())
}

fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn cargo_name(path: &Path) -> Option<String> {
    let text = read(path)?;
    let mut in_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "name"
        {
            let name = unquote(value);
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn cmake_name(path: &Path) -> Option<String> {
    let text = read(path)?;
    for line in text.lines() {
        let line = line.trim();
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("project(") {
            let original = &line[line.len() - rest.len()..];
            let name = unquote(
                original
                    .split(|c: char| c == ')' || c == ',' || c.is_whitespace())
                    .next()
                    .unwrap_or(""),
            );
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn go_name(path: &Path) -> Option<String> {
    let text = read(path)?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(module) = line.strip_prefix("module ") {
            return module
                .trim()
                .split('/')
                .next_back()
                .map(str::to_string)
                .filter(|name| !name.is_empty());
        }
    }
    None
}

fn xml_tag_name(path: &Path, tag: &str) -> Option<String> {
    let text = read(path)?;
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let rest = &text[start..];
    let end = rest.find(&close)?;
    let value = rest[..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn json_name(path: &Path) -> Option<String> {
    let text = read(path)?;
    let key = "\"name\"";
    let start = text.find(key)? + key.len();
    let rest = &text[start..];
    let colon = rest.find(':')?;
    let rest = &rest[colon + 1..];
    let open = rest.find('"')?;
    let rest = &rest[open + 1..];
    let close = rest.find('"')?;
    let value = rest[..close].trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let base = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp"));
        fs::create_dir_all(&base).expect("create test temp base");
        let dir = base.join(format!("{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create test temp dir");
        dir
    }

    #[test]
    fn detects_rust_project() {
        let root = tempdir("detect-rust");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

        let report = detect(&root, 4).unwrap();
        assert_eq!(report.project_count, 1);
        assert_eq!(report.projects[0].name, "demo");
        assert_eq!(report.projects[0].language, "Rust");
        assert_eq!(report.projects[0].build_system, "Cargo");
    }

    #[test]
    fn detects_flint_project() {
        let root = tempdir("detect-flint");
        fs::write(root.join("main.fl"), "fn main() -> i64 { 0 }\n").unwrap();

        let report = detect(&root, 4).unwrap();
        assert_eq!(report.project_count, 1);
        assert_eq!(report.projects[0].language, "Flint");
        assert_eq!(report.projects[0].build_system, "Flint");
    }

    #[test]
    fn detects_monorepo_roots_in_depth_order() {
        let root = tempdir("detect-mono");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("CMakeLists.txt"), "project(root)\n").unwrap();
        fs::write(
            root.join("sub/Cargo.toml"),
            "[package]\nname = \"nested\"\n",
        )
        .unwrap();

        let report = detect(&root, 4).unwrap();
        assert_eq!(report.project_count, 2);
        assert_eq!(report.projects[0].root, ".");
        assert_eq!(report.projects[1].root, "sub");
    }

    #[test]
    fn ignores_build_directories() {
        let root = tempdir("detect-skip");
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(
            root.join("target/debug/Cargo.toml"),
            "[package]\nname = \"ghost\"\n",
        )
        .unwrap();

        let report = detect(&root, 4).unwrap();
        assert_eq!(report.project_count, 0);
    }
}
