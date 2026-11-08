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

fn check_any_tool(names: &[&str], hint: &str) -> ToolCheck {
    for name in names {
        let tool = toolchain::check_tool(name, None, Some(hint));
        if tool.available {
            return tool;
        }
    }
    toolchain::check_tool(names[0], None, Some(hint))
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
                    let rust_src = toolchain::rust_src_installed();
                    tools.push(ToolCheck {
                        tool: format!("rust-std:{}", spec.rust_triple),
                        available: std_installed,
                        path: None,
                        hint: Some(if rust_src {
                            "missing target std; will build it from rust-src".to_string()
                        } else {
                            format!("rustup target add {}", spec.rust_triple)
                        }),
                    });
                    if !std_installed {
                        tools.push(ToolCheck {
                            tool: "rust-src".to_string(),
                            available: rust_src,
                            path: None,
                            hint: Some(
                                "pkg install rust-src or rustup component add rust-src".to_string(),
                            ),
                        });
                    }
                    let linker = if spec.os == "wasi" {
                        None
                    } else {
                        Some(toolchain::check_tool("zig", None, Some("pkg install zig")))
                    };
                    if let Some(linker) = &linker {
                        tools.push(linker.clone());
                    }
                    let windows_binutils_ready = if spec.os == "windows" && !std_installed {
                        let dlltool = check_any_tool(
                            &["llvm-dlltool", "dlltool"],
                            "install llvm-dlltool or mingw-w64 dlltool",
                        );
                        let ar = check_any_tool(
                            &["llvm-ar", "ar"],
                            "install llvm-ar or a target archiver",
                        );
                        let ready = dlltool.available && ar.available;
                        tools.push(dlltool);
                        tools.push(ar);
                        ready
                    } else {
                        true
                    };
                    let macos_signer_ready = if spec.os == "macos" {
                        let signer = check_any_tool(
                            &["rcodesign", "codesign"],
                            "install rcodesign: cargo install apple-codesign",
                        );
                        let ready = signer.available;
                        tools.push(signer);
                        ready
                    } else {
                        true
                    };
                    if !std_installed && !rust_src {
                        (
                            ReadinessStatus::Blocked,
                            Some(format!(
                                "Rust std for `{}` is missing and rust-src is not installed",
                                spec.rust_triple
                            )),
                        )
                    } else if linker.as_ref().is_some_and(|tool| !tool.available) {
                        (
                            ReadinessStatus::Blocked,
                            Some(format!(
                                "Rust cross-linking `{}` needs {}",
                                spec.rust_triple,
                                linker
                                    .as_ref()
                                    .map(|tool| tool.tool.as_str())
                                    .unwrap_or("a linker")
                            )),
                        )
                    } else if !windows_binutils_ready {
                        (
                            ReadinessStatus::Blocked,
                            Some(
                                "building Rust std for Windows needs llvm-dlltool and llvm-ar"
                                    .to_string(),
                            ),
                        )
                    } else if !macos_signer_ready {
                        (
                            ReadinessStatus::Blocked,
                            Some(
                                "macOS artifacts need ad-hoc signing; install rcodesign with `cargo install apple-codesign`"
                                    .to_string(),
                            ),
                        )
                    } else if spec.os == "macos" {
                        (
                            ReadinessStatus::Ready,
                            Some("builds and ad-hoc signs a Mach-O artifact for macOS".to_string()),
                        )
                    } else if spec.os == "wasi" {
                        let wasmtime =
                            toolchain::check_tool("wasmtime", None, Some("pkg install wasmtime"));
                        let available = wasmtime.available;
                        tools.push(wasmtime);
                        (
                            ReadinessStatus::Ready,
                            Some(if available {
                                "builds WASI; executes with wasmtime".to_string()
                            } else {
                                "builds WASI; install wasmtime to execute".to_string()
                            }),
                        )
                    } else if std_installed {
                        (ReadinessStatus::Ready, None)
                    } else {
                        (
                            ReadinessStatus::Ready,
                            Some(format!(
                                "builds Rust std from rust-src with {}",
                                linker
                                    .as_ref()
                                    .map(|tool| tool.tool.as_str())
                                    .unwrap_or("linker")
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
                    if spec.os == "wasi" {
                        if !zig.available {
                            (
                                ReadinessStatus::Blocked,
                                Some("Flint WASI needs zig; install zig".to_string()),
                            )
                        } else if !runtime_ready {
                            (
                                ReadinessStatus::Blocked,
                                Some(
                                    "Flint runtime sources were not found next to flintc; cannot build WASI"
                                        .to_string(),
                                ),
                            )
                        } else {
                            let wasmtime = toolchain::check_tool(
                                "wasmtime",
                                None,
                                Some("pkg install wasmtime"),
                            );
                            let available = wasmtime.available;
                            tools.push(wasmtime);
                            (
                                ReadinessStatus::Ready,
                                Some(if available {
                                    "builds Flint WASI; executes with wasmtime".to_string()
                                } else {
                                    "builds Flint WASI; install wasmtime to execute".to_string()
                                }),
                            )
                        }
                    } else if spec.os == "android" {
                        (
                            ReadinessStatus::Experimental,
                            Some(
                                "Flint Android cross-builds need a native Android toolchain; run OmniBuild on the Android device"
                                    .to_string(),
                            ),
                        )
                    } else if !matches!(spec.os, "linux" | "windows" | "macos") {
                        (
                            ReadinessStatus::Experimental,
                            Some(format!(
                                "Flint cross-link for `{}` is not implemented",
                                spec.os
                            )),
                        )
                    } else if spec.os == "linux" && !matches!(spec.arch, "x86_64" | "aarch64") {
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
                        let macos_signer_ready = if spec.os == "macos" {
                            let signer = check_any_tool(
                                &["rcodesign", "codesign"],
                                "install rcodesign: cargo install apple-codesign",
                            );
                            let ready = signer.available;
                            tools.push(signer);
                            ready
                        } else {
                            true
                        };
                        if !macos_signer_ready {
                            (
                                ReadinessStatus::Blocked,
                                Some(
                                    "macOS Flint artifacts need ad-hoc signing; install rcodesign with `cargo install apple-codesign`"
                                        .to_string(),
                                ),
                            )
                        } else {
                            let reason = match spec.os {
                                "windows" => {
                                    "cross-links the Flint object with Zig's MinGW runtime; execute on Windows"
                                }
                                "macos" => {
                                    "cross-links the Flint object with Zig's macOS runtime and ad-hoc signs it"
                                }
                                _ => "cross-links the Flint object with a Zig-built C runtime",
                            };
                            (ReadinessStatus::Ready, Some(reason.to_string()))
                        }
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
                    } else if spec.os == "wasi" {
                        let wasmtime =
                            toolchain::check_tool("wasmtime", None, Some("pkg install wasmtime"));
                        let available = wasmtime.available;
                        tools.push(wasmtime);
                        (
                            ReadinessStatus::Ready,
                            Some(if available {
                                "builds WASI; executes with wasmtime".to_string()
                            } else {
                                "builds WASI; install wasmtime to execute".to_string()
                            }),
                        )
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
    fn cross_cargo_target_reports_actionable_readiness() {
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
        let readiness = &plan.targets[0].readiness;
        assert!(readiness.cross);
        match readiness.status {
            ReadinessStatus::Ready => {}
            ReadinessStatus::Blocked => {
                let reason = readiness.reason.as_deref().unwrap_or_default();
                assert!(
                    ["rust-src", "Zig", "llvm-dlltool", "cargo"]
                        .iter()
                        .any(|needle| reason.contains(needle)),
                    "blocked without an actionable reason: {reason}"
                );
            }
            ref other => panic!("unexpected cross readiness: {other:?}"),
        }
    }

    #[test]
    fn macos_target_reports_actionable_readiness() {
        let Some(host) = target::host() else {
            return;
        };
        if host.id == "macos-arm64" {
            return;
        }
        let root = rust_project("plan-macos");
        let report = detect::detect(&root, 4).unwrap();
        let config = Config::default();
        let macos = target::by_id("macos-arm64").unwrap();
        let plan = build_plan(&root, &config, &report, &[macos]).unwrap();
        let readiness = &plan.targets[0].readiness;
        assert!(readiness.cross);
        match readiness.status {
            ReadinessStatus::Ready => {}
            ReadinessStatus::Blocked => {
                let reason = readiness.reason.as_deref().unwrap_or_default();
                assert!(
                    ["rust-src", "Zig", "rcodesign", "cargo"]
                        .iter()
                        .any(|needle| reason.contains(needle)),
                    "blocked without an actionable reason: {reason}"
                );
            }
            ref other => panic!("unexpected macOS readiness: {other:?}"),
        }
    }
}
