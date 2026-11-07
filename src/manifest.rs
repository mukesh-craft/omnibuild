use crate::{cache::CacheRecord, exec::CommandOutput, verify::ArtifactInfo};
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize)]
pub struct CommandRecord {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub timed_out: bool,
    pub duration_ms: u128,
    pub log: String,
}

impl From<&CommandOutput> for CommandRecord {
    fn from(output: &CommandOutput) -> Self {
        Self {
            program: output.program.clone(),
            args: output.args.clone(),
            cwd: output.cwd.clone(),
            exit_code: output.exit_code,
            success: output.success,
            timed_out: output.timed_out,
            duration_ms: output.duration_ms,
            log: output.log.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetRecord {
    pub id: String,
    pub os: String,
    pub arch: String,
    pub abi: String,
    pub llvm_triple: String,
    pub binary_format: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceRecord {
    pub path: String,
    pub git_commit: Option<String>,
    pub git_dirty: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerificationRecord {
    pub level: String,
    pub checks: Vec<crate::verify::VerificationCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetManifest {
    pub schema_version: u32,
    pub project: String,
    pub adapter: String,
    pub target: TargetRecord,
    pub build_mode: String,
    pub source: SourceRecord,
    pub status: String,
    pub verification: VerificationRecord,
    pub toolchain: BTreeMap<String, String>,
    pub artifact: Option<ArtifactInfo>,
    pub commands: Vec<CommandRecord>,
    pub run: Option<CommandRecord>,
    pub cache: Option<CacheRecord>,
    pub error: Option<String>,
    pub generated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseTarget {
    pub target: String,
    pub status: String,
    pub level: String,
    pub artifact: Option<String>,
    pub sha256: Option<String>,
    pub manifest: String,
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub project: String,
    pub source: SourceRecord,
    pub generated_at_unix: u64,
    pub targets: Vec<ReleaseTarget>,
}

pub fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create manifest directory {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{text}\n"))
        .with_context(|| format!("cannot write manifest {}", path.display()))
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn source_record(path: &Path) -> SourceRecord {
    let commit = git(path, &["rev-parse", "HEAD"]);
    let dirty = git(path, &["status", "--porcelain"]).map(|status| !status.trim().is_empty());
    SourceRecord {
        path: path.display().to_string(),
        git_commit: commit,
        git_dirty: dirty,
    }
}

fn git(path: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
