use crate::{
    detect::Detection,
    exec::{self, CommandOutput, CommandSpec},
    target::{self, TargetSpec},
    toolchain,
};
use anyhow::{Context, Result, anyhow, bail};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct BuildContext<'a> {
    pub source: &'a Path,
    pub project: &'a Detection,
    pub root: &'a Path,
    pub target: &'static TargetSpec,
    pub mode: &'a str,
    pub work_dir: &'a Path,
    pub timeout: Duration,
}

pub struct BuildResult {
    pub artifact: PathBuf,
    pub commands: Vec<CommandOutput>,
    pub toolchain: BTreeMap<String, String>,
}

pub trait Adapter {
    fn name(&self) -> &'static str;
    fn build(&self, context: &BuildContext<'_>) -> Result<BuildResult>;
}

pub fn for_build_system(build_system: &str) -> Option<Box<dyn Adapter>> {
    match build_system {
        "Cargo" => Some(Box::new(CargoAdapter)),
        "CMake" => Some(Box::new(CMakeAdapter)),
        "Flint" => Some(Box::new(FlintAdapter)),
        _ => None,
    }
}

struct CargoAdapter;
struct CMakeAdapter;
struct FlintAdapter;

impl Adapter for CargoAdapter {
    fn name(&self) -> &'static str {
        "cargo"
    }

    fn build(&self, context: &BuildContext<'_>) -> Result<BuildResult> {
        let cross = context.target.id != target::host_id();
        let wasi = context.target.os == "wasi";
        let cargo = toolchain::check_tool("cargo", None, None);
        let Some(cargo_path) = cargo.path else {
            bail!("cargo not found; install a Rust toolchain");
        };

        fs::create_dir_all(context.work_dir)
            .with_context(|| format!("cannot create {}", context.work_dir.display()))?;

        let std_installed = !cross || toolchain::rust_std_installed(context.target.rust_triple);
        let build_std = cross && !std_installed;
        if build_std && !toolchain::rust_src_installed() {
            bail!(
                "Rust std for `{}` is not installed and rust-src is missing; install the target std or rust-src",
                context.target.rust_triple
            );
        }

        let release = context.mode != "debug";
        let mut spec = CommandSpec::new(PathBuf::from(cargo_path), context.root);
        spec.arg("build");
        if release {
            spec.arg("--release");
        }
        if cross {
            spec.arg("--target");
            spec.arg(context.target.rust_triple);
        }
        if build_std {
            spec.arg("-Z");
            spec.arg("build-std=std,panic_abort");
            spec.env("RUSTC_BOOTSTRAP", "1");
        }
        if cross && !wasi {
            let linker = write_cross_linker(context)?;
            spec.env(cargo_linker_env(context.target.rust_triple), linker);
            if context.target.abi == "musl" {
                let flags = std::env::var("RUSTFLAGS").unwrap_or_default();
                spec.env(
                    "RUSTFLAGS",
                    format!("{} -C link-self-contained=no", flags).trim(),
                );
            }
        }
        if context.target.os == "windows"
            && let Some(path) = write_windows_binutils(context)?
        {
            let current = std::env::var("PATH").unwrap_or_default();
            let joined = std::env::join_paths(
                std::iter::once(std::path::PathBuf::from(&path))
                    .chain(std::env::split_paths(&current)),
            )
            .context("cannot construct PATH for Windows cross-tools")?;
            spec.env("PATH", joined.to_string_lossy().to_string());
        }
        if context.root.join("Cargo.lock").exists() {
            spec.arg("--locked");
        }
        spec.env("CARGO_TERM_COLOR", "never");
        spec.timeout(context.timeout);

        let log_path = context.work_dir.join("build.log");
        let command = exec::run(&spec, &log_path)?;
        if !command.success {
            bail!(
                "cargo build failed{}; see {}",
                if command.timed_out {
                    " (timed out)"
                } else {
                    ""
                },
                log_path.display()
            );
        }

        let profile = if release { "release" } else { "debug" };
        let profile_dir = if cross {
            context
                .root
                .join("target")
                .join(context.target.rust_triple)
                .join(profile)
        } else {
            context.root.join("target").join(profile)
        };
        let artifact = if wasi {
            find_wasm_artifact(&profile_dir, &context.project.name)?
        } else {
            find_executable(&profile_dir, &context.project.name)?
        };
        let destination = context
            .work_dir
            .join(artifact.file_name().unwrap_or_default());
        fs::copy(&artifact, &destination).with_context(|| {
            format!(
                "cannot copy artifact {} to {}",
                artifact.display(),
                destination.display()
            )
        })?;

        let mut commands = vec![command];
        if context.target.os == "macos" {
            commands.push(sign_macos_artifact(context, &destination)?);
        }

        let mut versions = BTreeMap::new();
        if let Some(version) = toolchain::version_of("cargo", &["--version"]) {
            versions.insert("cargo".to_string(), version);
        }
        if let Some(version) = toolchain::version_of("rustc", &["--version"]) {
            versions.insert("rustc".to_string(), version);
        }
        if build_std {
            versions.insert("rust_std".to_string(), "built from rust-src".to_string());
        }
        if context.target.os == "macos" {
            for signer in ["rcodesign", "codesign"] {
                if let Some(version) = toolchain::version_of(signer, &["--version"]) {
                    versions.insert(signer.to_string(), version);
                    break;
                }
            }
        }

        Ok(BuildResult {
            artifact: destination,
            commands,
            toolchain: versions,
        })
    }
}

