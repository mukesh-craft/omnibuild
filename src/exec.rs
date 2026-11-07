use anyhow::{Context, Result};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            env: Vec::new(),
            timeout: Duration::from_secs(1800),
        }
    }

    pub fn arg(&mut self, arg: impl Into<String>) -> &mut Self {
        self.args.push(arg.into());
        self
    }

    pub fn env(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn timeout(&mut self, timeout: Duration) -> &mut Self {
        self.timeout = timeout;
        self
    }
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub timed_out: bool,
    pub duration_ms: u128,
    pub log: String,
}

pub fn run(spec: &CommandSpec, log_path: &Path) -> Result<CommandOutput> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create log directory {}", parent.display()))?;
    }

    let file = File::create(log_path)
        .with_context(|| format!("cannot create log file {}", log_path.display()))?;
    let error_file = file
        .try_clone()
        .with_context(|| format!("cannot clone log file {}", log_path.display()))?;

    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(error_file));
    for (key, value) in &spec.env {
        command.env(key, value);
    }

    let started = Instant::now();
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn {}", spec.program.display()))?;

    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().context("failed to poll child process")? {
            break status;
        }
        if started.elapsed() > spec.timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().context("failed to wait for timed-out child")?;
        }
        thread::sleep(Duration::from_millis(50));
    };

    let duration_ms = started.elapsed().as_millis();

    Ok(CommandOutput {
        program: spec.program.display().to_string(),
        args: spec.args.clone(),
        cwd: spec.cwd.display().to_string(),
        exit_code: status.code(),
        success: status.success() && !timed_out,
        timed_out,
        duration_ms,
        log: log_path.display().to_string(),
    })
}
