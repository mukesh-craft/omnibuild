use crate::{config::Config, target::TargetSpec, toolchain};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::{DirEntry, WalkDir};

const CACHE_SCHEMA_VERSION: u32 = 2;
const ENV_KEYS: &[&str] = &[
    "PATH",
    "TMPDIR",
    "CC",
    "CXX",
    "CFLAGS",
    "CXXFLAGS",
    "CPPFLAGS",
    "LDFLAGS",
    "RUSTFLAGS",
    "CARGO_BUILD_TARGET",
    "CMAKE_GENERATOR",
    "SOURCE_DATE_EPOCH",
    "FLINTC",
];

const LOCKFILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "go.sum",
    "Package.resolved",
];

const SKIP_DIRS: &[&str] = &[
    "target",
    "dist",
    ".git",
    ".cache",
    ".omnibuild-cache",
    "node_modules",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheRecord {
    pub key: String,
    pub hit: bool,
    pub directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub schema_version: u32,
    pub key: String,
    pub project: String,
    pub target: String,
    pub abi: String,
    pub adapter: String,
    pub build_mode: String,
    pub artifact_file: String,
    pub artifact_sha256: String,
    pub level: String,
    pub toolchain: BTreeMap<String, String>,
    pub created_at_unix: u64,
}

pub struct CachedArtifact {
    pub artifact: PathBuf,
    pub toolchain: BTreeMap<String, String>,
}

pub struct Cache {
    root: PathBuf,
}

pub struct KeyInput<'a> {
    pub source: &'a Path,
    pub project_root: &'a Path,
    pub project_name: &'a str,
    pub build_system: &'a str,
    pub target: &'static TargetSpec,
    pub config: &'a Config,
}

impl Cache {
    pub fn new(source: &Path, config: &Config) -> Self {
        let configured = PathBuf::from(&config.cache.directory);
        let root = if configured.is_absolute() {
            configured
        } else {
            source.join(configured)
        };
        Self { root }
    }