impl Adapter for CMakeAdapter {
    fn name(&self) -> &'static str {
        "cmake"
    }

    fn build(&self, context: &BuildContext<'_>) -> Result<BuildResult> {
        let cross = context.target.id != target::host_id();
        let cmake = toolchain::check_tool("cmake", None, Some("pkg install cmake"));
        let Some(cmake_path) = cmake.path else {
            bail!("cmake not found; install it to use the CMake adapter");
        };

        fs::create_dir_all(context.work_dir)
            .with_context(|| format!("cannot create {}", context.work_dir.display()))?;

        let build_dir = context.work_dir.join("build");
        let build_type = if context.mode == "debug" {
            "Debug"
        } else {
            "Release"
        };

        let mut configure = CommandSpec::new(PathBuf::from(&cmake_path), context.root);
        configure.arg("-S");
        configure.arg(context.root.display().to_string());
        configure.arg("-B");
        configure.arg(build_dir.display().to_string());
        if toolchain::check_tool("ninja", None, None).available {
            configure.arg("-G");
            configure.arg("Ninja");
        }
        configure.arg(format!("-DCMAKE_BUILD_TYPE={build_type}"));

        let mut versions = BTreeMap::new();
        if cross {
            if !matches!(context.target.os, "linux" | "windows" | "wasi") {
                bail!(
                    "CMake cross-build for `{}` is not implemented yet",
                    context.target.os
                );
            }
            let zig = toolchain::check_tool("zig", None, Some("pkg install zig"));
            let Some(zig_path) = zig.path else {
                bail!("zig not found; cross C/C++ requires zig");
            };
            let toolchain_file = write_cmake_toolchain(context, &zig_path)?;
            configure.arg(format!(
                "-DCMAKE_TOOLCHAIN_FILE={}",
                toolchain_file.display()
            ));
            if let Some(version) = toolchain::version_of(&zig_path, &["version"]) {
                versions.insert("zig".to_string(), version);
            }
        }

        configure.timeout(context.timeout);
        let configure_log = context.work_dir.join("configure.log");
        let configure_output = exec::run(&configure, &configure_log)?;
        if !configure_output.success {
            bail!(
                "cmake configure failed{}; see {}",
                if configure_output.timed_out {
                    " (timed out)"
                } else {
                    ""
                },
                configure_log.display()
            );
        }

        let mut build = CommandSpec::new(PathBuf::from(&cmake_path), context.root);
        build.arg("--build");
        build.arg(build_dir.display().to_string());
        build.arg("--config");
        build.arg(build_type);
        build.arg("--parallel");
        build.timeout(context.timeout);
        let build_log = context.work_dir.join("build.log");
        let build_output = exec::run(&build, &build_log)?;
        if !build_output.success {
            bail!(
                "cmake build failed{}; see {}",
                if build_output.timed_out {
                    " (timed out)"
                } else {
                    ""
                },
                build_log.display()
            );
        }

        let artifact = find_cmake_artifact(&build_dir, &context.project.name)?;
        let destination = context
            .work_dir
            .join(artifact.file_name().unwrap_or_default());
        let destination =
            if context.target.binary_format == "WebAssembly" && destination.extension().is_none() {
                destination.with_extension("wasm")
            } else {
                destination
            };
        fs::copy(&artifact, &destination).with_context(|| {
            format!(
                "cannot copy artifact {} to {}",
                artifact.display(),
                destination.display()
            )
        })?;

        if let Some(version) = toolchain::version_of(&cmake_path, &["--version"]) {
            versions.insert("cmake".to_string(), version);
        }

        Ok(BuildResult {
            artifact: destination,
            commands: vec![configure_output, build_output],
            toolchain: versions,
        })
    }
}

