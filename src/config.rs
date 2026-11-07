use crate::target;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub project: ProjectConfig,
    pub build: BuildConfig,
    pub targets: Vec<String>,
    pub toolchains: ToolchainConfig,
    pub workers: WorkerConfig,
    pub verification: VerificationConfig,
    pub cache: CacheConfig,
    pub artifacts: ArtifactConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildConfig {
    pub system: String,
    pub mode: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolchainConfig {
    pub strategy: String,
    pub pin_versions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkerConfig {
    pub strategy: String,
    pub allow_remote: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VerificationConfig {
    pub inspect_binary: bool,
    pub check_dependencies: bool,
    pub run_tests: bool,
    pub require_target_execution: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheConfig {
    pub enabled: bool,
    pub directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtifactConfig {
    pub directory: String,
    pub generate_manifest: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            project: ProjectConfig::default(),
            build: BuildConfig::default(),
            targets: Vec::new(),
            toolchains: ToolchainConfig::default(),
            workers: WorkerConfig::default(),
            verification: VerificationConfig::default(),
            cache: CacheConfig::default(),
            artifacts: ArtifactConfig::default(),
        }
    }
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: "auto".to_string(),
            source: ".".to_string(),
        }
    }
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            system: "auto".to_string(),
            mode: "release".to_string(),
            timeout_secs: 1800,
        }
    }
}

impl Default for ToolchainConfig {
    fn default() -> Self {
        Self {
            strategy: "auto".to_string(),
            pin_versions: true,
        }
    }
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            strategy: "auto".to_string(),
            allow_remote: true,
        }
    }
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            inspect_binary: true,
            check_dependencies: true,
            run_tests: true,
            require_target_execution: false,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            directory: ".omnibuild-cache".to_string(),
        }
    }
}

impl Default for ArtifactConfig {
    fn default() -> Self {
        Self {
            directory: "dist/".to_string(),
            generate_manifest: true,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read configuration {}", path.display()))?;
        let config: Config = serde_yaml_ng::from_str(&text)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!(
                "unsupported configuration version {}; this build understands version 1",
                self.version
            );
        }
        for id in &self.targets {
            if target::by_id(id).is_none() {
                bail!("unknown target `{id}`; run `omnibuild targets` for the list");
            }
        }
        if self.cache.enabled && self.cache.directory.trim().is_empty() {
            bail!("cache.directory must not be empty when caching is enabled");
        }
        if self.artifacts.directory.trim().is_empty() {
            bail!("artifacts.directory must not be empty");
        }
        Ok(())
    }

    pub fn sample(name: &str) -> String {
        format!(
            "version: 1\n\nproject:\n  name: {name}\n  source: .\n\nbuild:\n  system: auto\n  mode: release\n  timeout_secs: 1800\n\ntargets: []\n\ntoolchains:\n  strategy: auto\n  pin_versions: true\n\nworkers:\n  strategy: auto\n  allow_remote: true\n\nverification:\n  inspect_binary: true\n  check_dependencies: true\n  run_tests: true\n  require_target_execution: false\n\ncache:\n  enabled: true\n\nartifacts:\n  directory: dist/\n  generate_manifest: true\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().expect("default config");
    }

    #[test]
    fn sample_round_trips() {
        let text = Config::sample("demo");
        let config: Config = serde_yaml_ng::from_str(&text).expect("sample parses");
        config.validate().expect("sample validates");
        assert_eq!(config.project.name, "demo");
        assert_eq!(config.version, 1);
    }

    #[test]
    fn unknown_target_is_rejected() {
        let mut config = Config::default();
        config.targets.push("mips-unknown".to_string());
        assert!(config.validate().is_err());
    }
}
