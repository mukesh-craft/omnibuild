# OmniBuild

**A local-first build orchestrator that refuses to confuse "compiled" with "works".**

OmniBuild detects a project, plans a build for a target, invokes the right toolchain,
inspects the result, runs it when possible, and writes manifests, hashes, cache entries
and release archives. It is built for the awkward space between Cargo, CMake, Zig, qemu
and a machine that cannot natively run every target it builds for.

> **Status: experimental 0.1.0.** OmniBuild is a working proof of concept, not
> production build infrastructure. The verified target matrix is deliberately small,
> and the CLI may change. Prebuilt Linux, Windows and Android archives are attached to
> the [v0.1.0 release](https://github.com/mukesh-craft/omnibuild/releases/tag/v0.1.0).
> The Windows binary is built and PE-inspected here, not executed on Windows.

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
| Cargo cross-builds | Linux x86-64 gnu/musl, Linux aarch64, Linux riscv64, Windows x86-64, macOS ARM64 and WASI (`wasm32-wasi`). Uses the installed target std or falls back to `-Z build-std` with `rust-src`; Zig handles linking and `rcodesign` ad-hoc signs Mach-O artifacts. |
| CMake cross-builds | Linux x86-64 gnu/musl, Linux aarch64, Windows x86-64, Linux riscv64 and WASI (`wasm32-wasi`) through Zig compiler/archiver wrappers and a generated CMake toolchain file. |
| Flint cross-builds | Linux x86-64 and Linux aarch64 through `flintc` plus Zig-compiled Flint runtime sources. Flint WASI uses LLVM IR plus a small POSIX shim and Zig's WASI libc. |
| Artifact inspection | Verified for ELF, PE/COFF, WebAssembly and Mach-O: format, architecture, executable kind, size and SHA-256. macOS artifacts are ad-hoc signed so they can run on a Mac. |
| Runtime smoke test | Runs native artifacts directly, `linux-x86_64-musl` through `qemu-x86_64`, and WASI artifacts through `wasmtime`. A non-running artifact is never reported as executed. |
| Local cache | Implemented and tested: content-addressed cache of successful final artifacts. Cache hits restore, re-hash, re-verify and, when execution is enabled, re-run the artifact with zero build commands. |
| Packaging | Implemented and tested: per-target `.tar.gz` and `.zip` archives with the artifact, target manifest, release manifest and `SHA256SUMS`; plus a package manifest and top-level checksums. |
| Machine-readable output | Text, JSON and YAML output for every inspection and build command. |
| Reproducibility records | Source path and Git state, toolchain versions, exact commands, logs, artifact hashes, cache keys and verification levels in manifests. |

### Verified target matrix

These results were produced on Termux aarch64 with CMake 4.4.5, Zig 0.17.0,
qemu-x86_64 11.1.3, wasmtime 50.0.1, Rust 1.99.1 (including `rust-src` and the
`wasm32-wasip1` std), Flint 0.0.23 and rcodesign 0.29.0. macOS and Windows archive
execution was additionally checked on GitHub Actions. They are environment-specific, not
universal support claims.

| Target | Tier | Builders verified | Best verified result |
|---|---|---|---|
| `android-arm64` | 1 | Cargo, CMake, Flint | Executed natively |
| `linux-x86_64-musl` | 1 | Cargo, CMake, Flint | Executed through qemu |
| `linux-x86_64` | 1 | Cargo, CMake, Flint | Inspected; glibc binary does not execute under Termux qemu |
| `linux-aarch64` | 1 | Cargo, CMake, Flint | Inspected |
| `windows-x86_64` | 2 | Cargo, CMake | Executed on GitHub Actions Windows x64; inspect-only on Termux |
| `linux-riscv64` | 3 | Cargo, CMake | Inspected; experimental |
| `wasm32-wasi` | 3 | Cargo, CMake, Flint | Executed with wasmtime |
| `macos-arm64` | 2 | Cargo | Cross-built, ad-hoc signed and executed on GitHub Actions macOS arm64 |

Cargo cross-compilation uses the installed target `std` when present; otherwise OmniBuild
builds `std` from `rust-src` (`RUSTC_BOOTSTRAP=1`, `-Z build-std=std,panic_abort`).
Cross-linking goes through Zig. Windows build-std additionally needs `llvm-dlltool` and
`llvm-ar`. macOS builds use Zig for linking and `rcodesign` (`cargo install
apple-codesign --locked`) for ad-hoc signing. `--all` means every supported registry
entry, including experimental ones; it does not mean every entry can execute. Flint WASI
works for the verified fixture and runtime, with the limitations listed below.

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
  caching and shared caches are not implemented, and cross-platform CI is limited to
  macOS and release-artifact execution.
- **Not every Rust target.** Cargo cross-builds cover the registry entries listed above,
  not the whole Rust target universe. Standard-library builds can take several minutes the
  first time. Windows is inspect-only on Termux and executed on a Windows runner; macOS
  builds are ad-hoc signed and executed on a macOS runner.
- **Not a complete Flint WASI runtime.** Flint WASI compiles and runs the verified
  fixture. `flint_command`/`flint_command_output` return `ENOSYS`; signal, process-clock
  and pthread behavior is emulated or limited.
- **Not a source of platform claims.** Windows is inspect-only on Termux and executed on
  a Windows runner. macOS artifacts are built and ad-hoc signed here and executed on a
  macOS runner. WASI is executed with wasmtime and reported as such.

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

Prebuilt **v0.1.0** archives:

- `omnibuild-0.1.0-linux-x86_64-musl.tar.gz`
- `omnibuild-0.1.0-windows-x86_64.zip`
- `omnibuild-0.1.0-aarch64-linux-android.tar.gz`

Download them from the
[v0.1.0 release](https://github.com/mukesh-craft/omnibuild/releases/tag/v0.1.0)
and check `SHA256SUMS`. The Windows archive has been executed on GitHub Actions
`windows-latest`; Termux verification remains inspect-only. There is no crates.io release
yet.

A macOS ARM64 archive has also been built, ad-hoc signed and executed on GitHub Actions
`macos-26-arm64`, but it is not attached to the public `v0.1.0` release yet.

### Planned one-command installs

These package-manager paths are planned, not published yet:

```bash
# Termux
pkg install omnibuild

# Debian/Ubuntu
sudo apt install omnibuild

# macOS
brew install omnibuild

# Windows
winget install omnibuild
scoop install omnibuild
choco install omnibuild

# Any Rust host, after crates.io publishing
cargo install omnibuild
```

The planned order is Termux `pkg`, Debian/Ubuntu apt, Homebrew, then Windows package
managers. Each package must pass an install-and-run check in CI before it is documented as
available.

### Self-hosting

The `v0.1.0` archives are self-hosted. OmniBuild built this project from a clean
`git archive` checkout with `omnibuild release` for the three original shipped targets:

- `android-arm64` — native build on Termux;
- `linux-x86_64-musl` — Cargo cross-build with `-Z build-std` and Zig linking; executed
  under `qemu-x86_64`; and
- `windows-x86_64` — Cargo cross-build with the Windows binutils wrappers; inspected as
  PE/COFF here and executed on GitHub Actions `windows-latest`.

This is a self-hosting check. The earlier manual Windows PE hash differs for three known
reasons: it predates the `70765e8` path-canonicalization fix, it forced
`CARGO_PROFILE_RELEASE_LTO=false` while OmniBuild follows `Cargo.toml`'s `lto = "thin"`,
and PE headers embedded a wall-clock timestamp. LTO changes code size: rebuilding the same
`e3a09b4` source with LTO disabled produced `dedbc2d9…` at 3,055,104 bytes, while the
thin-LTO self-host build produced `6a80dbc6…` at 2,274,816 bytes. To make future Windows
builds deterministic, the Cargo adapter now sets `SOURCE_DATE_EPOCH=0` for Windows unless
the caller already set it; two consecutive verification builds then produced the same PE
hash. The old manual hash is historical, not a reproducible target. The shipped Windows
archive was executed on GitHub Actions `windows-latest`; Termux verification remains
inspect-only.

The macOS ARM64 archive was built from `d382bc7`, ad-hoc signed with `rcodesign` 0.29.0,
and executed on GitHub Actions `macos-26-arm64`. The workflow reported `omnibuild 0.1.0`
and a valid ad-hoc signature (`CDHash=5fdfc6a90b38f3a6759de6918822eaeab6ad9837`). The
Windows archive reported `omnibuild 0.1.0` and its executable SHA-256 was
`6a80dbc6a975e136004394598ff9d977c9d2d454b7204cf3b6698a054320797a`.

Execution evidence: [Verify release artifacts run 42454515351](https://github.com/mukesh-craft/omnibuild/actions/runs/42454515351).
The corresponding archive hashes are:

- macOS: `4c20a7ebcaab68a3f39325829e202dc82834c039eaae0ce7bb6e56c8803e6c6b` (binary),
  `795f2906d08414830a38075b4f4dcecef452ad3207171a81e841613a167b6dfa` (archive).
- Windows executable: `6a80dbc6a975e136004394598ff9d977c9d2d454b7204cf3b6698a054320797a`.

### Build for macOS from Termux

macOS builds use Zig for linking and `rcodesign` for ad-hoc signing:

```bash
cargo install apple-codesign --locked --no-default-features
omnibuild release --path . --target macos-arm64
./scripts/run-macos.sh ./dist/<project>/macos-arm64/<artifact> --version
```

Run `scripts/run-macos.sh` on the Mac. It removes the quarantine attribute, applies an
ad-hoc signature if needed, and executes the artifact. The macOS CI job runs the same
script against a native build. The cross-built macOS archive was also verified and
executed on GitHub Actions `macos-26-arm64` with `codesign --verify` passing.

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

# Build and execute a WASI artifact when the WASI tools are installed.
omnibuild release --path . --target wasm32-wasi

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
| Rust WASI std | Cargo `wasm32-wasip1` cross-builds | `pkg install rust-std-wasm32-wasip1` |
| Rust source | Building missing target `std` from source | `pkg install rust-src` |
| LLVM binutils | Windows build-std (`llvm-dlltool`, `llvm-ar`) | `pkg install llvm` |
| CMake + Ninja | CMake projects | `pkg install cmake ninja` |
| Zig | C/C++ cross-compilation and all cross-linking | `pkg install zig` |
| qemu-x86_64 | Executing `linux-x86_64-musl` artifacts | `pkg install qemu-user-x86-64` |
| wasmtime | Executing WASI artifacts | `pkg install wasmtime` |
| Flint | Flint projects | Build Flint and set `FLINTC=/path/to/flintc` |
| tar + zip | Release archives | `pkg install tar zip` |

Check what is available at any time:

```bash
omnibuild doctor
omnibuild plan --target <target>
```

## CI and verification

GitHub Actions runs `cargo fmt --all -- --check`,
`cargo clippy --all-targets -- -D warnings` and `cargo test` on Ubuntu. It provisions
Rust `rust-src` and the `wasm32-wasip1` std, Zig, wasmtime, CMake, Ninja and
`qemu-user`, so Cargo WASI, CMake WASI, CMake cross, Rust cross-build-std and musl
execution tests run in CI. Tests that need Flint skip unless `FLINTC` or `flintc` is
available.

### Evidence map

| Claim | Where it lives | What proves it |
|---|---|---|
| Artifact SHA-256 and inspection | `src/verify.rs` | Release tests assert the artifact hash; CI runs them. |
| Local cache entries | `src/cache.rs` | `release_reuses_a_cached_artifact_on_the_second_run`. |
| `.tar.gz`/`.zip` release archives | `src/package.rs` | `package_writes_verified_tar_and_zip_archives`; archive hashes and `SHA256SUMS` are checked. |
| Cargo WASI | `src/adapter.rs`, `src/build.rs` | `release_builds_and_executes_wasm_cargo_target_when_tools_are_available`. |
| CMake WASI | `src/adapter.rs` | `release_builds_and_executes_wasm_cmake_target_when_tools_are_available`. |
| Flint WASI | `src/adapter.rs` | `release_builds_and_executes_flint_wasi_target_when_tools_are_available`; also verified manually on Termux. |
| Rust cross-builds | `src/adapter.rs`, `src/plan.rs` | `release_builds_and_executes_rust_cross_target_when_tools_are_available`; manual Termux runs for gnu/aarch64/riscv64 (inspected) and Windows (PE inspected). |
| WASI execution | `src/build.rs` (`wasmtime`) | Wasm tests assert level `executed`. |

## FAQ

### Is this a replacement for Cargo/CMake/Zig/Cross?

No. It coordinates them. Those tools still do the compilation; OmniBuild handles target
selection, invocation, verification, manifests, caching and packaging.

### Does it run my test suite?

No. The current `run_tests` flag is a runtime smoke test: it executes the produced
artifact. A real test-suite runner is future work.

### Can it cross-compile Rust?

Yes, for the registry targets. OmniBuild uses an installed target `std` or builds one
from `rust-src`, links with Zig, and inspects the result. WASI runs through `wasmtime`;
musl x86-64 runs through `qemu-x86_64`; Windows and glibc/other Linux targets are
inspected only on this device.

### Does it support WASM?

Yes, for Cargo, CMake and Flint WASI targets when the required tools are installed.
Flint WASI uses an LLVM IR shim; command execution and full POSIX thread support are not
available.

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
- Go, Java, .NET, JavaScript, Python and Swift adapters.
- Real test-suite execution and a `tested` verification level.
- Windows runtime verification; macOS execution on a native Mac or macOS CI runner.
- Broader Flint WASI runtime coverage (commands, signals, threads).
- Package-manager distribution: Termux `pkg`, apt, PowerShell (`winget`/`scoop`/`choco`)
  and macOS Homebrew.

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

OmniBuild is licensed under the **OmniBuild License 1.0**
(`SPDX-License-Identifier: LicenseRef-OmniBuild-1.0`). The full text is in
[`LICENSE`](LICENSE). The GNU General Public License version 2 text is in
[`COPYING`](COPYING).

This section explains what the license does and why it is written this way. It is a
project explanation, not legal advice. The license text controls if this summary and the
license ever differ.

### What the default license allows

The default path is the **OmniBuild License 1.0**, not GPLv2. It grants a broad,
non-exclusive, royalty-free copyright license to reproduce, modify, and redistribute the
Work and Derivative Works in source or object form. It also grants a patent license for
patent claims that a Contributor controls and that the Contribution necessarily infringes.

You may use OmniBuild commercially in products, internal systems, client work, and build
pipelines. You may distribute it, including for a fee, if you follow the redistribution
conditions. The main restriction is Commercial Hosting under Section 5: offering a
substantial set of OmniBuild's features to third parties as a hosted or managed service
requires a separate written commercial hosting license from the Licensor.

The license removes two Apache-2.0-style documentation requirements:

- no separate `NOTICE` file must be reproduced; and
- no per-file "modified" markers or change descriptions are required.

It does not remove copyright, attribution, or license-notice obligations. If you distribute
the Work, you must include the license text and retain the applicable copyright, patent,
trademark, and attribution notices in source distributions.

### The four deliberate changes from Apache-2.0-style terms

1. **GPLv2 alternative.** Section 7 grants a separate alternative license under GPLv2. It
   is not an "additional permission" bolted onto GPLv2; GPLv2 has no general
   additional-permissions mechanism. If you choose the alternative, GPLv2 governs your
   copy and any Derivative Work you distribute under it.

2. **Lower documentation burden.** Section 4 removes the separate `NOTICE` file and
   per-file modification-notice requirements, while keeping the ordinary copyright,
   attribution, and license-text requirements.

3. **Commercial hosting.** Section 5 requires a separate written license for Commercial
   Hosting. Section 5.3 keeps internal use, personal and non-commercial use, ordinary
   distribution (including paid distribution), client professional services where the
   client receives a copy, build-tool use, and source forks available without that
   separate license.

4. **Narrower patent termination.** Section 3.2 terminates only the patent license, and
   only for a direct patent action about the Work or a Contribution. Unrelated patents,
   unrelated products, indirect or contributory claims, and defensive counterclaims do not
   trigger it. The copyright license does not terminate under Section 3.2.

### GPLv2 compatibility and the unavoidable trade-off

Apache 2.0 is compatible with GPLv3 but not with GPLv2 under the ASF's and FSF's published
compatibility guidance. OmniBuild uses a **separate GPLv2 alternative** in Section 7, with
the complete GPLv2 text in [`COPYING`](COPYING). A recipient who takes that route gets a
GPLv2 copy; Sections 3.2, 4, and 5 do not apply to that copy, and GPLv2 controls.

This has a direct consequence: **a GPLv2 copy can be used for Commercial Hosting without a
separate commercial hosting license**, because GPLv2 does not permit that extra
restriction. That is not an accident or a drafting gap; it is the price of GPLv2
compatibility. If preventing that outcome matters more than GPLv2 compatibility, the GPLv2
alternative must be removed. The default OmniBuild License path keeps the hosting
condition.

The alternative also does not relicense third-party dependencies. Some dependencies may be
Apache-2.0-only or otherwise GPLv2-incompatible, in which case the GPLv2 route may not be
available for a particular build. Check the dependency licenses before relying on it.

### Commercial Hosting in practice

Section 5.2 defines Commercial Hosting as making the Work or a Derivative Work available to
third parties as a Hosted or Managed Service where the service exposes a substantial set of
the Work's features and the operator receives a Commercial Advantage. Advertising,
subscriptions, data monetization, and similar indirect benefits count as Commercial
Advantage.

Section 5.3 is intentionally narrow. It does not restrict:

- internal use by you and your affiliates;
- personal, educational, research, or other non-commercial use without third-party
  Commercial Advantage;
- distributing the software, including selling copies, if it is not primarily a hosted or
  managed service;
- professional services for a specific client where the client receives a copy and the
  license;
- using OmniBuild as a build, test, packaging, or automation tool where the primary value
  is the software or artifacts produced, not access to OmniBuild itself; or
- publishing the source, including a fork, if recipients comply with the license.

Section 5.4 provides a 30-day cure after written notice and reinstates the licenses as of
the cure date. It does not excuse the period of violation, and a repeat violation after
reinstatement terminates the licenses permanently.

### Patent termination in practice

Section 3.2 is narrower than Apache 2.0's patent-termination clause. It applies only when
you bring a direct patent action alleging that the Work, or a Contribution as incorporated
in the Work, infringes a patent, and the accused instrumentality is the Work or
Contribution rather than a separate product or service. It excludes indirect, contributory,
and induced claims; unrelated products and unrelated patents; and defensive counterclaims.
The remedy is termination of the patent license for the Work, not termination of the
copyright license.

This is a deliberate risk-reduction choice, not a guarantee that the clause will be
enforced exactly as written in every jurisdiction.

### Redistribution, contributions, and third-party material

If you redistribute the Work or a Derivative Work, Section 4 requires you to include this
license, retain applicable notices, avoid misrepresenting authorship, and avoid imposing
extra restrictions on the Work. You may license your own original contributions under
other terms only if those terms do not restrict rights in the Work.

Section 6 says that intentionally submitting a contribution grants the copyright and
patent licenses in Sections 2, 3, and 7 for that contribution. If you cannot grant those
terms, state different terms in writing when you submit the contribution. The license does
not assign your copyright.

Third-Party Materials remain under their own licenses. The OmniBuild License does not
relicense them, and you must comply with their terms.

### Is this open source?

No. The license is **source-available**, not OSI-approved open source. The Open Source
Definition's criterion 6 forbids field-of-use restrictions, and Section 5 restricts a
particular commercial use. The same reason prevents calling it "open source" in a way that
implies OSI approval. It is also not on the SPDX License List, which is why it uses the
custom `LicenseRef-OmniBuild-1.0` identifier.

The license is related in spirit to other source-available licenses, but it is not legally
identical to any of them:

- **Functional Source License 1.1 (FSL-1.1-MIT)** restricts competing use for a period,
  then converts to MIT. OmniBuild does not convert automatically.
- **Elastic License 2.0** restricts managed-service use and has a different structure; it
  does not provide a GPLv2 alternative.
- **Commons Clause** adds a "not sell" restriction to an existing license. The public
  Commons Clause/Neo4j dispute shows the legal and branding risk of mixing a commercial
  restriction with open-source claims.

OmniBuild's license is a custom text. If OSI approval matters, the Section 5 hosting
restriction would have to be removed or narrowed to fit the Open Source Definition.

### References

- Apache License 2.0: <https://www.apache.org/licenses/LICENSE-2.0>
- ASF licensing and GPL compatibility: <https://www.apache.org/legal/resolved.html>
- GNU license-compatibility guidance: <https://www.gnu.org/licenses/license-list.html>
- Open Source Definition, especially criterion 6: <https://opensource.org/osd>
- Functional Source License: <https://fsl.software/>
- Elastic License 2.0: <https://www.elastic.co/licensing/elastic-license>
- Commons Clause: <https://commonsclause.com/>
- SPDX custom license identifiers: <https://spdx.github.io/spdx-spec/v2.3/other-licensing-information-detected/>

These references are background, not legal advice, and do not imply that those projects
endorse this license.

### Legal status and limits

This is a custom license, not legal advice. Its enforceability can vary by jurisdiction,
and commercial-hosting restrictions can interact with competition, consumer, contract, and
copyright law. Patent termination and warranty/liability exclusions can also be limited by
mandatory law. The license includes severability, a 30-day cure, and a narrow patent clause
to reduce risk, but it cannot eliminate uncertainty. Have counsel review it before relying
on it for a commercial hosting business or a high-value distribution.

The license does not grant trademark rights beyond customary origin descriptions. It
disclaims warranties and limits liability to the extent permitted by law. It does not
restrict rights that applicable law makes mandatory.

### Compliance checklist

- Include [`LICENSE`](LICENSE) with every distribution of the Work.
- Keep applicable copyright, patent, trademark, and attribution notices in source
  distributions.
- Do not impose extra restrictions on the Work beyond the license.
- Get a separate written commercial hosting license before offering a covered hosted or
  managed service.
- If you use the GPLv2 alternative, follow GPLv2, include [`COPYING`](COPYING), and check
  that your dependencies are GPLv2-compatible.
- For contributions, either accept Section 6 or state different terms in writing.
- Keep third-party notices and comply with third-party licenses.

### Metadata

`Cargo.toml` uses `license-file = "LICENSE"`. SPDX consumers can describe the choice as
`LicenseRef-OmniBuild-1.0 OR GPL-2.0-only`. The repository stores the canonical license
text so recipients can identify the terms without relying on a package registry.