fn write_cmake_toolchain(context: &BuildContext<'_>, zig: &str) -> Result<PathBuf> {
    let dir = context.work_dir.join("zig-toolchain");
    fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;

    write_wrapper(
        &dir.join("zig-cc"),
        zig,
        &["cc", "-target", context.target.zig_triple],
    )?;
    write_wrapper(
        &dir.join("zig-c++"),
        zig,
        &["c++", "-target", context.target.zig_triple],
    )?;
    write_wrapper(&dir.join("zig-ar"), zig, &["ar"])?;
    write_wrapper(&dir.join("zig-ranlib"), zig, &["ranlib"])?;

    let system = match context.target.os {
        "linux" => "Linux",
        "windows" => "Windows",
        "wasi" => "WASI",
        other => bail!("unsupported CMake cross system `{other}`"),
    };

    let mut toolchain = String::new();
    toolchain.push_str(&format!("set(CMAKE_SYSTEM_NAME {system})\n"));
    toolchain.push_str(&format!(
        "set(CMAKE_SYSTEM_PROCESSOR {})\n",
        context.target.arch
    ));
    toolchain.push_str(&format!(
        "set(CMAKE_C_COMPILER \"{}\")\n",
        dir.join("zig-cc").display()
    ));
    toolchain.push_str(&format!(
        "set(CMAKE_CXX_COMPILER \"{}\")\n",
        dir.join("zig-c++").display()
    ));
    toolchain.push_str(&format!(
        "set(CMAKE_AR \"{}\")\n",
        dir.join("zig-ar").display()
    ));
    toolchain.push_str(&format!(
        "set(CMAKE_RANLIB \"{}\")\n",
        dir.join("zig-ranlib").display()
    ));
    toolchain.push_str("set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)\n");
    if context.target.abi == "musl" {
        toolchain.push_str("set(CMAKE_EXE_LINKER_FLAGS_INIT \"-static\")\n");
    }

    let path = dir.join("toolchain.cmake");
    fs::write(&path, toolchain).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path)
}