    pub fn enabled(&self, config: &Config) -> bool {
        config.cache.enabled
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn key(&self, input: &KeyInput<'_>) -> Result<String> {
        let artifacts = resolve(input.source, &input.config.artifacts.directory);
        let skip = [self.root.clone(), artifacts];
        compute_key(input, &skip)
    }

    pub fn restore(&self, key: &str, work_dir: &Path) -> Result<Option<CachedArtifact>> {
        let entry_dir = self.root.join(key);
        let entry_path = entry_dir.join("entry.json");
        if !entry_path.is_file() {
            return Ok(None);
        }
        let text = fs::read_to_string(&entry_path)
            .with_context(|| format!("cannot read cache entry {}", entry_path.display()))?;
        let entry: CacheEntry = match serde_json::from_str(&text) {
            Ok(entry) => entry,
            Err(_) => return Ok(None),
        };
        if entry.schema_version != CACHE_SCHEMA_VERSION || entry.key != key {
            return Ok(None);
        }
        let artifact = entry_dir.join("artifact").join(&entry.artifact_file);
        if !artifact.is_file() {
            return Ok(None);
        }
        let actual = crate::verify::sha256_file(&artifact)?;
        if actual != entry.artifact_sha256 {
            return Ok(None);
        }

        fs::create_dir_all(work_dir)
            .with_context(|| format!("cannot create {}", work_dir.display()))?;
        let destination = work_dir.join(&entry.artifact_file);
        fs::copy(&artifact, &destination).with_context(|| {
            format!(
                "cannot restore cached artifact {} to {}",
                artifact.display(),
                destination.display()
            )
        })?;

        Ok(Some(CachedArtifact {
            artifact: destination,
            toolchain: entry.toolchain,
        }))
    }

    pub fn store(&self, key: &str, artifact: &Path, mut entry: CacheEntry) -> Result<()> {
        let entry_dir = self.root.join(key);
        if entry_dir.join("entry.json").is_file() {
            return Ok(());
        }
        fs::create_dir_all(&self.root)
            .with_context(|| format!("cannot create cache directory {}", self.root.display()))?;
        let temp = self
            .root
            .join(format!(".tmp-{}-{}", std::process::id(), now_unix()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(temp.join("artifact"))
            .with_context(|| format!("cannot create {}", temp.display()))?;

        let file_name = artifact
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("cache artifact has no file name: {}", artifact.display()))?
            .to_string();
        let stored = temp.join("artifact").join(&file_name);
        fs::copy(artifact, &stored).with_context(|| {
            format!(
                "cannot copy {} into cache {}",
                artifact.display(),
                stored.display()
            )
        })?;

        entry.schema_version = CACHE_SCHEMA_VERSION;
        entry.key = key.to_string();
        entry.artifact_file = file_name;
        entry.artifact_sha256 = crate::verify::sha256_file(&stored)?;
        entry.created_at_unix = now_unix();
        let text = serde_json::to_string_pretty(&entry)?;
        fs::write(temp.join("entry.json"), format!("{text}\n"))
            .with_context(|| format!("cannot write cache entry under {}", temp.display()))?;

        match fs::rename(&temp, &entry_dir) {
            Ok(()) => Ok(()),
            Err(_) if entry_dir.join("entry.json").is_file() => {
                let _ = fs::remove_dir_all(&temp);
                Ok(())
            }
            Err(error) => {
                let _ = fs::remove_dir_all(&temp);
                Err(error)
                    .with_context(|| format!("cannot install cache entry {}", entry_dir.display()))
            }
        }
    }
}

fn compute_key(input: &KeyInput<'_>, skip: &[PathBuf]) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"omnibuild-cache-v1\0");
    hash_text(&mut hasher, env!("CARGO_PKG_VERSION"));
    hash_text(&mut hasher, input.build_system);
    hash_text(&mut hasher, input.project_name);
    hash_text(&mut hasher, input.target.id);
    hash_text(&mut hasher, input.target.os);
    hash_text(&mut hasher, input.target.arch);
    hash_text(&mut hasher, input.target.abi);
    hash_text(&mut hasher, input.target.llvm_triple);
    hash_text(&mut hasher, input.target.flint_triple);
    hash_text(&mut hasher, input.target.zig_triple);
    hash_text(&mut hasher, &input.config.build.mode);

    let config_bytes = serde_json::to_vec(input.config)?;
    hasher.update((config_bytes.len() as u64).to_le_bytes());
    hasher.update(&config_bytes);

    for path in skip {
        hash_text(&mut hasher, &path.display().to_string());
    }
    hash_tree(input.project_root, skip, &mut hasher)?;

    for lockfile in LOCKFILES {
        let path = input.project_root.join(lockfile);
        if path.is_file() {
            hash_text(&mut hasher, lockfile);
            hasher.update(crate::verify::sha256_file(&path)?.as_bytes());
        }
    }

    if input.build_system == "Flint"
        && let Some(flintc) = toolchain::check_tool("flintc", Some("FLINTC"), None).path
        && let Some(root) = Path::new(&flintc).parent()
    {
        for name in [
            "runtime/runtime.c",
            "runtime/flint_aegis.c",
            "runtime/flint_aegis.h",
        ] {
            let path = root.join(name);
            if path.is_file() {
                hash_text(&mut hasher, name);
                hasher.update(crate::verify::sha256_file(&path)?.as_bytes());
            }
        }
    }

    for key in ENV_KEYS {
        hash_text(&mut hasher, key);
        match std::env::var_os(key) {
            Some(value) => hash_text(&mut hasher, &value.to_string_lossy()),
            None => hash_text(&mut hasher, "<unset>"),
        }
    }

    let tools = toolchain_identity(input.build_system);
    for (name, version) in tools {
        hash_text(&mut hasher, &name);
        hash_text(&mut hasher, &version);
    }

    let digest = hasher.finalize();
    let mut output = String::from("v2-");
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    Ok(output)
}

fn hash_tree(root: &Path, skip: &[PathBuf], hasher: &mut Sha256) -> Result<()> {
    if !root.is_dir() {
        bail!("cannot hash project root {}", root.display());
    }

    let mut files = Vec::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !skipped(entry, skip));

    for entry in walker {
        let entry = entry.with_context(|| format!("cannot walk {}", root.display()))?;
        if entry.file_type().is_file() || entry.file_type().is_symlink() {
            files.push(entry.path().to_path_buf());
        }
    }
    files.sort_by_key(|path| relative(root, path));

    for path in files {
        let rel = relative(root, &path);
        hash_text(hasher, &rel);
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("cannot stat {}", path.display()))?;
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path)
                .with_context(|| format!("cannot read symlink {}", path.display()))?;
            hash_text(hasher, &target.to_string_lossy());
            continue;
        }
        let data = fs::read(&path).with_context(|| format!("cannot read {}", path.display()))?;
        hasher.update((data.len() as u64).to_le_bytes());
        hasher.update(&data);
    }
    Ok(())
}

