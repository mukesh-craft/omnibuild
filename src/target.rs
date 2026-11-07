use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Tier1,
    Tier2,
    Tier3,
    Unsupported,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Tier1 => "tier1",
            Tier::Tier2 => "tier2",
            Tier::Tier3 => "tier3",
            Tier::Unsupported => "unsupported",
        }
    }

    pub fn is_supported(self) -> bool {
        !matches!(self, Tier::Unsupported)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TargetSpec {
    pub id: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub abi: &'static str,
    pub tier: Tier,
    pub llvm_triple: &'static str,
    pub rust_triple: &'static str,
    pub flint_triple: &'static str,
    pub zig_triple: &'static str,
    pub binary_format: &'static str,
    pub notes: &'static str,
}

pub const TARGETS: &[TargetSpec] = &[
    TargetSpec {
        id: "android-arm64",
        os: "android",
        arch: "aarch64",
        abi: "bionic",
        tier: Tier::Tier1,
        llvm_triple: "aarch64-unknown-linux-android24",
        rust_triple: "aarch64-linux-android",
        flint_triple: "aarch64-unknown-linux-android24",
        zig_triple: "aarch64-linux-android",
        binary_format: "ELF",
        notes: "Primary dev target on Termux; bionic ABI",
    },
    TargetSpec {
        id: "linux-x86_64",
        os: "linux",
        arch: "x86_64",
        abi: "gnu",
        tier: Tier::Tier1,
        llvm_triple: "x86_64-unknown-linux-gnu",
        rust_triple: "x86_64-unknown-linux-gnu",
        flint_triple: "x86_64-pc-linux-gnu",
        zig_triple: "x86_64-linux-gnu",
        binary_format: "ELF",
        notes: "glibc cross target; execution needs a compatible x86_64 sysroot",
    },
    TargetSpec {
        id: "linux-x86_64-musl",
        os: "linux",
        arch: "x86_64",
        abi: "musl",
        tier: Tier::Tier1,
        llvm_triple: "x86_64-unknown-linux-musl",
        rust_triple: "x86_64-unknown-linux-musl",
        flint_triple: "x86_64-unknown-linux-musl",
        zig_triple: "x86_64-linux-musl",
        binary_format: "ELF",
        notes: "Static musl cross target; runtime-testable under qemu-x86_64",
    },
    TargetSpec {
        id: "linux-aarch64",
        os: "linux",
        arch: "aarch64",
        abi: "gnu",
        tier: Tier::Tier1,
        llvm_triple: "aarch64-unknown-linux-gnu",
        rust_triple: "aarch64-unknown-linux-gnu",
        flint_triple: "aarch64-unknown-linux-gnu",
        zig_triple: "aarch64-linux-gnu",
        binary_format: "ELF",
        notes: "glibc cross target; the Termux host itself is bionic",
    },
    TargetSpec {
        id: "windows-x86_64",
        os: "windows",
        arch: "x86_64",
        abi: "gnu",
        tier: Tier::Tier2,
        llvm_triple: "x86_64-w64-windows-gnu",
        rust_triple: "x86_64-pc-windows-gnu",
        flint_triple: "x86_64-windows-gnu",
        zig_triple: "x86_64-windows-gnu",
        binary_format: "PE/COFF",
        notes: "MinGW/COFF; inspect-only on this device",
    },
    TargetSpec {
        id: "macos-arm64",
        os: "macos",
        arch: "aarch64",
        abi: "darwin",
        tier: Tier::Tier2,
        llvm_triple: "aarch64-apple-darwin",
        rust_triple: "aarch64-apple-darwin",
        flint_triple: "aarch64-apple-darwin",
        zig_triple: "aarch64-macos",
        binary_format: "Mach-O",
        notes: "Cross-built with Zig and ad-hoc signed with rcodesign; run on macOS",
    },
    TargetSpec {
        id: "wasm32-wasi",
        os: "wasi",
        arch: "wasm32",
        abi: "wasi",
        tier: Tier::Tier3,
        llvm_triple: "wasm32-unknown-wasi",
        rust_triple: "wasm32-wasip1",
        flint_triple: "wasm32-unknown-wasi",
        zig_triple: "wasm32-wasi",
        binary_format: "WebAssembly",
        notes: "Cargo, CMake and Flint build WASI; wasmtime executes it",
    },
    TargetSpec {
        id: "linux-riscv64",
        os: "linux",
        arch: "riscv64",
        abi: "gnu",
        tier: Tier::Tier3,
        llvm_triple: "riscv64-unknown-linux-gnu",
        rust_triple: "riscv64gc-unknown-linux-gnu",
        flint_triple: "riscv64-unknown-linux-gnu",
        zig_triple: "riscv64-linux-gnu",
        binary_format: "ELF",
        notes: "Experimental; toolchain not provisioned",
    },
];

pub fn all() -> &'static [TargetSpec] {
    TARGETS
}

pub fn by_id(id: &str) -> Option<&'static TargetSpec> {
    TARGETS.iter().find(|target| target.id == id)
}

pub fn supported_ids(include_experimental: bool) -> Vec<&'static str> {
    TARGETS
        .iter()
        .filter(|target| target.tier.is_supported())
        .filter(|target| include_experimental || target.tier != Tier::Tier3)
        .map(|target| target.id)
        .collect()
}

pub fn host() -> Option<&'static TargetSpec> {
    let id = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("android", "aarch64") => "android-arm64",
        ("linux", "aarch64") => "linux-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("windows", "x86_64") => "windows-x86_64",
        ("macos", "aarch64") => "macos-arm64",
        _ => return None,
    };
    by_id(id)
}

pub fn host_id() -> &'static str {
    host().map(|target| target.id).unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique() {
        let mut ids = HashSet::new();
        for target in all() {
            assert!(ids.insert(target.id), "duplicate id {}", target.id);
        }
    }

    #[test]
    fn triples_are_present() {
        for target in all() {
            assert!(!target.llvm_triple.is_empty());
            assert!(!target.rust_triple.is_empty());
            assert!(!target.flint_triple.is_empty());
            assert!(!target.zig_triple.is_empty());
        }
    }

    #[test]
    fn known_target_parses() {
        let target = by_id("linux-x86_64").expect("known target");
        assert_eq!(target.llvm_triple, "x86_64-unknown-linux-gnu");
        assert_eq!(target.binary_format, "ELF");

        let musl = by_id("linux-x86_64-musl").expect("musl target");
        assert_eq!(musl.abi, "musl");
        assert_eq!(musl.zig_triple, "x86_64-linux-musl");
    }

    #[test]
    fn experimental_filter_works() {
        let stable = supported_ids(false);
        assert!(!stable.contains(&"wasm32-wasi"));
        assert!(stable.contains(&"macos-arm64"));
        let all_supported = supported_ids(true);
        assert!(all_supported.contains(&"wasm32-wasi"));
        assert!(all_supported.contains(&"macos-arm64"));
    }
}