fn write_wrapper(path: &Path, program: &str, args: &[&str]) -> Result<()> {
    let mut script = String::from("#!/bin/sh\nexec ");
    script.push_str(&shell_quote(program));
    for arg in args {
        script.push(' ');
        script.push_str(&shell_quote(arg));
    }
    script.push_str(" \"$@\"\n");
    fs::write(path, script).with_context(|| format!("cannot write {}", path.display()))?;
    make_executable(path)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .with_context(|| format!("cannot stat {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("cannot mark {} executable", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn find_cmake_artifact(dir: &Path, name: &str) -> Result<PathBuf> {
    let names = [
        name.to_string(),
        format!("{name}.exe"),
        format!("{name}.wasm"),
    ];
    for candidate in &names {
        let path = dir.join(candidate);
        if path.is_file() {
            return Ok(path);
        }
    }

    let mut matches = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.into_path();
        if path
            .components()
            .any(|component| component.as_os_str() == "CMakeFiles")
        {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if names.iter().any(|candidate| candidate == file_name) {
            matches.push(path);
        }
    }
    matches.sort();
    matches.into_iter().next().ok_or_else(|| {
        anyhow!(
            "no CMake artifact named `{name}` found under {}",
            dir.display()
        )
    })
}

impl Adapter for FlintAdapter {
    fn name(&self) -> &'static str {
        "flint"
    }

    fn build(&self, context: &BuildContext<'_>) -> Result<BuildResult> {
        if context.target.os == "wasi" {
            build_flint_wasi(context)
        } else if context.target.id == target::host_id() {
            build_flint_native(context)
        } else {
            build_flint_cross(context)
        }
    }
}

fn build_flint_native(context: &BuildContext<'_>) -> Result<BuildResult> {
    let flintc = toolchain::check_tool(
        "flintc",
        Some("FLINTC"),
        Some("set FLINTC or add flintc to PATH"),
    );
    let Some(flintc_path) = flintc.path else {
        bail!("flintc not found; set FLINTC or add flintc to PATH");
    };

    fs::create_dir_all(context.work_dir)
        .with_context(|| format!("cannot create {}", context.work_dir.display()))?;

    let main = flint_main(context)?;
    let mut artifact = context.work_dir.join(&context.project.name);
    if cfg!(windows) {
        artifact.set_extension("exe");
    }

    let mut spec = CommandSpec::new(PathBuf::from(flintc_path), context.root);
    spec.arg(main.display().to_string());
    spec.arg("-o");
    spec.arg(artifact.display().to_string());
    spec.arg("--no-run");
    spec.timeout(context.timeout);

    let log_path = context.work_dir.join("build.log");
    let command = exec::run(&spec, &log_path)?;
    if !command.success {
        bail!(
            "flintc build failed{}; see {}",
            if command.timed_out {
                " (timed out)"
            } else {
                ""
            },
            log_path.display()
        );
    }
    if !artifact.is_file() {
        bail!(
            "flintc reported success but no artifact exists at {}",
            artifact.display()
        );
    }

    let mut versions = BTreeMap::new();
    if let Some(version) = toolchain::version_of("flintc", &["--version"]) {
        versions.insert("flintc".to_string(), version);
    }
    if let Some(version) = toolchain::version_of("clang", &["--version"]) {
        versions.insert("clang".to_string(), version);
    }

    Ok(BuildResult {
        artifact,
        commands: vec![command],
        toolchain: versions,
    })
}

fn build_flint_cross(context: &BuildContext<'_>) -> Result<BuildResult> {
    if context.target.os != "linux" {
        bail!(
            "Flint cross-link is implemented for Linux targets only (target {})",
            context.target.id
        );
    }

    let flintc = toolchain::check_tool(
        "flintc",
        Some("FLINTC"),
        Some("set FLINTC or add flintc to PATH"),
    );
    let Some(flintc_path) = flintc.path else {
        bail!("flintc not found; set FLINTC or add flintc to PATH");
    };
    let zig = toolchain::check_tool("zig", None, Some("pkg install zig"));
    let Some(zig_path) = zig.path else {
        bail!("zig not found; Flint cross-link needs zig");
    };

    let flint_root = Path::new(&flintc_path)
        .parent()
        .ok_or_else(|| anyhow!("cannot locate the Flint source tree from {flintc_path}"))?;
    let runtime_dir = flint_root.join("runtime");
    for source in ["runtime.c", "flint_aegis.c"] {
        let path = runtime_dir.join(source);
        if !path.is_file() {
            bail!("Flint runtime source missing: {}", path.display());
        }
    }

    fs::create_dir_all(context.work_dir)
        .with_context(|| format!("cannot create {}", context.work_dir.display()))?;

    let main = flint_main(context)?;
    let object = context.work_dir.join("main.o");
    let mut emit = CommandSpec::new(PathBuf::from(&flintc_path), context.root);
    emit.arg(main.display().to_string());
    emit.arg("--target");
    emit.arg(context.target.flint_triple);
    emit.arg("-o");
    emit.arg(object.display().to_string());
    emit.arg("--no-run");
    emit.timeout(context.timeout);

    let emit_log = context.work_dir.join("emit.log");
    let emit_output = exec::run(&emit, &emit_log)?;
    if !emit_output.success {
        bail!(
            "flintc cross-emit failed{}; see {}",
            if emit_output.timed_out {
                " (timed out)"
            } else {
                ""
            },
            emit_log.display()
        );
    }
    if !object.is_file() {
        bail!(
            "flintc reported success but no object exists at {}",
            object.display()
        );
    }
    let mut commands = vec![emit_output];
    let mut runtime_objects = Vec::new();

    for source in ["runtime.c", "flint_aegis.c"] {
        let source_path = runtime_dir.join(source);
        let object_name = if source == "runtime.c" {
            "runtime.o"
        } else {
            "flint_aegis.o"
        };
        let object_path = context.work_dir.join(object_name);

        let mut compile = CommandSpec::new(PathBuf::from(&zig_path), context.root);
        compile.arg("cc");
        compile.arg("-target");
        compile.arg(context.target.zig_triple);
        compile.arg("-O2");
        compile.arg("-fno-pie");
        compile.arg("-include");
        compile.arg("stddef.h");
        compile.arg("-c");
        compile.arg("-I");
        compile.arg(runtime_dir.display().to_string());
        compile.arg(source_path.display().to_string());
        compile.arg("-o");
        compile.arg(object_path.display().to_string());
        compile.timeout(context.timeout);

        let log_path = context.work_dir.join(format!("compile-{object_name}.log"));
        let output = exec::run(&compile, &log_path)?;
        if !output.success {
            bail!(
                "zig cc failed compiling Flint runtime `{source}`; see {}",
                log_path.display()
            );
        }
        commands.push(output);
        runtime_objects.push(object_path);
    }

    let artifact = context.work_dir.join(&context.project.name);
    let mut link = CommandSpec::new(PathBuf::from(&zig_path), context.root);
    link.arg("cc");
    link.arg("-target");
    link.arg(context.target.zig_triple);
    if context.target.abi == "musl" {
        link.arg("-static");
    }
    link.arg("-no-pie");
    link.arg(object.display().to_string());
    for runtime_object in &runtime_objects {
        link.arg(runtime_object.display().to_string());
    }
    link.arg("-o");
    link.arg(artifact.display().to_string());
    link.arg("-lm");
    link.arg("-lpthread");
    link.timeout(context.timeout);

    let link_log = context.work_dir.join("link.log");
    let link_output = exec::run(&link, &link_log)?;
    if !link_output.success {
        bail!(
            "zig cc failed linking the Flint artifact{}; see {}",
            if link_output.timed_out {
                " (timed out)"
            } else {
                ""
            },
            link_log.display()
        );
    }
    commands.push(link_output);
    if !artifact.is_file() {
        bail!(
            "zig cc reported success but no artifact exists at {}",
            artifact.display()
        );
    }

    let mut versions = BTreeMap::new();
    if let Some(version) = toolchain::version_of(&flintc_path, &["--version"]) {
        versions.insert("flintc".to_string(), version);
    }
    if let Some(version) = toolchain::version_of(&zig_path, &["version"]) {
        versions.insert("zig".to_string(), version);
    }

    Ok(BuildResult {
        artifact,
        commands,
        toolchain: versions,
    })
}

fn build_flint_wasi(context: &BuildContext<'_>) -> Result<BuildResult> {
    let flintc = toolchain::check_tool(
        "flintc",
        Some("FLINTC"),
        Some("set FLINTC or add flintc to PATH"),
    );
    let Some(flintc_path) = flintc.path else {
        bail!("flintc not found; set FLINTC or add flintc to PATH");
    };
    let zig = toolchain::check_tool("zig", None, Some("pkg install zig"));
    let Some(zig_path) = zig.path else {
        bail!("zig not found; Flint WASI needs zig");
    };

    let flint_root = Path::new(&flintc_path)
        .parent()
        .ok_or_else(|| anyhow!("cannot locate the Flint source tree from {flintc_path}"))?;
    let runtime_dir = flint_root.join("runtime");
    for source in ["runtime.c", "flint_aegis.c"] {
        let path = runtime_dir.join(source);
        if !path.is_file() {
            bail!("Flint runtime source missing: {}", path.display());
        }
    }

    fs::create_dir_all(context.work_dir)
        .with_context(|| format!("cannot create {}", context.work_dir.display()))?;

    let main = flint_main(context)?;
    let ir = context.work_dir.join("main.wasi.ll");
    let mut emit = CommandSpec::new(PathBuf::from(&flintc_path), context.root);
    emit.arg(main.display().to_string());
    emit.arg("--target");
    emit.arg(context.target.flint_triple);
    emit.arg("--emit-llvm");
    emit.arg("-o");
    emit.arg(ir.display().to_string());
    emit.arg("--no-run");
    emit.timeout(context.timeout);
    let emit_log = context.work_dir.join("emit.log");
    let emit_output = exec::run(&emit, &emit_log)?;
    if !emit_output.success {
        bail!(
            "flintc WASI IR emission failed{}; see {}",
            if emit_output.timed_out {
                " (timed out)"
            } else {
                ""
            },
            emit_log.display()
        );
    }

    let ir_text = fs::read_to_string(&ir)
        .with_context(|| format!("cannot read emitted IR {}", ir.display()))?;
    let ir_text = sanitize_flint_wasi_ir(&ir_text);
    fs::write(&ir, ir_text).with_context(|| format!("cannot rewrite {}", ir.display()))?;

    let shim_header = context.work_dir.join("flint_wasi_shim.h");
    let shim_source = context.work_dir.join("flint_wasi_shim.c");
    let entry_source = context.work_dir.join("flint_wasi_entry.c");
    fs::write(&shim_header, FLINT_WASI_SHIM_H)?;
    fs::write(&shim_source, FLINT_WASI_SHIM_C)?;
    fs::write(&entry_source, FLINT_WASI_ENTRY_C)?;

    let mut commands = vec![emit_output];
    let main_object = context.work_dir.join("main.wasi.o");
    let entry_object = context.work_dir.join("flint_wasi_entry.o");
    let mut runtime_objects = Vec::new();

    let mut emit_object = CommandSpec::new(PathBuf::from(&zig_path), context.root);
    emit_object.arg("cc");
    emit_object.arg("-target");
    emit_object.arg(context.target.zig_triple);
    emit_object.arg("-O2");
    emit_object.arg("-c");
    emit_object.arg(ir.display().to_string());
    emit_object.arg("-o");
    emit_object.arg(main_object.display().to_string());
    emit_object.timeout(context.timeout);
    let log = context.work_dir.join("compile-main.log");
    let output = exec::run(&emit_object, &log)?;
    if !output.success {
        bail!("zig cc failed compiling Flint IR; see {}", log.display());
    }
    commands.push(output);

    let mut compile_entry = CommandSpec::new(PathBuf::from(&zig_path), context.root);
    compile_entry.arg("cc");
    compile_entry.arg("-target");
    compile_entry.arg(context.target.zig_triple);
    compile_entry.arg("-O2");
    compile_entry.arg("-c");
    compile_entry.arg(entry_source.display().to_string());
    compile_entry.arg("-o");
    compile_entry.arg(entry_object.display().to_string());
    compile_entry.timeout(context.timeout);
    let log = context.work_dir.join("compile-entry.log");
    let output = exec::run(&compile_entry, &log)?;
    if !output.success {
        bail!(
            "zig cc failed compiling Flint WASI entry; see {}",
            log.display()
        );
    }
    commands.push(output);

    for (source, name) in [
        (runtime_dir.join("runtime.c"), "runtime.wasi.o"),
        (runtime_dir.join("flint_aegis.c"), "flint_aegis.wasi.o"),
        (shim_source.clone(), "flint_wasi_shim.o"),
    ] {
        let object = context.work_dir.join(name);
        let mut compile = CommandSpec::new(PathBuf::from(&zig_path), context.root);
        compile.arg("cc");
        compile.arg("-target");
        compile.arg(context.target.zig_triple);
        compile.arg("-O2");
        compile.arg("-fno-pie");
        compile.arg("-D_WASI_EMULATED_SIGNAL");
        compile.arg("-D_WASI_EMULATED_PROCESS_CLOCKS");
        if name != "flint_wasi_shim.o" {
            compile.arg("-include");
            compile.arg(shim_header.display().to_string());
            compile.arg("-include");
            compile.arg("stddef.h");
            compile.arg("-I");
            compile.arg(runtime_dir.display().to_string());
        }
        compile.arg("-c");
        compile.arg(source.display().to_string());
        compile.arg("-o");
        compile.arg(object.display().to_string());
        compile.timeout(context.timeout);
        let log = context.work_dir.join(format!("compile-{name}.log"));
        let output = exec::run(&compile, &log)?;
        if !output.success {
            bail!(
                "zig cc failed compiling `{}` for WASI; see {}",
                source.display(),
                log.display()
            );
        }
        commands.push(output);
        runtime_objects.push(object);
    }

    let artifact = context
        .work_dir
        .join(format!("{}.wasm", context.project.name));
    let mut link = CommandSpec::new(PathBuf::from(&zig_path), context.root);
    link.arg("cc");
    link.arg("-target");
    link.arg(context.target.zig_triple);
    link.arg("-O2");
    link.arg(entry_object.display().to_string());
    link.arg(main_object.display().to_string());
    for object in &runtime_objects {
        link.arg(object.display().to_string());
    }
    link.arg("-o");
    link.arg(artifact.display().to_string());
    link.arg("-lm");
    link.arg("-lwasi-emulated-signal");
    link.arg("-lwasi-emulated-process-clocks");
    link.timeout(context.timeout);
    let log = context.work_dir.join("link.log");
    let output = exec::run(&link, &log)?;
    if !output.success {
        bail!(
            "zig cc failed linking Flint WASI{}; see {}",
            if output.timed_out { " (timed out)" } else { "" },
            log.display()
        );
    }
    commands.push(output);
    if !artifact.is_file() {
        bail!(
            "zig cc reported success but no artifact exists at {}",
            artifact.display()
        );
    }

    let mut versions = BTreeMap::new();
    if let Some(version) = toolchain::version_of(&flintc_path, &["--version"]) {
        versions.insert("flintc".to_string(), version);
    }
    if let Some(version) = toolchain::version_of(&zig_path, &["version"]) {
        versions.insert("zig".to_string(), version);
    }

    Ok(BuildResult {
        artifact,
        commands,
        toolchain: versions,
    })
}

fn sanitize_flint_wasi_ir(ir: &str) -> String {
    ir.replace("@main", "@flint_entry")
        .replace(",-wide-arithmetic,\" }", "-wide-arithmetic\" }")
}

const FLINT_WASI_SHIM_H: &str = r#"#ifndef FLINT_WASI_SHIM_H
#define FLINT_WASI_SHIM_H
#include <stddef.h>
#include <stdio.h>
typedef struct { int __flint_wasi_dummy; } siginfo_t;
FILE *flint_wasi_popen(const char *cmd, const char *mode);
int flint_wasi_pclose(FILE *pipe);
int flint_wasi_system(const char *cmd);
#define popen flint_wasi_popen
#define pclose flint_wasi_pclose
#define system flint_wasi_system
#endif
"#;

const FLINT_WASI_SHIM_C: &str = r#"#include <errno.h>
#include <stdio.h>
FILE *flint_wasi_popen(const char *cmd, const char *mode) {
    (void)cmd; (void)mode; errno = ENOSYS; return NULL;
}
int flint_wasi_pclose(FILE *pipe) {
    (void)pipe; errno = ENOSYS; return -1;
}
int flint_wasi_system(const char *cmd) {
    (void)cmd; errno = ENOSYS; return -1;
}
"#;

const FLINT_WASI_ENTRY_C: &str = r#"#include <stdint.h>
extern int32_t flint_entry(int64_t argc, const void *argv) __asm__("flint_entry");
int main(void) { return flint_entry(0, (const void *)0); }
"#;

fn flint_main(context: &BuildContext<'_>) -> Result<PathBuf> {
    if let Some(manifest) = &context.project.manifest {
        let candidate = context.source.join(manifest);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    let main = context.root.join("main.fl");
    if main.is_file() {
        return Ok(main);
    }

    for entry in fs::read_dir(context.root)
        .with_context(|| format!("cannot read {}", context.root.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("fl") {
            return Ok(path);
        }
    }

    Err(anyhow!(
        "no Flint source file found under {}",
        context.root.display()
    ))
}

fn cargo_linker_env(triple: &str) -> String {
    format!(
        "CARGO_TARGET_{}_LINKER",
        triple.replace(['-', '.'], "_").to_uppercase()
    )
}

fn write_cross_linker(context: &BuildContext<'_>) -> Result<String> {
    let zig = toolchain::check_tool("zig", None, Some("pkg install zig"));
    let Some(program) = zig.path else {
        bail!("zig not found; install zig for Rust cross-linking");
    };

    let filters = if context.target.os == "windows" {
        "    -Wl,--fix-cortex-a53-843419|-nodefaultlibs|-Wl,-Bdynamic|-Wl,-Bstatic|-l:libpthread.a|-lmsvcrt|-lmingwex|-lmingw32|-lgcc) ;;\n    -lgcc_eh) set -- \"$@\" \"-lunwind\" ;;\n"
    } else {
        "    -Wl,--fix-cortex-a53-843419) ;;\n"
    };
    let link = context.work_dir.join("cargo-linker.sh");
    let mut body = format!(
        "#!/bin/sh\nn=$#\ni=0\nwhile [ \"$i\" -lt \"$n\" ]; do\n  arg=$1\n  shift\n  case \"$arg\" in\n{filters}    *) set -- \"$@\" \"$arg\" ;;\n  esac\n  i=$((i+1))\ndone\nexec "
    );
    body.push_str(&shell_quote(&program));
    for arg in ["cc", "-target", context.target.zig_triple] {
        body.push(' ');
        body.push_str(&shell_quote(arg));
    }
    body.push_str(" \"$@\"\n");
    fs::write(&link, body)
        .with_context(|| format!("cannot write linker wrapper {}", link.display()))?;
    make_executable(&link)?;
    Ok(link.display().to_string())
}

fn sign_macos_artifact(context: &BuildContext<'_>, artifact: &Path) -> Result<CommandOutput> {
    let artifact = artifact.to_string_lossy().to_string();
    let rcodesign = toolchain::check_tool("rcodesign", None, Some("cargo install apple-codesign"));
    let (program, args, log_name) = if let Some(program) = rcodesign.path {
        (program, vec!["sign".to_string(), artifact], "rcodesign.log")
    } else if std::env::consts::OS == "macos" {
        let codesign = toolchain::check_tool("codesign", None, None);
        let Some(program) = codesign.path else {
            bail!("macOS artifact signing needs rcodesign or codesign");
        };
        (
            program,
            vec![
                "--force".to_string(),
                "--sign".to_string(),
                "-".to_string(),
                artifact,
            ],
            "codesign.log",
        )
    } else {
        bail!(
            "macOS artifacts need ad-hoc signing; install rcodesign with `cargo install apple-codesign`"
        );
    };

    let mut spec = CommandSpec::new(program, context.root);
    for arg in args {
        spec.arg(arg);
    }
    spec.timeout(context.timeout);
    let log_path = context.work_dir.join(log_name);
    let command = exec::run(&spec, &log_path)?;
    if !command.success {
        bail!(
            "macOS ad-hoc signing failed{}; see {}",
            if command.timed_out {
                " (timed out)"
            } else {
                ""
            },
            log_path.display()
        );
    }
    Ok(command)
}

fn write_windows_binutils(context: &BuildContext<'_>) -> Result<Option<String>> {
    if context.target.arch != "x86_64" {
        return Ok(None);
    }
    let Some(dlltool) = toolchain::check_tool("llvm-dlltool", None, None)
        .path
        .or_else(|| toolchain::check_tool("dlltool", None, None).path)
    else {
        return Ok(None);
    };
    let Some(ar) = toolchain::check_tool("llvm-ar", None, None)
        .path
        .or_else(|| toolchain::check_tool("ar", None, None).path)
    else {
        return Ok(None);
    };
    let prefix = "x86_64-w64-mingw32";
    for (name, program) in [
        (format!("{prefix}-dlltool"), &dlltool),
        (format!("{prefix}-ar"), &ar),
    ] {
        let path = context.work_dir.join(name);
        let mut body = String::from("#!/bin/sh\nexec ");
        body.push_str(&shell_quote(program));
        body.push_str(" \"$@\"\n");
        fs::write(&path, body)
            .with_context(|| format!("cannot write Windows cross-tool {}", path.display()))?;
        make_executable(&path)?;
    }
    let path = fs::canonicalize(context.work_dir)
        .with_context(|| format!("cannot resolve {}", context.work_dir.display()))?;
    Ok(Some(path.display().to_string()))
}

fn find_wasm_artifact(dir: &Path, preferred: &str) -> Result<PathBuf> {
    let preferred_path = dir.join(format!("{preferred}.wasm"));
    if preferred_path.is_file() {
        return Ok(preferred_path);
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("wasm") {
            candidates.push(path);
        }
    }
    candidates.sort();
    candidates.into_iter().next().ok_or_else(|| {
        anyhow!(
            "no WASM artifact named `{preferred}.wasm` found in {}",
            dir.display()
        )
    })
}

fn find_executable(dir: &Path, preferred: &str) -> Result<PathBuf> {
    let preferred_path = dir.join(preferred);
    if preferred_path.is_file() {
        return Ok(preferred_path);
    }
    let preferred_exe = dir.join(format!("{preferred}.exe"));
    if preferred_exe.is_file() {
        return Ok(preferred_exe);
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name.ends_with(".d") || name.ends_with(".rlib") || name.ends_with(".rmeta") {
            continue;
        }
        if name.ends_with(".exe") {
            candidates.push(path);
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if entry.metadata()?.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        candidates.push(path);
    }
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no executable artifact found in {}", dir.display()))
}
