use crate::{
    adapter::{self, BuildContext, BuildResult},
    cache::{self, Cache, CacheEntry, CacheRecord},
    config::Config,
    detect::DetectionReport,
    exec::{self, CommandSpec},
    manifest::{
        self, CommandRecord, ReleaseManifest, ReleaseTarget, TargetManifest, TargetRecord,
        VerificationRecord,
    },
    plan::{self, ReadinessStatus},
    target::{self, TargetSpec},
    toolchain,
    verify::{self, ArtifactInfo, VerificationCheck},
};
use anyhow::{Result, anyhow};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Serialize)]
pub struct BuildRun {
    pub build: BuildReport,
    pub release_manifest: ReleaseManifest,
    pub release_manifest_path: String,
}

#[derive(Debug, Serialize)]
pub struct BuildReport {
    pub schema_version: u32,
    pub project: String,
    pub source: String,
    pub generated_at_unix: u64,
    pub results: Vec<TargetResult>,
}

#[derive(Debug, Serialize)]
pub struct TargetResult {
    pub target: String,
    pub adapter: String,
    pub status: String,
    pub level: String,
    pub artifact: Option<ArtifactInfo>,
    pub checks: Vec<VerificationCheck>,
    pub commands: Vec<CommandRecord>,
    pub run: Option<CommandRecord>,
    pub toolchain: BTreeMap<String, String>,
    pub cache: Option<CacheRecord>,
    pub error: Option<String>,
    pub manifest_path: Option<String>,
}

