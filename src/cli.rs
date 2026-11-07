use crate::{build, config::Config, detect, doctor, package, plan, target};
use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[command(
    name = "omnibuild",
    version,
    about = "Universal build orchestration and artifact verification",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create an omnibuild.yaml configuration file
    Init(InitArgs),
    /// Inspect a project and identify build systems
    Detect(DetectArgs),
    /// Show detected projects, dependencies and toolchain readiness
    Inspect(DetectArgs),
    /// List supported target combinations
    Targets(TargetsArgs),
    /// Preview the build plan for the selected targets
    Plan(PlanArgs),
    /// Build the selected targets and write artifacts plus manifests
    Build(BuildArgs),
    /// Build, verify and write release manifests
    Release(BuildArgs),
    /// Build, verify and package release archives
    Package(BuildArgs),
    /// Diagnose installed tools and host configuration
    Doctor(DoctorArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    Yaml,
}

#[derive(Debug, clap::Args)]
struct InitArgs {
    /// Project directory (defaults to the current directory)
    #[arg(short, long, value_name = "DIR")]
    path: Option<PathBuf>,

    /// Overwrite an existing omnibuild.yaml
    #[arg(long)]
    force: bool,
}

#[derive(Debug, clap::Args)]
struct DetectArgs {
    /// Project source directory
    #[arg(short, long, value_name = "DIR")]
    path: Option<PathBuf>,

    /// Maximum directory depth to scan
    #[arg(long, default_value_t = 4)]
    depth: usize,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(Debug, clap::Args)]
struct TargetsArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(Debug, clap::Args)]
struct PlanArgs {
    /// Project source directory
    #[arg(short, long, value_name = "DIR")]
    path: Option<PathBuf>,

    /// Path to omnibuild.yaml
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Target id to plan (repeatable)
    #[arg(long = "target", value_name = "TARGET", conflicts_with = "all")]
    targets: Vec<String>,

    /// Plan every supported target
    #[arg(long)]
    all: bool,

    /// Maximum directory depth to scan
    #[arg(long, default_value_t = 4)]
    depth: usize,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(Debug, clap::Args)]
struct DoctorArgs {
    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(Debug, clap::Args)]
struct BuildArgs {
    /// Project source directory
    #[arg(short, long, value_name = "DIR")]
    path: Option<PathBuf>,

    /// Path to omnibuild.yaml
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Target id to build (repeatable)
    #[arg(long = "target", value_name = "TARGET", conflicts_with = "all")]
    targets: Vec<String>,

    /// Build every supported target
    #[arg(long)]
    all: bool,

    /// Maximum directory depth to scan
    #[arg(long, default_value_t = 4)]
    depth: usize,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

pub fn run() -> i32 {
    let cli = Cli::parse();
    match execute(cli) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("omnibuild: {error:#}");
            1
        }
    }
}

fn execute(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init(args) => execute_init(args),
        Command::Detect(args) | Command::Inspect(args) => execute_detect(args),
        Command::Targets(args) => execute_targets(args),
        Command::Plan(args) => execute_plan(args),
        Command::Build(args) => execute_build(args, false),
        Command::Release(args) => execute_build(args, true),
        Command::Package(args) => execute_package(args),
        Command::Doctor(args) => execute_doctor(args),
    }
}

fn execute_init(args: InitArgs) -> Result<()> {
    let dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    ensure_dir(&dir)?;
    let path = dir.join("omnibuild.yaml");
    if path.exists() && !args.force {
        bail!(
            "{} already exists; pass --force to overwrite it",
            path.display()
        );
    }
    let name = safe_project_name(&dir);
    fs::write(&path, Config::sample(&name))?;
    println!("Wrote {}", path.display());
    println!("Next: omnibuild detect");
    Ok(())
}

fn execute_detect(args: DetectArgs) -> Result<()> {
    let source = args.path.unwrap_or_else(|| PathBuf::from("."));
    ensure_dir(&source)?;
    let report = detect::detect(&source, args.depth)?;
    match args.format {
        OutputFormat::Text => print_detection_text(&report),
        OutputFormat::Json => print_json(&report)?,
        OutputFormat::Yaml => print_yaml(&report)?,
    }
    Ok(())
}

fn execute_targets(args: TargetsArgs) -> Result<()> {
    match args.format {
        OutputFormat::Text => print_targets_text(),
        OutputFormat::Json => print_json(target::all())?,
        OutputFormat::Yaml => print_yaml(target::all())?,
    }
    Ok(())
}

fn execute_plan(args: PlanArgs) -> Result<()> {
    let (source, config, _config_path) = load_context(args.path.clone(), args.config.clone())?;
    ensure_dir(&source)?;
    let report = detect::detect(&source, args.depth)?;
    let targets = select_targets(&args.targets, args.all, &config)?;
    let plan = plan::build_plan(&source, &config, &report, &targets)?;
    match args.format {
        OutputFormat::Text => print_plan_text(&plan),
        OutputFormat::Json => print_json(&plan)?,
        OutputFormat::Yaml => print_yaml(&plan)?,
    }
    Ok(())
}

