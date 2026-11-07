use crate::{
    config::{Config, VerificationConfig},
    detect::DetectionReport,
    target::{self, TargetSpec, Tier},
    toolchain::{self, ToolCheck},
};
use anyhow::{Result, anyhow};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct BuildPlan {
    pub schema_version: u32,
    pub project: PlannedProject,
    pub build_mode: String,
    pub verification: VerificationConfig,
    pub targets: Vec<PlannedTarget>,
}

#[derive(Debug, Serialize)]
pub struct PlannedProject {
    pub name: String,
    pub root: String,
    pub language: String,
    pub build_system: String,
}

#[derive(Debug, Serialize)]
pub struct PlannedTarget {
    pub id: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub abi: &'static str,
    pub tier: Tier,
    pub llvm_triple: &'static str,
    pub rust_triple: &'static str,
    pub flint_triple: &'static str,
    pub binary_format: &'static str,
    pub readiness: Readiness,
}

#[derive(Debug, Serialize)]
pub struct Readiness {
    pub status: ReadinessStatus,
    pub build_host: String,
    pub cross: bool,
    pub adapter: String,
    pub reason: Option<String>,
    pub tools: Vec<ToolCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessStatus {
    Ready,
    Blocked,
    Experimental,
    Unsupported,
}

impl ReadinessStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ReadinessStatus::Ready => "ready",
            ReadinessStatus::Blocked => "blocked",
            ReadinessStatus::Experimental => "experimental",
            ReadinessStatus::Unsupported => "unsupported",
        }
    }
}

