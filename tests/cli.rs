use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_omnibuild")
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .args(args)
        .output()
        .expect("run omnibuild")
}

#[test]
fn detect_reports_rust_project_as_json() {
    let path = fixture("rust-hello");
    let output = run(&[
        "detect",
        "--path",
        path.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(report["project_count"], 1);
    assert_eq!(report["projects"][0]["build_system"], "Cargo");
    assert_eq!(report["projects"][0]["name"], "rust-hello");
}

#[test]
fn targets_reports_the_registry() {
    let output = run(&["targets", "--format", "json"]);
    assert!(output.status.success());
    let targets: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let targets = targets.as_array().expect("array");
    assert!(targets.iter().any(|target| target["id"] == "linux-x86_64"));
    assert!(
        targets
            .iter()
            .any(|target| target["id"] == "linux-x86_64-musl")
    );
    assert!(targets.iter().any(|target| target["id"] == "macos-arm64"));
}

#[test]
fn plan_uses_the_host_target_when_none_is_configured() {
    let path = fixture("rust-hello");
    let output = run(&["plan", "--path", path.to_str().unwrap(), "--format", "json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(plan["schema_version"], 1);
    assert_eq!(plan["project"]["name"], "rust-hello");
    let targets = plan["targets"].as_array().expect("targets");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0]["readiness"]["status"], "ready");
}

#[test]
fn plan_rejects_unknown_targets() {
    let path = fixture("rust-hello");
    let output = run(&[
        "plan",
        "--path",
        path.to_str().unwrap(),
        "--target",
        "mips-unknown",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown target"));
}

#[test]
fn release_builds_and_executes_rust_fixture() {
    let dir = temp_project("release-rust");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::copy(fixture("rust-hello/Cargo.toml"), dir.join("Cargo.toml")).unwrap();
    fs::copy(fixture("rust-hello/src/main.rs"), dir.join("src/main.rs")).unwrap();

    let output = run(&[
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["adapter"], "cargo");
    assert_eq!(result["status"], "success");
    assert_eq!(result["level"], "executed");

    let artifact = result["artifact"]["path"].as_str().expect("artifact path");
    assert!(
        Path::new(artifact).is_file(),
        "artifact missing: {artifact}"
    );
    assert!(
        !result["artifact"]["sha256"]
            .as_str()
            .expect("sha256")
            .is_empty()
    );

    let manifest = report["release_manifest_path"]
        .as_str()
        .expect("release manifest path");
    assert!(
        Path::new(manifest).is_file(),
        "manifest missing: {manifest}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_builds_flint_fixture_when_flintc_is_available() {
    let flintc = std::env::var_os("FLINTC").filter(|value| !value.is_empty());
    if flintc.is_none() && which::which("flintc").is_err() {
        return;
    }

    let dir = temp_project("release-flint");
    fs::write(
        dir.join("main.fl"),
        include_str!("fixtures/flint-hello/main.fl"),
    )
    .unwrap();

    let mut command = Command::new(binary());
    command.args([
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--format",
        "json",
    ]);
    if let Some(flintc) = flintc {
        command.env("FLINTC", flintc);
    }

    let output = command.output().expect("run omnibuild");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["adapter"], "flint");
    assert_eq!(result["status"], "success");
    assert_eq!(result["level"], "executed");

    let artifact = result["artifact"]["path"].as_str().expect("artifact path");
    assert!(
        Path::new(artifact).is_file(),
        "artifact missing: {artifact}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_reuses_a_cached_artifact_on_the_second_run() {
    let dir = temp_project("release-cache");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::copy(fixture("rust-hello/Cargo.toml"), dir.join("Cargo.toml")).unwrap();
    fs::copy(fixture("rust-hello/Cargo.lock"), dir.join("Cargo.lock")).unwrap();
    fs::copy(fixture("rust-hello/src/main.rs"), dir.join("src/main.rs")).unwrap();

    let first = run(&[
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_report: Value = serde_json::from_slice(&first.stdout).expect("valid json");
    let first_result = &first_report["build"]["results"][0];
    assert_eq!(first_result["status"], "success");
    assert_eq!(first_result["cache"]["hit"], false);
    let first_hash = first_result["artifact"]["sha256"]
        .as_str()
        .expect("sha256")
        .to_string();

    let second = run(&[
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_report: Value = serde_json::from_slice(&second.stdout).expect("valid json");
    let second_result = &second_report["build"]["results"][0];
    assert_eq!(second_result["status"], "success");
    assert_eq!(second_result["level"], "executed");
    assert_eq!(second_result["cache"]["hit"], true);
    assert_eq!(second_result["commands"].as_array().unwrap().len(), 0);
    assert_eq!(second_result["artifact"]["sha256"], first_hash);
    assert_eq!(second_result["cache"]["key"], first_result["cache"]["key"]);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn package_writes_verified_tar_and_zip_archives() {
    if which::which("tar").is_err() {
        return;
    }

    let dir = temp_project("package-rust");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::copy(fixture("rust-hello/Cargo.toml"), dir.join("Cargo.toml")).unwrap();
    fs::copy(fixture("rust-hello/Cargo.lock"), dir.join("Cargo.lock")).unwrap();
    fs::copy(fixture("rust-hello/src/main.rs"), dir.join("src/main.rs")).unwrap();

    let output = run(&[
        "package",
        "--path",
        dir.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let archives = report["packages"]["archives"].as_array().expect("archives");
    assert!(!archives.is_empty(), "no archives produced");
    assert!(
        archives.iter().all(|archive| archive["sha256"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64)),
        "archive hash"
    );

    let tar_archive = archives
        .iter()
        .find(|archive| archive["format"] == "tar.gz")
        .expect("tar.gz archive");
    let tar_path = tar_archive["archive"].as_str().expect("tar path");
    assert!(Path::new(tar_path).is_file(), "archive missing: {tar_path}");
    let listing = Command::new("tar")
        .args(["-tzf", tar_path])
        .output()
        .expect("list archive");
    assert!(listing.status.success(), "tar listing failed");
    let listing = String::from_utf8_lossy(&listing.stdout);
    assert!(listing.contains("manifest.json"));
    assert!(listing.contains("release.json"));
    assert!(listing.contains("SHA256SUMS"));
    assert!(!listing.contains("artifact/"));
    assert!(
        archives.iter().all(|archive| {
            archive["archive"]
                .as_str()
                .and_then(|path| Path::new(path).file_name())
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("omnibuild-0.1.1-"))
        }),
        "archive naming should use version-target layout"
    );

    if which::which("zip").is_ok() {
        let zip_archive = archives
            .iter()
            .find(|archive| archive["format"] == "zip")
            .expect("zip archive");
        let zip_path = zip_archive["archive"].as_str().expect("zip path");
        assert!(Path::new(zip_path).is_file(), "archive missing: {zip_path}");
        assert!(
            zip_archive["sha256"]
                .as_str()
                .is_some_and(|hash| hash.len() == 64),
            "zip hash"
        );
        if which::which("zipinfo").is_ok() {
            let listing = Command::new("zipinfo")
                .args(["-1", zip_path])
                .output()
                .expect("list zip");
            assert!(listing.status.success(), "zip listing failed");
            let listing = String::from_utf8_lossy(&listing.stdout);
            assert!(listing.contains("manifest.json"));
            assert!(listing.contains("release.json"));
            assert!(listing.contains("SHA256SUMS"));
            assert!(!listing.contains("artifact/"));
        }
    }

    let package_manifest = report["package_manifest_path"]
        .as_str()
        .expect("package manifest path");
    assert!(
        Path::new(package_manifest).is_file(),
        "package manifest missing: {package_manifest}"
    );
    let sums = fs::read_to_string(Path::new(package_manifest).with_file_name("SHA256SUMS"))
        .expect("package sums");
    for archive in archives {
        let name = Path::new(archive["archive"].as_str().expect("archive"))
            .file_name()
            .and_then(|name| name.to_str())
            .expect("archive name");
        assert!(sums.contains(name), "SHA256SUMS missing {name}");
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_builds_and_executes_wasm_cargo_target_when_tools_are_available() {
    if which::which("cargo").is_err() || which::which("wasmtime").is_err() {
        return;
    }
    if !rust_std_available("wasm32-wasip1") {
        return;
    }

    let dir = temp_project("release-wasm-rust");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::copy(fixture("rust-hello/Cargo.toml"), dir.join("Cargo.toml")).unwrap();
    fs::copy(fixture("rust-hello/Cargo.lock"), dir.join("Cargo.lock")).unwrap();
    fs::copy(fixture("rust-hello/src/main.rs"), dir.join("src/main.rs")).unwrap();

    let output = run(&[
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--target",
        "wasm32-wasi",
        "--format",
        "json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["target"], "wasm32-wasi");
    assert_eq!(result["adapter"], "cargo");
    assert_eq!(result["status"], "success");
    assert_eq!(result["level"], "executed");
    assert_eq!(result["artifact"]["format"], "WebAssembly");
    assert_eq!(result["artifact"]["architecture"], "wasm32");
    assert_eq!(result["run"]["success"], true);
    let artifact = result["artifact"]["path"].as_str().expect("artifact path");
    assert!(
        artifact.ends_with(".wasm"),
        "unexpected artifact: {artifact}"
    );
    assert!(Path::new(artifact).is_file());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_builds_and_executes_rust_cross_target_when_tools_are_available() {
    for tool in ["cargo", "zig", "qemu-x86_64"] {
        if which::which(tool).is_err() {
            return;
        }
    }
    if !rust_src_available() {
        return;
    }

    let dir = temp_project("release-rust-cross");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::copy(fixture("rust-hello/Cargo.toml"), dir.join("Cargo.toml")).unwrap();
    fs::copy(fixture("rust-hello/Cargo.lock"), dir.join("Cargo.lock")).unwrap();
    fs::copy(fixture("rust-hello/src/main.rs"), dir.join("src/main.rs")).unwrap();

    let output = run(&[
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--target",
        "linux-x86_64-musl",
        "--format",
        "json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["target"], "linux-x86_64-musl");
    assert_eq!(result["adapter"], "cargo");
    assert_eq!(result["status"], "success");
    assert_eq!(result["level"], "executed");
    assert_eq!(result["artifact"]["format"], "ELF");
    assert_eq!(result["artifact"]["architecture"], "x86_64");
    assert_eq!(result["run"]["success"], true);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_builds_and_executes_wasm_cmake_target_when_tools_are_available() {
    for tool in ["cmake", "zig", "wasmtime"] {
        if which::which(tool).is_err() {
            return;
        }
    }

    let dir = temp_project("release-wasm-cmake");
    fs::copy(
        fixture("c-hello/CMakeLists.txt"),
        dir.join("CMakeLists.txt"),
    )
    .unwrap();
    fs::copy(fixture("c-hello/main.c"), dir.join("main.c")).unwrap();

    let output = run(&[
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--target",
        "wasm32-wasi",
        "--format",
        "json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["target"], "wasm32-wasi");
    assert_eq!(result["adapter"], "cmake");
    assert_eq!(result["status"], "success");
    assert_eq!(result["level"], "executed");
    assert_eq!(result["artifact"]["format"], "WebAssembly");
    assert_eq!(result["artifact"]["architecture"], "wasm32");
    assert_eq!(result["run"]["success"], true);
    let artifact = result["artifact"]["path"].as_str().expect("artifact path");
    assert!(
        artifact.ends_with(".wasm"),
        "unexpected artifact: {artifact}"
    );
    assert!(Path::new(artifact).is_file());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_builds_and_executes_cmake_cross_target_when_tools_are_available() {
    for tool in ["cmake", "zig", "qemu-x86_64"] {
        if which::which(tool).is_err() {
            return;
        }
    }

    let dir = temp_project("release-cmake-cross");
    fs::copy(
        fixture("c-hello/CMakeLists.txt"),
        dir.join("CMakeLists.txt"),
    )
    .unwrap();
    fs::copy(fixture("c-hello/main.c"), dir.join("main.c")).unwrap();

    let output = run(&[
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--target",
        "linux-x86_64-musl",
        "--format",
        "json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["target"], "linux-x86_64-musl");
    assert_eq!(result["adapter"], "cmake");
    assert_eq!(result["status"], "success");
    assert_eq!(result["level"], "executed");
    assert_eq!(result["artifact"]["format"], "ELF");
    assert_eq!(result["artifact"]["architecture"], "x86_64");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_inspects_windows_cmake_target_when_zig_is_available() {
    for tool in ["cmake", "zig"] {
        if which::which(tool).is_err() {
            return;
        }
    }

    let dir = temp_project("release-cmake-windows");
    fs::copy(
        fixture("c-hello/CMakeLists.txt"),
        dir.join("CMakeLists.txt"),
    )
    .unwrap();
    fs::copy(fixture("c-hello/main.c"), dir.join("main.c")).unwrap();

    let output = run(&[
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--target",
        "windows-x86_64",
        "--format",
        "json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["target"], "windows-x86_64");
    assert_eq!(result["status"], "success");
    assert_eq!(result["level"], "inspected");
    assert_eq!(result["artifact"]["format"], "PE");
    assert_eq!(result["artifact"]["architecture"], "x86_64");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_builds_and_executes_flint_cross_target_when_tools_are_available() {
    if which::which("zig").is_err() || which::which("qemu-x86_64").is_err() {
        return;
    }
    let Some(flintc) = flintc_path() else {
        return;
    };
    let runtime = flintc.parent().map(|root| root.join("runtime"));
    if !runtime
        .map(|runtime| runtime.join("runtime.c").is_file())
        .unwrap_or(false)
    {
        return;
    }

    let dir = temp_project("release-flint-cross");
    fs::write(
        dir.join("main.fl"),
        include_str!("fixtures/flint-hello/main.fl"),
    )
    .unwrap();

    let mut command = Command::new(binary());
    command.env("FLINTC", &flintc);
    command.args([
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--target",
        "linux-x86_64-musl",
        "--format",
        "json",
    ]);

    let output = command.output().expect("run omnibuild");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["target"], "linux-x86_64-musl");
    assert_eq!(result["adapter"], "flint");
    assert_eq!(result["status"], "success");
    assert_eq!(result["level"], "executed");
    assert_eq!(result["artifact"]["format"], "ELF");
    assert_eq!(result["artifact"]["architecture"], "x86_64");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_builds_flint_riscv_target_when_tools_are_available() {
    if which::which("zig").is_err() {
        return;
    }
    let Some(flintc) = flintc_path() else {
        return;
    };
    let runtime = flintc.parent().map(|root| root.join("runtime"));
    if !runtime
        .as_ref()
        .map(|runtime| runtime.join("runtime.c").is_file())
        .unwrap_or(false)
    {
        return;
    }

    let dir = temp_project("release-flint-riscv");
    fs::write(
        dir.join("main.fl"),
        include_str!("fixtures/flint-hello/main.fl"),
    )
    .unwrap();

    let mut command = Command::new(binary());
    command.env("FLINTC", &flintc);
    command.args([
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--target",
        "linux-riscv64",
        "--format",
        "json",
    ]);

    let output = command.output().expect("run omnibuild");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["target"], "linux-riscv64");
    assert_eq!(result["adapter"], "flint");
    assert_eq!(result["status"], "success");
    assert_eq!(result["level"], "inspected");
    assert_eq!(result["artifact"]["format"], "ELF");
    assert_eq!(result["artifact"]["architecture"], "riscv64");

    let artifact = result["artifact"]["path"].as_str().expect("artifact path");
    if let Ok(header) = Command::new("readelf").args(["-h", artifact]).output()
        && header.status.success()
    {
        let text = String::from_utf8_lossy(&header.stdout);
        assert!(
            text.contains("double-float ABI"),
            "Flint RISC-V artifact must use the lp64d ABI: {text}"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_builds_flint_windows_target_when_tools_are_available() {
    if which::which("zig").is_err() {
        return;
    }
    let Some(flintc) = flintc_path() else {
        return;
    };
    let runtime = flintc.parent().map(|root| root.join("runtime"));
    if !runtime
        .as_ref()
        .map(|runtime| runtime.join("runtime.c").is_file())
        .unwrap_or(false)
    {
        return;
    }

    let dir = temp_project("release-flint-windows");
    fs::write(
        dir.join("main.fl"),
        include_str!("fixtures/flint-hello/main.fl"),
    )
    .unwrap();

    let mut command = Command::new(binary());
    command.env("FLINTC", &flintc);
    command.args([
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--target",
        "windows-x86_64",
        "--format",
        "json",
    ]);

    let output = command.output().expect("run omnibuild");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["target"], "windows-x86_64");
    assert_eq!(result["adapter"], "flint");
    assert_eq!(result["status"], "success");
    assert_eq!(
        result["level"],
        if std::env::consts::OS == "windows" {
            "executed"
        } else {
            "inspected"
        }
    );
    assert_eq!(result["artifact"]["format"], "PE");
    assert_eq!(result["artifact"]["architecture"], "x86_64");
    let artifact = result["artifact"]["path"].as_str().expect("artifact path");
    assert!(
        artifact.ends_with(".exe"),
        "unexpected artifact: {artifact}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_builds_flint_macos_target_when_tools_are_available() {
    if which::which("zig").is_err()
        || (which::which("rcodesign").is_err() && which::which("codesign").is_err())
    {
        return;
    }
    let Some(flintc) = flintc_path() else {
        return;
    };
    let runtime = flintc.parent().map(|root| root.join("runtime"));
    if !runtime
        .as_ref()
        .map(|runtime| runtime.join("runtime.c").is_file())
        .unwrap_or(false)
    {
        return;
    }

    let dir = temp_project("release-flint-macos");
    fs::write(
        dir.join("main.fl"),
        include_str!("fixtures/flint-hello/main.fl"),
    )
    .unwrap();

    let mut command = Command::new(binary());
    command.env("FLINTC", &flintc);
    command.args([
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--target",
        "macos-arm64",
        "--format",
        "json",
    ]);

    let output = command.output().expect("run omnibuild");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["target"], "macos-arm64");
    assert_eq!(result["adapter"], "flint");
    assert_eq!(result["status"], "success");
    assert_eq!(
        result["level"],
        if std::env::consts::OS == "macos" && std::env::consts::ARCH == "aarch64" {
            "executed"
        } else {
            "inspected"
        }
    );
    assert_eq!(result["artifact"]["format"], "Mach-O");
    assert_eq!(result["artifact"]["architecture"], "aarch64");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_builds_and_executes_flint_wasi_target_when_tools_are_available() {
    if which::which("zig").is_err() || which::which("wasmtime").is_err() {
        return;
    }
    let Some(flintc) = flintc_path() else {
        return;
    };
    let runtime = flintc.parent().map(|root| root.join("runtime"));
    if !runtime
        .as_ref()
        .map(|runtime| runtime.join("runtime.c").is_file())
        .unwrap_or(false)
    {
        return;
    }

    let dir = temp_project("release-flint-wasi");
    fs::write(
        dir.join("main.fl"),
        include_str!("fixtures/flint-hello/main.fl"),
    )
    .unwrap();

    let mut command = Command::new(binary());
    command.env("FLINTC", &flintc);
    command.args([
        "release",
        "--path",
        dir.to_str().unwrap(),
        "--target",
        "wasm32-wasi",
        "--format",
        "json",
    ]);

    let output = command.output().expect("run omnibuild");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let result = &report["build"]["results"][0];
    assert_eq!(result["target"], "wasm32-wasi");
    assert_eq!(result["adapter"], "flint");
    assert_eq!(result["status"], "success");
    assert_eq!(result["level"], "executed");
    assert_eq!(result["artifact"]["format"], "WebAssembly");
    assert_eq!(result["artifact"]["architecture"], "wasm32");
    assert_eq!(result["run"]["success"], true);
    let artifact = result["artifact"]["path"].as_str().expect("artifact path");
    assert!(
        artifact.ends_with(".wasm"),
        "unexpected artifact: {artifact}"
    );

    let _ = fs::remove_dir_all(&dir);
}

fn flintc_path() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("FLINTC").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(value));
    }
    which::which("flintc").ok()
}

fn rust_src_available() -> bool {
    let Ok(output) = Command::new("rustc").args(["--print", "sysroot"]).output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let sysroot = String::from_utf8_lossy(&output.stdout);
    Path::new(sysroot.trim())
        .join("lib/rustlib/src/rust/library/std")
        .is_dir()
}

fn rust_std_available(triple: &str) -> bool {
    let Ok(output) = Command::new("rustc")
        .args(["--print", "target-libdir", "--target", triple])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let path = String::from_utf8_lossy(&output.stdout);
    Path::new(path.trim()).is_dir()
}

fn temp_project(tag: &str) -> PathBuf {
    let base = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp"));
    fs::create_dir_all(&base).expect("create test temp base");
    let dir = base.join(format!("{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test temp dir");
    dir
}