fn execute_build(args: BuildArgs, release: bool) -> Result<()> {
    let (source, config, _config_path) = load_context(args.path.clone(), args.config.clone())?;
    ensure_dir(&source)?;
    let report = detect::detect(&source, args.depth)?;
    let targets = select_targets(&args.targets, args.all, &config)?;
    let run = build::run(&source, &config, &report, &targets)?;
    match args.format {
        OutputFormat::Text => print_build_text(&run, release),
        OutputFormat::Json => print_json(&run)?,
        OutputFormat::Yaml => print_yaml(&run)?,
    }

    let failed = run
        .build
        .results
        .iter()
        .any(|result| result.status == "failed");
    let all_skipped = run
        .build
        .results
        .iter()
        .all(|result| result.status == "skipped");
    if failed {
        bail!("one or more targets failed");
    }
    if all_skipped {
        bail!("no target could be built; see the reasons above");
    }
    Ok(())
}

fn execute_package(args: BuildArgs) -> Result<()> {
    let (source, config, _config_path) = load_context(args.path.clone(), args.config.clone())?;
    ensure_dir(&source)?;
    let report = detect::detect(&source, args.depth)?;
    let targets = select_targets(&args.targets, args.all, &config)?;
    let run = package::run(&source, &config, &report, &targets)?;
    match args.format {
        OutputFormat::Text => {
            print_build_text(&run.release, true);
            print_package_text(&run);
        }
        OutputFormat::Json => print_json(&run)?,
        OutputFormat::Yaml => print_yaml(&run)?,
    }

    let failed = run
        .release
        .build
        .results
        .iter()
        .any(|result| result.status == "failed");
    if failed {
        bail!("one or more targets failed");
    }
    if run.packages.archives.is_empty() {
        bail!("no successful artifacts to package");
    }
    Ok(())
}

fn execute_doctor(args: DoctorArgs) -> Result<()> {
    let report = doctor::run();
    match args.format {
        OutputFormat::Text => print_doctor_text(&report),
        OutputFormat::Json => print_json(&report)?,
        OutputFormat::Yaml => print_yaml(&report)?,
    }
    Ok(())
}

fn load_context(
    path: Option<PathBuf>,
    config: Option<PathBuf>,
) -> Result<(PathBuf, Config, Option<PathBuf>)> {
    if let Some(config_path) = config {
        let config = Config::load(&config_path)?;
        let source = match path {
            Some(path) => path,
            None => config_path
                .parent()
                .unwrap_or(Path::new("."))
                .join(&config.project.source),
        };
        return Ok((source, config, Some(config_path)));
    }

    let source = path.unwrap_or_else(|| PathBuf::from("."));
    let candidate = source.join("omnibuild.yaml");
    if candidate.exists() {
        let config = Config::load(&candidate)?;
        let source = source.join(&config.project.source);
        Ok((source, config, Some(candidate)))
    } else {
        Ok((source, Config::default(), None))
    }
}

fn select_targets(
    targets: &[String],
    all: bool,
    config: &Config,
) -> Result<Vec<&'static target::TargetSpec>> {
    let ids: Vec<String> = if all {
        target::supported_ids(true)
            .into_iter()
            .map(str::to_string)
            .collect()
    } else if !targets.is_empty() {
        targets.to_vec()
    } else if !config.targets.is_empty() {
        config.targets.clone()
    } else {
        let host = target::host().ok_or_else(|| {
            anyhow::anyhow!("cannot determine a known host target; pass --target")
        })?;
        vec![host.id.to_string()]
    };

    let mut selected = Vec::new();
    for id in ids {
        let spec = target::by_id(&id)
            .ok_or_else(|| anyhow::anyhow!("unknown target `{id}`; run `omnibuild targets`"))?;
        selected.push(spec);
    }
    Ok(selected)
}

fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_yaml<T: Serialize + ?Sized>(value: &T) -> Result<()> {
    print!("{}", serde_yaml_ng::to_string(value)?);
    Ok(())
}

fn print_detection_text(report: &detect::DetectionReport) {
    println!("Source: {}", report.source);
    println!("Detected {} project(s)\n", report.project_count);
    if report.projects.is_empty() {
        println!("No known project markers found.");
        return;
    }
    for project in &report.projects {
        println!(
            "  {}  [{} / {}]  {}",
            project.name, project.language, project.build_system, project.root
        );
        if let Some(manifest) = &project.manifest {
            println!("    manifest: {manifest}");
        }
        if !project.evidence.is_empty() {
            println!("    evidence: {}", project.evidence.join(", "));
        }
        println!();
    }
}

fn print_targets_text() {
    println!(
        "{:<18} {:<9} {:<8} {:<8} {:<11} {:<32} {:<12} NOTES",
        "ID", "OS", "ARCH", "ABI", "TIER", "LLVM TRIPLE", "FORMAT"
    );
    for target in target::all() {
        println!(
            "{:<18} {:<9} {:<8} {:<8} {:<11} {:<32} {:<12} {}",
            target.id,
            target.os,
            target.arch,
            target.abi,
            target.tier.as_str(),
            target.llvm_triple,
            target.binary_format,
            target.notes
        );
    }
}