pub fn build_plan(
    source: &Path,
    config: &Config,
    report: &DetectionReport,
    targets: &[&'static TargetSpec],
) -> Result<BuildPlan> {
    let primary = primary_project(report)
        .ok_or_else(|| anyhow!("no supported project detected in {}", report.source))?;

    let name = if config.project.name.trim().is_empty() || config.project.name == "auto" {
        primary.name.clone()
    } else {
        config.project.name.clone()
    };

    let root = if primary.root == "." {
        report.source.clone()
    } else {
        source.join(&primary.root).display().to_string()
    };

    let planned_targets = targets
        .iter()
        .map(|spec| PlannedTarget {
            id: spec.id,
            os: spec.os,
            arch: spec.arch,
            abi: spec.abi,
            tier: spec.tier,
            llvm_triple: spec.llvm_triple,
            rust_triple: spec.rust_triple,
            flint_triple: spec.flint_triple,
            binary_format: spec.binary_format,
            readiness: target_readiness(spec, &primary.build_system),
        })
        .collect();

    Ok(BuildPlan {
        schema_version: 1,
        project: PlannedProject {
            name,
            root,
            language: primary.language.clone(),
            build_system: primary.build_system.clone(),
        },
        build_mode: config.build.mode.clone(),
        verification: config.verification.clone(),
        targets: planned_targets,
    })
}

pub fn primary_project(report: &DetectionReport) -> Option<&crate::detect::Detection> {
    report.projects.iter().min_by_key(|project| {
        (
            root_depth(&project.root),
            build_system_priority(&project.build_system),
            project.root.clone(),
        )
    })
}

fn root_depth(root: &str) -> usize {
    if root == "." {
        0
    } else {
        root.split('/').count()
    }
}

fn build_system_priority(build_system: &str) -> usize {
    match build_system {
        "Cargo" => 0,
        "CMake" => 1,
        "Flint" => 2,
        "Go" => 3,
        "npm" => 4,
        "Maven" => 5,
        "Gradle" => 6,
        "SwiftPM" => 7,
        "dotnet" => 8,
        _ => 9,
    }
}

pub fn target_readiness(spec: &'static TargetSpec, adapter: &str) -> Readiness {
    let host = target::host();
    let cross = host.map(|host| host.id != spec.id).unwrap_or(true);
    let mut tools = Vec::new();

    let (status, reason) = if spec.tier == Tier::Unsupported {
        (
            ReadinessStatus::Unsupported,
            Some(format!("excluded from 0.1: {}", spec.notes)),
        )
    } else {
        match adapter {
            "Cargo" => {
                let cargo = toolchain::check_tool("cargo", None, None);
                tools.push(cargo.clone());
                if !cargo.available {
                    (
                        ReadinessStatus::Blocked,
                        Some("cargo not found; install a Rust toolchain".to_string()),
                    )
                } else if !cross {
                    (ReadinessStatus::Ready, None)
                } else {
                    let std_installed = toolchain::rust_std_installed(spec.rust_triple);
                    tools.push(ToolCheck {
                        tool: format!("rust-std:{}", spec.rust_triple),
                        available: std_installed,
                        path: None,
                        hint: Some(format!("rustup target add {}", spec.rust_triple)),
                    });
                    if std_installed {
                        (ReadinessStatus::Ready, None)
                    } else {
                        (
                            ReadinessStatus::Blocked,
                            Some(format!(
                                "Rust std for `{}` is not installed",
                                spec.rust_triple
                            )),
                        )
                    }
                }
            }
            "Flint" => {
                let flintc = toolchain::check_tool(
                    "flintc",
                    Some("FLINTC"),
                    Some("set FLINTC or add flintc to PATH"),
                );
                tools.push(flintc.clone());
                if !flintc.available {
                    (
                        ReadinessStatus::Blocked,
                        Some("flintc not found; set FLINTC or add it to PATH".to_string()),
                    )
                } else if !cross {
                    (ReadinessStatus::Ready, None)
                } else {
                    let zig = toolchain::check_tool("zig", None, Some("pkg install zig"));
                    tools.push(zig.clone());
                    let runtime_ready = flintc
                        .path
                        .as_deref()
                        .map(Path::new)
                        .and_then(Path::parent)
                        .map(|root| {
                            root.join("runtime/runtime.c").is_file()
                                && root.join("runtime/flint_aegis.c").is_file()
                        })
                        .unwrap_or(false);
                    if spec.os != "linux" {
                        (
                            ReadinessStatus::Experimental,
                            Some(format!(
                                "Flint cross-link is implemented for Linux targets; `{}` remains unimplemented",
                                spec.os
                            )),
                        )
                    } else if !matches!(spec.arch, "x86_64" | "aarch64") {
                        (
                            ReadinessStatus::Experimental,
                            Some(format!(
                                "Flint cross-link is provisioned for x86_64/aarch64 Linux; `{}` is not tested",
                                spec.arch
                            )),
                        )
                    } else if !zig.available {
                        (
                            ReadinessStatus::Blocked,
                            Some("Flint cross-link needs zig; install zig".to_string()),
                        )
                    } else if !runtime_ready {
                        (
                            ReadinessStatus::Blocked,
                            Some(
                                "Flint runtime sources were not found next to flintc; cannot cross-link"
                                    .to_string(),
                            ),
                        )
                    } else {
                        (
                            ReadinessStatus::Ready,
                            Some(
                                "cross-links the Flint object with a Zig-built C runtime"
                                    .to_string(),
                            ),
                        )
                    }
                }
            }
            "CMake" => {
                let cmake = toolchain::check_tool("cmake", None, Some("pkg install cmake"));
                tools.push(cmake.clone());
                if !cmake.available {
                    (
                        ReadinessStatus::Blocked,
                        Some("cmake not found; install it to use the CMake adapter".to_string()),
                    )
                } else if !cross {
                    let clang = toolchain::check_tool("clang", None, None);
                    tools.push(clang.clone());
                    if clang.available {
                        (ReadinessStatus::Ready, None)
                    } else {
                        (
                            ReadinessStatus::Blocked,
                            Some("clang not found; install a C/C++ toolchain".to_string()),
                        )
                    }
                } else {
                    let zig = toolchain::check_tool("zig", None, Some("pkg install zig"));
                    tools.push(zig.clone());
                    if !zig.available {
                        (
                            ReadinessStatus::Blocked,
                            Some(
                                "cross C/C++ needs zig or a target sysroot; install zig"
                                    .to_string(),
                            ),
                        )
                    } else if matches!(spec.os, "linux" | "windows") {
                        (ReadinessStatus::Ready, None)
                    } else {
                        (
                            ReadinessStatus::Experimental,
                            Some(format!(
                                "CMake cross-build for `{}` is not implemented yet",
                                spec.os
                            )),
                        )
                    }
                }
            }
            "Go" => {
                let go = toolchain::check_tool("go", None, Some("pkg install golang"));
                tools.push(go.clone());
                if !go.available {
                    (
                        ReadinessStatus::Blocked,
                        Some("go not found; install the Go toolchain".to_string()),
                    )
                } else {
                    (
                        ReadinessStatus::Experimental,
                        Some("Go adapter is not implemented yet".to_string()),
                    )
                }
            }
            "npm" => {
                let node = toolchain::check_tool("node", None, None);
                tools.push(node.clone());
                if !node.available {
                    (
                        ReadinessStatus::Blocked,
                        Some("node not found; install Node.js".to_string()),
                    )
                } else {
                    (
                        ReadinessStatus::Experimental,
                        Some("JavaScript adapter is not implemented yet".to_string()),
                    )
                }
            }
            "Maven" | "Gradle" => {
                let java = toolchain::check_tool("java", None, None);
                tools.push(java.clone());
                if !java.available {
                    (
                        ReadinessStatus::Blocked,
                        Some("java not found; install a JDK".to_string()),
                    )
                } else {
                    (
                        ReadinessStatus::Experimental,
                        Some(format!("{adapter} adapter is not implemented yet")),
                    )
                }
            }
            "SwiftPM" => {
                let swift = toolchain::check_tool("swift", None, None);
                tools.push(swift.clone());
                if !swift.available {
                    (
                        ReadinessStatus::Blocked,
                        Some("swift not found; install a Swift toolchain".to_string()),
                    )
                } else {
                    (
                        ReadinessStatus::Experimental,
                        Some("Swift adapter is not implemented yet".to_string()),
                    )
                }
            }
            "dotnet" => {
                let dotnet = toolchain::check_tool("dotnet", None, None);
                tools.push(dotnet.clone());
                if !dotnet.available {
                    (
                        ReadinessStatus::Blocked,
                        Some("dotnet not found; install the .NET SDK".to_string()),
                    )
                } else {
                    (
                        ReadinessStatus::Experimental,
                        Some(".NET adapter is not implemented yet".to_string()),
                    )
                }
            }
            other => (
                ReadinessStatus::Blocked,
                Some(format!("no adapter for build system `{other}`")),
            ),
        }
    };

    if spec.id == "linux-x86_64-musl" {
        tools.push(toolchain::check_tool(
            "qemu-x86_64",
            None,
            Some("pkg install qemu-user-x86-64"),
        ));
    }

    Readiness {
        status,
        build_host: target::host_id().to_string(),
        cross,
        adapter: adapter.to_string(),
        reason,
        tools,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, detect};
    use std::fs;
    use std::path::PathBuf;

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

    fn rust_project(tag: &str) -> PathBuf {
        let root = tempdir(tag);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        root
    }

    #[test]
    fn host_target_is_ready_for_native_cargo() {
        let Some(host) = target::host() else {
            return;
        };
        let root = rust_project("plan-host");
        let report = detect::detect(&root, 4).unwrap();
        let config = Config::default();
        let plan = build_plan(&root, &config, &report, &[host]).unwrap();
        assert_eq!(plan.project.name, "demo");
        assert_eq!(plan.targets[0].readiness.status, ReadinessStatus::Ready);
    }

    #[test]
    fn cross_cargo_target_is_blocked_without_std() {
        let Some(host) = target::host() else {
            return;
        };
        if host.id == "windows-x86_64" {
            return;
        }
        let root = rust_project("plan-cross");
        let report = detect::detect(&root, 4).unwrap();
        let config = Config::default();
        let windows = target::by_id("windows-x86_64").unwrap();
        let plan = build_plan(&root, &config, &report, &[windows]).unwrap();
        assert_eq!(plan.targets[0].readiness.status, ReadinessStatus::Blocked);
        assert!(plan.targets[0].readiness.cross);
    }

    #[test]
    fn unsupported_target_is_reported() {
        let Some(host) = target::host() else {
            return;
        };
        let root = rust_project("plan-unsupported");
        let report = detect::detect(&root, 4).unwrap();
        let config = Config::default();
        let macos = target::by_id("macos-arm64").unwrap();
        let plan = build_plan(&root, &config, &report, &[macos]).unwrap();
        assert_eq!(
            plan.targets[0].readiness.status,
            ReadinessStatus::Unsupported
        );
        let _ = host;
    }
}
