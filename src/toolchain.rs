use serde::Serialize;
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Serialize)]
pub struct ToolCheck {
    pub tool: String,
    pub available: bool,
    pub path: Option<String>,
    pub hint: Option<String>,
}

pub fn check_tool(tool: &str, env_var: Option<&str>, hint: Option<&str>) -> ToolCheck {
    if let Some(var) = env_var
        && let Some(value) = env::var_os(var)
        && !value.is_empty()
        && Path::new(&value).exists()
    {
        return available(tool, PathBuf::from(&value));
    }

    match which::which(tool) {
        Ok(path) => available(tool, path),
        Err(_) => ToolCheck {
            tool: tool.to_string(),
            available: false,
            path: None,
            hint: hint.map(str::to_string),
        },
    }
}

fn available(tool: &str, path: PathBuf) -> ToolCheck {
    ToolCheck {
        tool: tool.to_string(),
        available: true,
        path: Some(path.display().to_string()),
        hint: None,
    }
}

pub fn rust_sysroot() -> Option<String> {
    let output = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn rust_std_installed(triple: &str) -> bool {
    let Some(sysroot) = rust_sysroot() else {
        return false;
    };
    let lib = Path::new(&sysroot)
        .join("lib/rustlib")
        .join(triple)
        .join("lib");
    let Ok(entries) = std::fs::read_dir(lib) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name.starts_with("libstd-") && name.ends_with(".rlib")
    })
}

pub fn rust_src_installed() -> bool {
    rust_sysroot()
        .map(|sysroot| {
            Path::new(&sysroot)
                .join("lib/rustlib/src/rust/library/std")
                .is_dir()
        })
        .unwrap_or(false)
}

pub fn version_of(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
}