fn skipped(entry: &DirEntry, skip: &[PathBuf]) -> bool {
    if skip.iter().any(|path| entry.path().starts_with(path)) {
        return true;
    }
    if entry.depth() == 0 {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    entry.file_type().is_dir() && SKIP_DIRS.contains(&name.as_ref())
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string())
}

fn resolve(source: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        source.join(path)
    }
}

fn hash_text(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn toolchain_identity(build_system: &str) -> BTreeMap<String, String> {
    let tools: &[(&str, Option<&str>, &[&str])] = match build_system {
        "Cargo" => &[
            ("rustc", None, &["--version"]),
            ("cargo", None, &["--version"]),
        ],
        "Flint" => &[
            ("flintc", Some("FLINTC"), &["--version"]),
            ("zig", None, &["version"]),
            ("clang", None, &["--version"]),
        ],
        "CMake" => &[
            ("cmake", None, &["--version"]),
            ("clang", None, &["--version"]),
            ("zig", None, &["version"]),
            ("ninja", None, &["--version"]),
        ],
        _ => &[],
    };

    let mut identity = BTreeMap::new();
    for (name, env_var, args) in tools {
        let check = toolchain::check_tool(name, *env_var, None);
        let value = match check.path {
            Some(path) => {
                let version =
                    toolchain::version_of(&path, args).unwrap_or_else(|| "unknown".to_string());
                format!("{version} @ {path}")
            }
            None => "missing".to_string(),
        };
        identity.insert((*name).to_string(), value);
    }
    identity
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::target;
    use std::fs;

    fn tempdir(tag: &str) -> PathBuf {
        let base = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp"));
        fs::create_dir_all(&base).unwrap();
        let dir = base.join(format!("cache-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn input<'a>(root: &'a Path, config: &'a Config, target: &'static TargetSpec) -> KeyInput<'a> {
        KeyInput {
            source: root,
            project_root: root,
            project_name: "demo",
            build_system: "Cargo",
            target,
            config,
        }
    }

    #[test]
    fn key_is_stable_and_changes_with_source() {
        let root = tempdir("stable");
        fs::write(root.join("Cargo.toml"), "name = \"demo\"\n").unwrap();
        let config = Config::default();
        let target = target::by_id("android-arm64").unwrap();
        let cache = Cache::new(&root, &config);
        let first = cache.key(&input(&root, &config, target)).unwrap();
        let second = cache.key(&input(&root, &config, target)).unwrap();
        assert_eq!(first, second);

        fs::write(root.join("Cargo.toml"), "name = \"changed\"\n").unwrap();
        let third = cache.key(&input(&root, &config, target)).unwrap();
        assert_ne!(first, third);
    }

    #[test]
    fn store_and_restore_round_trip() {
        let root = tempdir("roundtrip");
        fs::write(root.join("Cargo.toml"), "name = \"demo\"\n").unwrap();
        let config = Config::default();
        let target = target::by_id("android-arm64").unwrap();
        let cache = Cache::new(&root, &config);
        let key = cache.key(&input(&root, &config, target)).unwrap();

        let artifact = root.join("demo");
        fs::write(&artifact, b"artifact-bytes").unwrap();
        let entry = CacheEntry {
            schema_version: CACHE_SCHEMA_VERSION,
            key: key.clone(),
            project: "demo".to_string(),
            target: target.id.to_string(),
            abi: target.abi.to_string(),
            adapter: "cargo".to_string(),
            build_mode: "release".to_string(),
            artifact_file: String::new(),
            artifact_sha256: String::new(),
            level: "inspected".to_string(),
            toolchain: BTreeMap::new(),
            created_at_unix: 0,
        };
        cache.store(&key, &artifact, entry).unwrap();

        let work = root.join("work");
        let restored = cache.restore(&key, &work).unwrap().expect("cache hit");
        assert_eq!(
            fs::read(&restored.artifact).unwrap(),
            b"artifact-bytes".to_vec()
        );
    }
}
