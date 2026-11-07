use crate::target::TargetSpec;
use anyhow::{Context, Result};
use object::{Architecture, BinaryFormat, Object, ObjectKind};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, fs, path::Path};

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactInfo {
    pub path: String,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
    pub format: String,
    pub architecture: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerificationCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Inspection {
    pub artifact: ArtifactInfo,
    pub checks: Vec<VerificationCheck>,
}

impl Inspection {
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|check| check.passed)
    }
}

pub fn inspect(path: &Path, target: &TargetSpec) -> Result<Inspection> {
    let data =
        fs::read(path).with_context(|| format!("cannot read artifact {}", path.display()))?;
    let metadata =
        fs::metadata(path).with_context(|| format!("cannot stat artifact {}", path.display()))?;

    let mut checks = vec![VerificationCheck {
        name: "file exists".to_string(),
        passed: path.is_file(),
        detail: path.display().to_string(),
    }];

    let mut artifact = ArtifactInfo {
        path: path.display().to_string(),
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string(),
        size: metadata.len(),
        sha256: sha256_hex(&data),
        format: "unknown".to_string(),
        architecture: "unknown".to_string(),
        kind: "unknown".to_string(),
    };

    checks.push(VerificationCheck {
        name: "artifact is not empty".to_string(),
        passed: metadata.len() > 0,
        detail: format!("{} bytes", metadata.len()),
    });

    match object::File::parse(&*data) {
        Ok(object) => {
            artifact.format = format_name(object.format()).to_string();
            artifact.architecture = architecture_name(object.architecture()).to_string();
            artifact.kind = kind_name(object.kind()).to_string();

            checks.push(VerificationCheck {
                name: "binary format".to_string(),
                passed: format_matches(target.binary_format, &artifact.format),
                detail: format!("expected {}, got {}", target.binary_format, artifact.format),
            });
            checks.push(VerificationCheck {
                name: "architecture".to_string(),
                passed: target.arch == artifact.architecture,
                detail: format!("expected {}, got {}", target.arch, artifact.architecture),
            });
            checks.push(VerificationCheck {
                name: "executable kind".to_string(),
                passed: matches!(object.kind(), ObjectKind::Executable | ObjectKind::Dynamic),
                detail: artifact.kind.clone(),
            });
        }
        Err(error) => {
            checks.push(VerificationCheck {
                name: "parse object file".to_string(),
                passed: false,
                detail: error.to_string(),
            });
        }
    }

    Ok(Inspection { artifact, checks })
}

fn format_matches(expected: &str, actual: &str) -> bool {
    match expected {
        "PE/COFF" => actual == "PE" || actual == "COFF",
        other => other == actual,
    }
}

fn format_name(format: BinaryFormat) -> &'static str {
    match format {
        BinaryFormat::Elf => "ELF",
        BinaryFormat::Coff => "COFF",
        BinaryFormat::Pe => "PE",
        BinaryFormat::MachO => "Mach-O",
        BinaryFormat::Wasm => "WebAssembly",
        _ => "other",
    }
}

fn architecture_name(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::Aarch64 => "aarch64",
        Architecture::X86_64 => "x86_64",
        Architecture::Wasm32 => "wasm32",
        Architecture::Riscv64 => "riscv64",
        _ => "other",
    }
}

fn kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Relocatable => "relocatable",
        ObjectKind::Executable => "executable",
        ObjectKind::Dynamic => "dynamic",
        _ => "other",
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let data =
        fs::read(path).with_context(|| format!("cannot read {} for hashing", path.display()))?;
    Ok(sha256_hex(&data))
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target;

    #[test]
    fn rejects_a_text_file() {
        let base = std::env::var_os("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp")
            });
        fs::create_dir_all(&base).unwrap();
        let path = base.join(format!("not-an-artifact-{}", std::process::id()));
        fs::write(&path, "hello").unwrap();
        let target = target::by_id("android-arm64").unwrap();
        let inspection = inspect(&path, target).unwrap();
        assert!(!inspection.passed());
        assert_eq!(inspection.artifact.format, "unknown");
        assert!(!inspection.artifact.sha256.is_empty());
    }

    #[test]
    fn hashes_are_stable() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
