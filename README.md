# OmniBuild

**A local-first build orchestrator that refuses to confuse "compiled" with "works".**

OmniBuild detects a project, plans a build for a target, invokes the right toolchain,
inspects the result, runs it when possible, and writes manifests, hashes, cache entries
and release archives. It is built for the awkward space between Cargo, CMake, Zig, qemu
and a machine that cannot natively run every target it builds for.

> **Status: experimental 0.1.0.** OmniBuild is a working proof of concept, not
> production build infrastructure. The verified target matrix is deliberately small,
> the CLI may change, and there are no prebuilt releases yet.

## Why this project exists

Cross-building usually starts as a shell script. Then the script grows target branches,
environment overrides, special cases, and a collection of "it built" claims that nobody
can reproduce later. The important question gets lost:

**Did this target build, and did the binary actually run?**

OmniBuild started while building [Flint](https://github.com/mukesh-craft/Flint) on
Termux/Android. The device is aarch64 Android, but the interesting targets are Linux,
Windows and WASM experiments. Each tool answered part of the problem:

- Cargo, CMake and Flint know how to compile their own projects.
- Zig can cross-compile C/C++.
- qemu can run some x86-64 binaries.
- Shell scripts can glue them together, but they rarely record what happened.

OmniBuild is the glue and the audit trail. It does not replace the build systems; it
selects them, runs them without a shell, verifies the output, and records the difference
between **built**, **inspected** and **executed**.

This project exists because a build result should be evidence, not an assumption.

## What it can really do

These are the capabilities implemented and exercised in this repository. Nothing below
is a promise about an untested platform.

| Capability | What actually works |
|---|---|
| Project detection | Cargo, CMake and Flint projects, including monorepo scans. Marker-only detection also recognises Go, npm, Maven, Gradle, SwiftPM and dotnet projects. |
| Build planning | Target readiness, native vs cross reporting, missing-tool hints, per-target triples, and a `--all` view. |
| Native builds | Cargo, CMake and Flint on the host target. |
| CMake cross-builds | Linux x86-64 gnu/musl, Linux aarch64, Windows x86-64 and Linux riscv64 through Zig compiler/archiver wrappers and a generated CMake toolchain file. |
| Flint cross-builds | Linux x86-64 and Linux aarch64 through `flintc` plus Zig-compiled Flint runtime sources. |
| Artifact verification | ELF, PE/COFF, Mach-O and WebAssembly parsing; architecture and executable-kind checks; size and SHA-256. |
| Runtime smoke test | Runs native artifacts directly and `linux-x86_64-musl` through `qemu-x86_64`. A non-running artifact is never reported as executed. |
| Local cache | Content-addressed cache of successful final artifacts. Cache hits restore, re-hash, re-verify and, when execution is enabled, re-run the artifact with zero build commands. |
| Packaging | Per-target `.tar.gz` and `.zip` archives with the artifact, target manifest, release manifest and `SHA256SUMS`; plus a package manifest and top-level checksums. |
| Machine-readable output | Text, JSON and YAML output for every inspection and build command. |
| Reproducibility records | Source path and Git state, toolchain versions, exact commands, logs, artifact hashes, cache keys and verification levels in manifests. |

### Verified target matrix

These results were produced on Termux aarch64 with CMake 4.4.5, Zig 0.17.0,
qemu-x86_64 11.1.3, Flint 0.0.23 and Rust 1.99.1. They are environment-specific,
not universal support claims.

| Target | Tier | Builders verified | Best verified result |
|---|---|---|---|
| `android-arm64` | 1 | Cargo, CMake, Flint | Executed natively |
| `linux-x86_64-musl` | 1 | CMake, Flint | Executed through qemu |
| `linux-x86_64` | 1 | CMake, Flint | Inspected; glibc binary does not execute under Termux qemu |
| `linux-aarch64` | 1 | CMake, Flint | Inspected |
| `windows-x86_64` | 2 | CMake | Inspected; no runtime on this device |
| `linux-riscv64` | 3 | CMake | Inspected; experimental |
| `wasm32-wasi` | 3 | None | Skipped; no adapter supports WASI yet |
| `macos-arm64` | Unsupported | None | Excluded from 0.1 |

Cargo cross-compilation is blocked for every non-host target because target `std` is not
provisioned. `--all` means every supported registry entry, including experimental ones;
it does not mean every entry can execute.

## What OmniBuild is not

Being explicit here is more useful than a longer feature list.

- **Not a replacement for Cargo, CMake, Zig, Cross or a full build system.** It invokes
  them and records their results.
- **Not a universal compiler.** There are three build adapters: Cargo, CMake and Flint.
  Detection knows more project types than the build engine can currently build.
- **Not a test runner.** `verification.run_tests` runs the produced artifact as a smoke
  test. It does not run `cargo test`, CTest, or a project test suite.
- **Not a remote build service.** There are no hosted workers, remote scheduling,
  accounts, billing or dashboard. Some configuration fields for those ideas exist as
  placeholders.
- **Not production-ready.** The cache is local and artifact-only; compiler intermediate
  caching, shared caches and cross-platform CI are not implemented.
- **Not a Rust cross-compiler.** Non-host Cargo targets fail with an actionable error;
  OmniBuild never silently falls back to the host target.
- **Not a source of platform claims.** Windows is inspect-only here, macOS is
  unsupported, and WASM is currently skipped.

## Quick start

### Build from source

Requirements: Rust 1.85 or newer (the crate uses edition 2024; tested with 1.99.1)
and Cargo.

```bash
git clone https://github.com/mukesh-craft/omnibuild.git
cd omnibuild
cargo build --release
./target/release/omnibuild --help
```

Optional:

```bash
cargo install --path .
```

There are no published binaries or crates.io releases yet.

### Try it on a project

```bash
cd /path/to/your/project

# Optional: write an omnibuild.yaml with defaults.
omnibuild init

# See what tools exist and what the host looks like.
omnibuild doctor

# Detect projects and inspect the target plan.
omnibuild detect --path .
omnibuild plan --path . --target android-arm64

# Build, verify, and write manifests.
omnibuild release --path . --target android-arm64

# Add release archives.
omnibuild package --path . --target android-arm64
```

Use a target that `plan` reports as `READY`. If no target is passed, OmniBuild uses the
host target. On Termux aarch64, that is `android-arm64`; on Linux x86-64, it is
`linux-x86_64`.

## Commands

| Command | What it does today |
|---|---|
| `omnibuild init` | Writes an `omnibuild.yaml` with defaults. `--force` overwrites it. |
| `omnibuild detect` | Scans a directory and reports detected projects and marker evidence. |
| `omnibuild inspect` | Currently an alias of `detect`. Readiness and target details live in `plan`. |
| `omnibuild targets` | Lists the target registry, triples, tiers and formats. |
| `omnibuild plan` | Shows the primary project, selected targets, readiness and missing-tool hints without building. |
| `omnibuild doctor` | Reports host, Rust sysroot, installed tools and platform-specific notes. |
| `omnibuild build` | Runs the build pipeline and writes artifacts and manifests. |
| `omnibuild release` | The release-facing name for the same pipeline. It currently shares the exact implementation with `build`. |
| `omnibuild package` | Runs the pipeline, then writes `.tar.gz`/`.zip` archives, package manifests and checksums. |

Monorepos are detected, but a build currently targets the **primary detected project**
(the shallowest supported project, with Cargo preferred over CMake, then Flint).

## How a run works

```text
detect -> plan readiness -> select adapter -> build
       -> inspect binary -> optionally execute -> cache
       -> write target/release manifests -> package
```

Commands are spawned directly, not through a shell. Each command has a timeout, log file,
exit code and duration. A failed command fails the target and is preserved in the
manifest instead of being hidden.

Verification levels are recorded as:

| Level | Meaning |
|---|---|
| `built` | A build command produced a file, but inspection did not pass. |
| `inspected` | Format, architecture and executable kind matched the target. |
| `executed` | The artifact was actually run successfully. |
| `tested` | Defined by the product plan, but not emitted by the current code. |

Inspection is never reported as runtime success.

## Output layout

With the default configuration:

```text
dist/<project>/
├── manifest.json                 # release manifest
├── <target>/
│   ├── manifest.json             # target manifest
│   ├── build.log                 # adapter/toolchain logs
│   ├── run.log                   # present when the artifact was run
│   └── <artifact>
└── packages/
    ├── manifest.json             # package manifest
    ├── SHA256SUMS
    ├── <project>-<target>-0.1.0.tar.gz
    └── <project>-<target>-0.1.0.zip
```

The local cache lives in `.omnibuild-cache/` by default. Both `dist/` and
`.omnibuild-cache/` are ignored by Git.

## Configuration

`omnibuild init` writes a configuration with forward-compatible placeholders. The
minimal configuration that currently affects behavior is:

```yaml
version: 1

project:
  name: auto          # auto uses the detected project name
  source: .

build:
  mode: release       # release | debug
  timeout_secs: 1800

targets: []           # empty means host target

verification:
  run_tests: true     # runs the artifact as a smoke test, not the test suite

cache:
  enabled: true
  directory: .omnibuild-cache

artifacts:
  directory: dist/
```

Some generated fields are parsed but not enforced yet: `build.system`,
`toolchains`, `workers`, `verification.inspect_binary`,
`verification.check_dependencies`, `verification.require_target_execution` and
`artifacts.generate_manifest`. They are kept in the file so the schema can grow without
breaking existing configurations.

## Installation requirements

OmniBuild itself only needs Rust and Cargo. The tools below are needed only for the
projects and targets you want to build.

| Tool | Used for | Termux example |
|---|---|---|
| Rust/Cargo | Building OmniBuild and Cargo projects | `pkg install rust` |
| CMake + Ninja | CMake projects | `pkg install cmake ninja` |
| Zig | C/C++ cross-compilation and Flint cross-linking | `pkg install zig` |
| qemu-x86_64 | Executing `linux-x86_64-musl` artifacts | `pkg install qemu-user-x86-64` |
| Flint | Flint projects | Build Flint and set `FLINTC=/path/to/flintc` |
| tar + zip | Release archives | `pkg install tar zip` |

Check what is available at any time:

```bash
omnibuild doctor
omnibuild plan --target <target>
```

## CI and verification

GitHub Actions runs `cargo fmt --all -- --check`,
`cargo clippy --all-targets -- -D warnings` and `cargo test` on Ubuntu. The current
suite is 18 unit tests plus 11 CLI/end-to-end tests.

CI does **not** provision Zig, qemu or Flint, so the cross-build tests skip there. The
cross-target evidence in this README comes from the Termux aarch64 device, not from CI.
This is intentional: a green CI badge is not evidence that a cross target ran.

## FAQ

### Is this a replacement for Cargo/CMake/Zig/Cross?

No. It coordinates them. Those tools still do the compilation; OmniBuild handles target
selection, invocation, verification, manifests, caching and packaging.

### Does it run my test suite?

No. The current `run_tests` flag is a runtime smoke test: it executes the produced
artifact. A real test-suite runner is future work.

### Can it cross-compile Rust?

No. Cargo builds are limited to the host target. Rust cross targets fail with an
actionable error instead of silently producing a host binary.

### Why is a Windows build only "inspected"?

This device has no Wine or Windows runtime. OmniBuild can verify the PE/COFF format and
architecture, but it will not claim the binary executed.

### Why is `linux-x86_64-musl` used for runtime checks?

Zig's glibc dynamic binaries segfault under Termux qemu, while static musl binaries run
reliably. `linux-x86_64` gnu builds are therefore inspected, not executed, in this
environment.

### Is it production-ready?

No. It is an experimental 0.1.0 proof of concept with a deliberately narrow verified
matrix. Use it to make build evidence explicit, not as the only gate for a production
release.

## Roadmap

Deferred, not implemented:

- Shared/remote cache and compiler intermediate caching.
- Remote workers, scheduling, accounts, billing and a dashboard.
- Cross Rust, Go, Java, .NET, JavaScript, Python and Swift adapters.
- Real test-suite execution and a `tested` verification level.
- Windows runtime verification and macOS support.
- WASI sysroot/runner provisioning.
- Prebuilt release binaries and package-manager distribution.

## Contributing

Issues and pull requests are welcome. Before changing behavior:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
FLINTC=/path/to/flintc cargo test
```

For cross-target changes, include evidence from a real run: the target, the exact command,
whether the artifact was inspected or executed, and any skipped targets with reasons. Do
not advertise a target as working if it was only compiled.

## License

OmniBuild is dual-licensed under the MIT License or the Apache License, Version 2.0, at
your option, matching `Cargo.toml`.