pub fn run(
    source: &Path,
    config: &Config,
    report: &DetectionReport,
    targets: &[&'static TargetSpec],
) -> Result<BuildRun> {
    let primary = plan::primary_project(report)
        .ok_or_else(|| anyhow!("no supported project detected in {}", report.source))?;
    let project_root = if primary.root == "." {
        source.to_path_buf()
    } else {
        source.join(&primary.root)
    };
    let display_name = if config.project.name.trim().is_empty() || config.project.name == "auto" {
        primary.name.clone()
    } else {
        config.project.name.clone()
    };
    let project_dist =
        artifacts_dir(source, &config.artifacts.directory).join(sanitize(&display_name));
    let source_record = manifest::source_record(source);
    let generated_at = manifest::now_unix();
    let timeout = Duration::from_secs(config.build.timeout_secs.max(1));
    let adapter = adapter::for_build_system(&primary.build_system);
    let cache_store = Cache::new(source, config);
    let cache_root = cache_store.enabled(config).then(|| cache_store.root());

    let mut results = Vec::new();
    let mut release_targets = Vec::new();

    for spec in targets {
        let readiness = plan::target_readiness(spec, &primary.build_system);
        if readiness.status != ReadinessStatus::Ready {
            results.push(skipped_result(
                spec,
                readiness
                    .reason
                    .unwrap_or_else(|| format!("target is {}", readiness.status.as_str())),
                &primary.build_system,
            ));
            continue;
        }

        let Some(adapter) = adapter.as_deref() else {
            results.push(skipped_result(
                spec,
                format!("`{}` adapter is not implemented", primary.build_system),
                &primary.build_system,
            ));
            continue;
        };

        let work_dir = project_dist.join(spec.id);
        fs::create_dir_all(&work_dir)?;
        let context = BuildContext {
            source,
            project: primary,
            root: &project_root,
            target: spec,
            mode: &config.build.mode,
            work_dir: &work_dir,
            cache_root,
            timeout,
        };

        let cache_key = if cache_store.enabled(config) {
            Some(cache_store.key(&cache::KeyInput {
                source,
                project_root: &project_root,
                project_name: &display_name,
                build_system: &primary.build_system,
                target: spec,
                config,
            })?)
        } else {
            None
        };
        let mut cache_record = cache_key.as_ref().map(|key| CacheRecord {
            key: key.clone(),
            hit: false,
            directory: cache_store.root().display().to_string(),
        });
        let cached = match &cache_key {
            Some(key) => cache_store.restore(key, &work_dir)?,
            None => None,
        };
        let from_cache = cached.is_some();
        if from_cache && let Some(record) = &mut cache_record {
            record.hit = true;
        }
        let build_result = match cached {
            Some(cached) => Ok(BuildResult {
                artifact: cached.artifact,
                commands: Vec::new(),
                toolchain: cached.toolchain,
            }),
            None => adapter.build(&context),
        };

        let result = match build_result {
            Ok(built) => {
                let inspection = verify::inspect(&built.artifact, spec)?;
                let mut status = "success";
                let mut level = if inspection.passed() {
                    "inspected"
                } else {
                    "built"
                };
                let mut error = None;

                if !inspection.passed() {
                    status = "failed";
                    error = Some("artifact verification failed".to_string());
                }

                let mut run_record = None;
                if status == "success"
                    && config.verification.run_tests
                    && let Some(mut run_spec) =
                        runtime_command(spec, &built.artifact, &project_root)
                {
                    run_spec.timeout(timeout);
                    let run_log = work_dir.join("run.log");
                    match exec::run(&run_spec, &run_log) {
                        Ok(output) => {
                            if output.success {
                                level = "executed";
                            } else {
                                status = "failed";
                                error = Some(format!(
                                    "artifact exited with code {:?}",
                                    output.exit_code
                                ));
                            }
                            run_record = Some(CommandRecord::from(&output));
                        }
                        Err(run_error) => {
                            status = "failed";
                            error = Some(format!("could not run artifact: {run_error:#}"));
                        }
                    }
                }

                if status == "success"
                    && !from_cache
                    && let Some(key) = &cache_key
                {
                    let entry = CacheEntry {
                        schema_version: 1,
                        key: key.clone(),
                        project: display_name.clone(),
                        target: spec.id.to_string(),
                        abi: spec.abi.to_string(),
                        adapter: adapter.name().to_string(),
                        build_mode: config.build.mode.clone(),
                        artifact_file: String::new(),
                        artifact_sha256: String::new(),
                        level: level.to_string(),
                        toolchain: built.toolchain.clone(),
                        created_at_unix: 0,
                    };
                    cache_store.store(key, &built.artifact, entry)?;
                }

                TargetResult {
                    target: spec.id.to_string(),
                    adapter: adapter.name().to_string(),
                    status: status.to_string(),
                    level: level.to_string(),
                    artifact: Some(inspection.artifact.clone()),
                    checks: inspection.checks.clone(),
                    commands: built.commands.iter().map(CommandRecord::from).collect(),
                    run: run_record,
                    toolchain: built.toolchain.clone(),
                    cache: cache_record.clone(),
                    error,
                    manifest_path: None,
                }
            }
            Err(build_error) => TargetResult {
                target: spec.id.to_string(),
                adapter: adapter.name().to_string(),
                status: "failed".to_string(),
                level: "none".to_string(),
                artifact: None,
                checks: Vec::new(),
                commands: Vec::new(),
                run: None,
                toolchain: BTreeMap::new(),
                cache: cache_record.clone(),
                error: Some(format!("{build_error:#}")),
                manifest_path: None,
            },
        };

        let target_manifest = TargetManifest {
            schema_version: 1,
            project: display_name.clone(),
            adapter: result.adapter.clone(),
            target: target_record(spec),
            build_mode: config.build.mode.clone(),
            source: source_record.clone(),
            status: result.status.clone(),
            verification: VerificationRecord {
                level: result.level.clone(),
                checks: result.checks.clone(),
            },
            toolchain: result.toolchain.clone(),
            artifact: result.artifact.clone(),
            commands: result.commands.clone(),
            run: result.run.clone(),
            cache: result.cache.clone(),
            error: result.error.clone(),
            generated_at_unix: generated_at,
        };
        let manifest_path = work_dir.join("manifest.json");
        manifest::write(&manifest_path, &target_manifest)?;

        let mut result = result;
        result.manifest_path = Some(manifest_path.display().to_string());

        release_targets.push(ReleaseTarget {
            target: spec.id.to_string(),
            status: result.status.clone(),
            level: result.level.clone(),
            artifact: result
                .artifact
                .as_ref()
                .map(|artifact| artifact.path.clone()),
            sha256: result
                .artifact
                .as_ref()
                .map(|artifact| artifact.sha256.clone()),
            manifest: manifest_path.display().to_string(),
            cached: result
                .cache
                .as_ref()
                .map(|cache| cache.hit)
                .unwrap_or(false),
        });
        results.push(result);
    }

    let release_manifest = ReleaseManifest {
        schema_version: 1,
        project: display_name.clone(),
        source: source_record,
        generated_at_unix: generated_at,
        targets: release_targets,
    };
    let release_manifest_path = project_dist.join("manifest.json");
    manifest::write(&release_manifest_path, &release_manifest)?;

    Ok(BuildRun {
        build: BuildReport {
            schema_version: 1,
            project: display_name,
            source: report.source.clone(),
            generated_at_unix: generated_at,
            results,
        },
        release_manifest,
        release_manifest_path: release_manifest_path.display().to_string(),
    })
}

fn runtime_command(spec: &TargetSpec, artifact: &Path, cwd: &Path) -> Option<CommandSpec> {
    let host = target::host();
    let same_platform = host
        .map(|host| host.os == spec.os && host.arch == spec.arch)
        .unwrap_or(false);
    if spec.id == target::host_id() || same_platform {
        return Some(CommandSpec::new(artifact.to_path_buf(), cwd));
    }

    if spec.id == "linux-x86_64-musl" {
        let qemu = toolchain::check_tool("qemu-x86_64", None, None);
        if let Some(qemu_path) = qemu.path {
            let mut command = CommandSpec::new(PathBuf::from(qemu_path), cwd);
            command.arg(artifact.display().to_string());
            return Some(command);
        }
    }

    if spec.os == "wasi" {
        let wasmtime = toolchain::check_tool("wasmtime", None, None);
        if let Some(wasmtime_path) = wasmtime.path {
            let mut command = CommandSpec::new(PathBuf::from(wasmtime_path), cwd);
            command.arg("run");
            command.arg(artifact.display().to_string());
            return Some(command);
        }
    }

    None
}

fn skipped_result(spec: &TargetSpec, reason: String, adapter: &str) -> TargetResult {
    TargetResult {
        target: spec.id.to_string(),
        adapter: adapter.to_string(),
        status: "skipped".to_string(),
        level: "none".to_string(),
        artifact: None,
        checks: Vec::new(),
        commands: Vec::new(),
        run: None,
        toolchain: BTreeMap::new(),
        cache: None,
        error: Some(reason),
        manifest_path: None,
    }
}

fn target_record(spec: &TargetSpec) -> TargetRecord {
    TargetRecord {
        id: spec.id.to_string(),
        os: spec.os.to_string(),
        arch: spec.arch.to_string(),
        abi: spec.abi.to_string(),
        llvm_triple: spec.llvm_triple.to_string(),
        binary_format: spec.binary_format.to_string(),
    }
}

fn artifacts_dir(source: &Path, configured: &str) -> PathBuf {
    let path = PathBuf::from(configured);
    if path.is_absolute() {
        path
    } else {
        source.join(path)
    }
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
