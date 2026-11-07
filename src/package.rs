use crate::{
    build::{self, BuildRun},
    config::Config,
    detect::DetectionReport,
    exec::{self, CommandSpec},
    manifest::{self, SourceRecord},
    target::TargetSpec,
    toolchain, verify,
};
use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Serialize)]
pub struct PackageRun {
    pub release: BuildRun,
    pub packages: PackageManifest,
    pub package_manifest_path: String,
}

#[derive(Debug, Serialize)]
pub struct PackageManifest {
    pub schema_version: u32,
    pub project: String,
    pub source: SourceRecord,
    pub generated_at_unix: u64,
    pub release_manifest: String,
    pub release_manifest_sha256: Option<String>,
    pub archives: Vec<ArchiveRecord>,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ArchiveRecord {
    pub target: String,
    pub status: String,
    pub level: String,
    pub format: String,
    pub archive: String,
    pub sha256: String,
    pub size: u64,
    pub artifact_sha256: Option<String>,
    pub target_manifest: String,
    pub archive_log: String,
}

pub fn run(
    source: &Path,
    config: &Config,
    report: &DetectionReport,
    targets: &[&'static TargetSpec],
) -> Result<PackageRun> {
    let build_run = build::run(source, config, report, targets)?;
    let project_dist = Path::new(&build_run.release_manifest_path)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("cannot locate the project artifact directory"))?;
    let packages_dir = project_dist.join("packages");
    fs::create_dir_all(&packages_dir)
        .with_context(|| format!("cannot create {}", packages_dir.display()))?;

    let tar = toolchain::check_tool("tar", None, Some("pkg install tar"));
    let Some(tar_path) = tar.path else {
        bail!("tar not found; install tar to create release archives");
    };
    let zip = toolchain::check_tool("zip", None, Some("pkg install zip"));
    let timeout = Duration::from_secs(config.build.timeout_secs.max(1));

    let mut archives = Vec::new();
    let mut notes = Vec::new();
    if zip.path.is_none() {
        notes.push("zip not found; only .tar.gz archives were produced".to_string());
    }
    for result in &build_run.build.results {
        if result.status != "success" {
            continue;
        }
        let (Some(artifact), Some(manifest_path)) =
            (result.artifact.as_ref(), result.manifest_path.as_ref())
        else {
            continue;
        };
        let manifest_path = PathBuf::from(manifest_path);
        let stage = packages_dir.join(".stage").join(&result.target);
        let _ = fs::remove_dir_all(&stage);
        let stage_artifact = stage.join("artifact");
        fs::create_dir_all(&stage_artifact)
            .with_context(|| format!("cannot create {}", stage_artifact.display()))?;

        let artifact_path = PathBuf::from(&artifact.path);
        let artifact_name = artifact_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("artifact has no file name: {}", artifact.path))?;
        let staged_artifact = stage_artifact.join(artifact_name);
        fs::copy(&artifact_path, &staged_artifact).with_context(|| {
            format!(
                "cannot stage artifact {} into {}",
                artifact_path.display(),
                staged_artifact.display()
            )
        })?;

        let staged_manifest = stage.join("manifest.json");
        fs::copy(&manifest_path, &staged_manifest).with_context(|| {
            format!(
                "cannot stage manifest {} into {}",
                manifest_path.display(),
                staged_manifest.display()
            )
        })?;

        let staged_release = stage.join("release.json");
        fs::copy(&build_run.release_manifest_path, &staged_release).with_context(|| {
            format!(
                "cannot stage release manifest {} into {}",
                build_run.release_manifest_path,
                staged_release.display()
            )
        })?;

        let artifact_sha = verify::sha256_file(&staged_artifact)?;
        let manifest_sha = verify::sha256_file(&staged_manifest)?;
        let release_sha = verify::sha256_file(&staged_release)?;
        let sums = format!(
            "{artifact_sha}  artifact/{artifact_name}\n\
             {manifest_sha}  manifest.json\n\
             {release_sha}  release.json\n"
        );
        fs::write(stage.join("SHA256SUMS"), sums)
            .with_context(|| format!("cannot write checksums under {}", stage.display()))?;

        let archive_path = packages_dir.join(format!(
            "{}-{}-{}.tar.gz",
            sanitize(&build_run.build.project),
            sanitize(&result.target),
            env!("CARGO_PKG_VERSION")
        ));
        let _ = fs::remove_file(&archive_path);
        let tar_log = packages_dir.join(format!("{}.tar.log", sanitize(&result.target)));
        let mut command = CommandSpec::new(PathBuf::from(&tar_path), &stage);
        command.arg("-czf");
        command.arg(archive_path.display().to_string());
        command.arg(".");
        command.timeout(timeout);
        let output = exec::run(&command, &tar_log)?;
        if !output.success {
            bail!(
                "tar failed for target `{}`{}; see {}",
                result.target,
                if output.timed_out { " (timed out)" } else { "" },
                tar_log.display()
            );
        }

        let size = fs::metadata(&archive_path)
            .with_context(|| format!("cannot stat {}", archive_path.display()))?
            .len();
        archives.push(ArchiveRecord {
            target: result.target.clone(),
            status: result.status.clone(),
            level: result.level.clone(),
            format: "tar.gz".to_string(),
            archive: archive_path.display().to_string(),
            sha256: verify::sha256_file(&archive_path)?,
            size,
            artifact_sha256: Some(artifact.sha256.clone()),
            target_manifest: manifest_path.display().to_string(),
            archive_log: tar_log.display().to_string(),
        });

        if let Some(zip_path) = zip.path.as_ref() {
            let zip_archive = packages_dir.join(format!(
                "{}-{}-{}.zip",
                sanitize(&build_run.build.project),
                sanitize(&result.target),
                env!("CARGO_PKG_VERSION")
            ));
            let _ = fs::remove_file(&zip_archive);
            let zip_log = packages_dir.join(format!("{}.zip.log", sanitize(&result.target)));
            let mut command = CommandSpec::new(PathBuf::from(zip_path), &stage);
            command.arg("-q");
            command.arg("-r");
            command.arg(zip_archive.display().to_string());
            command.arg(".");
            command.timeout(timeout);
            let output = exec::run(&command, &zip_log)?;
            if !output.success {
                bail!(
                    "zip failed for target `{}`{}; see {}",
                    result.target,
                    if output.timed_out { " (timed out)" } else { "" },
                    zip_log.display()
                );
            }
            let size = fs::metadata(&zip_archive)
                .with_context(|| format!("cannot stat {}", zip_archive.display()))?
                .len();
            archives.push(ArchiveRecord {
                target: result.target.clone(),
                status: result.status.clone(),
                level: result.level.clone(),
                format: "zip".to_string(),
                archive: zip_archive.display().to_string(),
                sha256: verify::sha256_file(&zip_archive)?,
                size,
                artifact_sha256: Some(artifact.sha256.clone()),
                target_manifest: manifest_path.display().to_string(),
                archive_log: zip_log.display().to_string(),
            });
        }
        let _ = fs::remove_dir_all(&stage);
    }
    let _ = fs::remove_dir(packages_dir.join(".stage"));

    let release_manifest = PathBuf::from(&build_run.release_manifest_path);
    let release_manifest_sha256 = if release_manifest.is_file() {
        Some(verify::sha256_file(&release_manifest)?)
    } else {
        None
    };
    let packages = PackageManifest {
        schema_version: 1,
        project: build_run.build.project.clone(),
        source: manifest::source_record(source),
        generated_at_unix: manifest::now_unix(),
        release_manifest: build_run.release_manifest_path.clone(),
        release_manifest_sha256,
        archives,
        notes,
    };
    let package_manifest_path = packages_dir.join("manifest.json");
    manifest::write(&package_manifest_path, &packages)?;

    let mut sums = String::new();
    for archive in &packages.archives {
        let name = Path::new(&archive.archive)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        sums.push_str(&format!("{}  {name}\n", archive.sha256));
    }
    fs::write(packages_dir.join("SHA256SUMS"), sums)
        .with_context(|| format!("cannot write {}", packages_dir.join("SHA256SUMS").display()))?;

    Ok(PackageRun {
        release: build_run,
        packages,
        package_manifest_path: package_manifest_path.display().to_string(),
    })
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed
    }
}
