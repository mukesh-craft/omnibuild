use crate::{target, toolchain};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub host: Option<String>,
    pub host_llvm_triple: Option<String>,
    pub rust_sysroot: Option<String>,
    pub tools: Vec<toolchain::ToolCheck>,
    pub notes: Vec<String>,
}

pub fn run() -> DoctorReport {
    let host = target::host();
    let checks: &[(&str, Option<&str>, Option<&str>)] = &[
        ("rustc", None, None),
        ("cargo", None, None),
        ("clang", None, None),
        ("clang++", None, None),
        ("cmake", None, Some("pkg install cmake")),
        ("ninja", None, Some("pkg install ninja")),
        ("make", None, None),
        ("zig", None, Some("pkg install zig")),
        (
            "flintc",
            Some("FLINTC"),
            Some("set FLINTC or add flintc to PATH"),
        ),
        ("qemu-x86_64", None, Some("pkg install qemu-user-x86-64")),
        ("tar", None, Some("pkg install tar")),
        ("zip", None, Some("pkg install zip")),
        ("ld.lld", None, None),
        ("wasm-ld", None, None),
        ("go", None, Some("pkg install golang")),
        ("node", None, None),
        ("git", None, None),
    ];

    let tools: Vec<toolchain::ToolCheck> = checks
        .iter()
        .map(|(tool, env_var, hint)| toolchain::check_tool(tool, *env_var, *hint))
        .collect();

    let mut notes = Vec::new();
    if host.is_none() {
        notes.push(
            "host triple is not in the target registry; pass --target explicitly".to_string(),
        );
    }
    if std::env::var_os("TMPDIR").is_none() && !Path::new("/tmp").is_dir() {
        notes.push(
            "TMPDIR is unset and /tmp does not exist; use an explicit writable temp directory"
                .to_string(),
        );
    }
    if toolchain::rust_sysroot().is_none() {
        notes.push("rustc --print sysroot failed; Rust target checks are unavailable".to_string());
    }
    let has_tool = |name: &str| tools.iter().any(|tool| tool.tool == name && tool.available);
    if has_tool("zig") && has_tool("qemu-x86_64") {
        notes.push(
            "Zig glibc dynamic binaries do not run under Termux qemu-x86_64; use \
             `linux-x86_64-musl` for executed cross-target verification"
                .to_string(),
        );
    }

    DoctorReport {
        host: host.map(|target| target.id.to_string()),
        host_llvm_triple: host.map(|target| target.llvm_triple.to_string()),
        rust_sysroot: toolchain::rust_sysroot(),
        tools,
        notes,
    }
}
