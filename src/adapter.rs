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
        if context.target.id != target::host_id() {
            bail!(
                "Cargo cross-build for `{}` is not implemented; Rust cross is out of 0.1",
                context.target.id
            );
        }

        let cargo = toolchain::check_tool("cargo", None, None);
        let Some(cargo_path) = cargo.path else {
            bail!("cargo not found; install a Rust toolchain");
        };

        fs::create_dir_all(context.work_dir)
            .with_context(|| format!("cannot create {}", context.work_dir.display()))?;

        let release = context.mode != "debug";
        let mut spec = CommandSpec::new(PathBuf::from(cargo_path), context.root);
        spec.arg("build");
        if release {
            spec.arg("--release");
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
        let profile_dir = context.root.join("target").join(profile);
        let artifact = find_executable(&profile_dir, &context.project.name)?;
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

        let mut versions = BTreeMap::new();
        if let Some(version) = toolchain::version_of("cargo", &["--version"]) {
            versions.insert("cargo".to_string(), version);
        }
        if let Some(version) = toolchain::version_of("rustc", &["--version"]) {
            versions.insert("rustc".to_string(), version);
        }

        Ok(BuildResult {
            artifact: destination,
            commands: vec![command],
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
            if !matches!(context.target.os, "linux" | "windows") {
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
    let names = [name.to_string(), format!("{name}.exe")];
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
        if context.target.id == target::host_id() {
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

fn find_executable(dir: &Path, preferred: &str) -> Result<PathBuf> {
    let preferred_path = dir.join(preferred);
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
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name.ends_with(".d") {
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