fn print_plan_text(plan: &plan::BuildPlan) {
    println!("OmniBuild {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!(
        "Project   {} ({} / {})",
        plan.project.name, plan.project.language, plan.project.build_system
    );
    println!("Root      {}", plan.project.root);
    println!("Build     {}", plan.build_mode);
    println!(
        "Verify    inspect={} dependencies={} tests={} require_target_execution={}",
        plan.verification.inspect_binary,
        plan.verification.check_dependencies,
        plan.verification.run_tests,
        plan.verification.require_target_execution
    );
    println!();
    println!("Targets ({})", plan.targets.len());
    for target in &plan.targets {
        println!(
            "  {:<18} {:<8} {}",
            target.id,
            format!("[{}]", target.tier),
            target.readiness.status.as_str().to_uppercase()
        );
        println!(
            "      triple  {}  format={}",
            target.llvm_triple, target.binary_format
        );
        println!(
            "      host    {} ({})",
            target.readiness.build_host,
            if target.readiness.cross {
                "cross"
            } else {
                "native"
            }
        );
        if let Some(reason) = &target.readiness.reason {
            println!("      reason  {reason}");
        }
        for tool in &target.readiness.tools {
            if !tool.available {
                println!(
                    "      missing {}: {}",
                    tool.tool,
                    tool.hint.as_deref().unwrap_or("not found")
                );
            }
        }
    }
}

fn print_build_text(run: &build::BuildRun, release: bool) {
    let results = &run.build.results;
    let executed = results
        .iter()
        .filter(|result| result.level == "executed")
        .count();
    let inspected = results
        .iter()
        .filter(|result| result.level == "inspected")
        .count();
    let skipped = results
        .iter()
        .filter(|result| result.status == "skipped")
        .count();
    let failed = results
        .iter()
        .filter(|result| result.status == "failed")
        .count();
    let cached = results
        .iter()
        .filter(|result| {
            result
                .cache
                .as_ref()
                .map(|cache| cache.hit)
                .unwrap_or(false)
        })
        .count();

    println!("OmniBuild {}", env!("CARGO_PKG_VERSION"));
    println!("Project   {}", run.build.project);
    println!("Source    {}", run.build.source);
    println!("Manifest  {}", run.release_manifest_path);
    println!();
    println!("Targets ({})", results.len());
    for result in results {
        println!(
            "  {:<18} {:<8} {}",
            result.target,
            result.status.to_uppercase(),
            result.level
        );
        println!("      adapter   {}", result.adapter);
        if let Some(artifact) = &result.artifact {
            println!("      artifact  {}", artifact.path);
            println!("      sha256    {}", artifact.sha256);
            println!(
                "      format    {} / {} ({})",
                artifact.format, artifact.architecture, artifact.kind
            );
        }
        if let Some(error) = &result.error {
            println!("      reason    {error}");
        }
        if let Some(manifest) = &result.manifest_path {
            println!("      manifest  {manifest}");
        }
        if let Some(cache) = &result.cache {
            println!(
                "      cache     {} ({})",
                if cache.hit { "hit" } else { "miss" },
                cache.key
            );
        }
    }
    println!();
    println!(
        "{}: {} executed, {} inspected, {} skipped, {} failed, {} cached",
        if release { "Release" } else { "Build" },
        executed,
        inspected,
        skipped,
        failed,
        cached
    );
}

fn print_package_text(run: &package::PackageRun) {
    println!();
    println!("Packages ({})", run.packages.archives.len());
    for archive in &run.packages.archives {
        println!("  {:<18} {}", archive.target, archive.archive);
        println!("      sha256    {}", archive.sha256);
        println!("      size      {} bytes", archive.size);
        println!("      manifest  {}", archive.target_manifest);
    }
    println!();
    println!("Package manifest  {}", run.package_manifest_path);
}

fn print_doctor_text(report: &doctor::DoctorReport) {
    println!("OmniBuild {}", env!("CARGO_PKG_VERSION"));
    match (&report.host, &report.host_llvm_triple) {
        (Some(host), Some(triple)) => println!("Host      {host} ({triple})"),
        _ => println!("Host      unknown"),
    }
    println!(
        "Rust      {}",
        report
            .rust_sysroot
            .as_deref()
            .unwrap_or("sysroot unavailable")
    );
    println!();
    println!("Tools");
    for tool in &report.tools {
        if tool.available {
            println!(
                "  ok       {:<12} {}",
                tool.tool,
                tool.path.as_deref().unwrap_or("")
            );
        } else {
            println!(
                "  missing  {:<12} {}",
                tool.tool,
                tool.hint.as_deref().unwrap_or("not found")
            );
        }
    }
    if !report.notes.is_empty() {
        println!();
        println!("Notes");
        for note in &report.notes {
            println!("  - {note}");
        }
    }
}

fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("source directory does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("not a directory: {}", path.display());
    }
    Ok(())
}

fn safe_project_name(dir: &Path) -> String {
    let raw = dir
        .canonicalize()
        .ok()
        .and_then(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "my-project".to_string());
    let cleaned: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "my-project".to_string()
    } else {
        trimmed
    }
}
