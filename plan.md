### OmniBuild — Complete Product & Engineering Plan

### Build once from one platform. Ship verified builds across operating systems and architectures.

My recommendation: Build OmniBuild as a universal, language-independent build orchestration platform, not as a replacement for every compiler.

OmniBuild should take an existing project, understand how it is built, select the correct toolchains and dependencies, produce native artifacts for requested targets, test what can actually be tested, and report exactly what is supported.

The first project we should use to prove the system is Flint. It already has target-aware LLVM code generation, so it gives us a real compiler project to integrate rather than a toy demonstration.

### 1\. The product we are actually building

### OmniBuild

Universal build orchestration and artifact verification

Any supported source repository

C/C++ · Rust · Go · Java · Python · JavaScript · Swift · C# · Flint · more

OmniBuild engine

Project detection · Build planning · Toolchain selection · Dependency resolution

Build adapters

CMake · Cargo · Go · Maven · Gradle · npm · Flint

Build workers

Linux · Windows · macOS · ARM64 · x86-64

Artifact verification

Format · Architecture · Dependencies · Tests · Provenance

Windows

.exe

Linux

ELF

macOS

Mach-O

One important engineering truth: OmniBuild cannot make one ordinary native executable run natively on every OS. It should generate a separate artifact for each target, unless the project uses a portable runtime or another packaging approach.

### 2\. Product goals and non-goals

The goals

* Build projects for multiple operating systems and CPU architectures from one interface.

* Support different languages through dedicated adapters.

* Automatically identify the project’s build system and required toolchains.

* Reuse build caches safely.

* Support both local and remote build workers.

* Package native outputs and their required runtime dependencies.

* Run tests on appropriate target environments.

* Provide a truthful compatibility report.

What we should not attempt initially

* Create a new compiler for every programming language.

* Promise every language can build for every target.

* Emulate every operating system.

* Pretend that generating an object file proves an application works.

* Claim macOS builds are fully supported without the required Apple toolchain and environment.

* Run untrusted repository build scripts on the host machine.

That boundary is important. It keeps the project achievable and technically credible.

### 3\. Target support strategy

We should use explicit support levels rather than a misleading list of operating systems.

| Level       | Meaning                                                                 |
| ----------- | ----------------------------------------------------------------------- |
| Tier 1      | Builds, links and tests in CI; documented and reliable                  |
| Tier 2      | Builds reliably, but some target tests or features are limited          |
| Tier 3      | Experimental; may produce objects or artifacts, but not fully validated |
| Unsupported | Not implemented or missing required toolchain support                   |

Initial target matrix:

| Operating system   | Architecture | Initial priority |
| ------------------ | ------------ | ---------------- |
| Linux              | x86-64       | P0               |
| Linux              | ARM64        | P0               |
| Windows (MinGW)    | x86-64       | P0               |
| Windows (MSVC)     | x86-64       | P1               |
| macOS              | ARM64        | P1               |
| macOS              | x86-64       | P1               |
| Android            | ARM64        | P1               |
| Linux              | RISC-V 64    | P2               |
| WebAssembly / WASI | wasm32       | P2               |

A target is not Tier 1 just because a compiler accepts its target triple. It needs a working link, runtime compatibility, and repeatable tests.

### 4\. Existing tools and how OmniBuild should differ

We need to be honest about the market: cross-compilation is already well established.

| Existing tool | What it already does                                                             | What OmniBuild should add                                                 |
| ------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| Zig           | Cross-compiles supported targets, including C/C++ through its compiler toolchain | Orchestrates multiple language ecosystems, not just Zig's toolchain       |
| CMake         | Supports cross-compilation through toolchain files                               | Automatically discovers and manages project build requirements            |
| cross-rs      | Provides container-backed cross-compilation for Rust                             | One consistent workflow across multiple languages                         |
| Bazel         | Build graph, caching and remote execution                                        | Simpler onboarding and automatic build planning for ordinary repositories |

These are not weak competitors. Zig can already generate Windows, Linux and macOS-targeted binaries from one host for supported configurations; CMake explicitly requires target-specific toolchain configuration; and `cross` provides containerized Rust cross-compilation. ziglang.org +2

Our differentiation must be the whole workflow:

* Detect what the project is and how it builds.

* Discover what each target requires.

* Generate a reproducible build plan.

* Choose local or remote workers.

* Build, package and verify outputs.

* Explain exactly why a target failed and what is missing.

The product should make complicated build infrastructure accessible without pretending that the underlying complexity has disappeared.

### 5\. System architecture

I recommend a local-first core with optional remote workers. This lets us build and test the product without first paying to operate a large cloud service.

![EngFlow Harnesses Google Bazel for Faster Monorepo Builds - The New Stack](https://images.openai.com/static-rsc-4/bW1C2RICdDPczfRMTIhVw1H3z26DRZzKlkg0RBmGzLib4xLNs-o2cHOwgouZrEtqfGkhf8OIr1TqT92mscqPoOhHT_3g3GPkqksBr1vR7SCjghX0IFJ2teozyZcezCz_lI-PGfB0h37CmoZmsFFXbZoY_KS3u89GWEsjTeHLUzxqseRPjcYgRapGPNmRbsUJ?purpose=fullsize)

### A. Project detector

Input: a source directory or Git repository.

It should detect:

* Language and language versions.

* Build system and build configuration.

* Entry points and output types.

* Dependencies and lockfiles.

* Existing CI configuration.

* Existing target support.

* Required SDKs and native libraries.

Detection should be evidence-based. For example, a `Cargo.toml` strongly suggests Rust/Cargo, but a repository can contain multiple independent projects. The detector must support monorepos rather than assume one repository equals one application.

### B. Build planner

The planner converts project information and requested targets into a concrete execution plan.

Example:

```yaml
project:
  name: example-app
  language: rust
  build_system: cargo

targets:
  - os: linux
    arch: x86_64
    abi: gnu
  - os: linux
    arch: aarch64
    abi: gnu
  - os: windows
    arch: x86_64
    abi: gnu

verification:
  inspect_binary: true
  run_tests: true
  test_on_target: when_available
```

The planner must identify missing prerequisites before starting expensive builds.

### C. Language adapters

Each language has its own build conventions. We should not try to solve them with a single universal shell command.

| Adapter                 | Build engine                        | Initial support |
| ----------------------- | ----------------------------------- | --------------- |
| C/C++                   | CMake, Ninja, Make, direct compiler | P0              |
| Rust                    | Cargo                               | P0              |
| Go                      | Go toolchain                        | P0              |
| Flint                   | Flint compiler/build system         | P0              |
| Java                    | Gradle, Maven                       | P1              |
| JavaScript / TypeScript | npm, pnpm, yarn                     | P1              |
| Python                  | PyInstaller or Nuitka integrations  | P1              |
| C#                      | .NET SDK                            | P2              |
| Swift                   | Swift Package Manager               | P2              |

A language adapter should provide a common contract:

```text
detect(project)
inspect(project)
plan(project, targets)
prepare(plan, worker)
build(plan, worker)
test(plan, worker)
package(plan, artifacts)
```

Each adapter returns structured results, not just terminal text.

### D. Toolchain manager

This component manages:

* Compiler versions.

* Linkers and assemblers.

* Target sysroots and SDKs.

* C and C++ runtimes.

* Platform headers and libraries.

* Environment variables.

* Toolchain checksums and provenance.

It should support several strategies:

* Use a compatible toolchain already installed.

* Use a pinned, downloadable toolchain.

* Use an isolated container or VM.

* Dispatch to a compatible remote worker.

Never silently substitute a different ABI or system library to make a build appear successful.

### E. Build execution engine

The execution engine runs planned tasks in an isolated environment.

Core responsibilities:

* Execute build steps.

* Stream logs.

* Capture exit codes and errors.

* Apply time, CPU and memory limits.

* Support cancellation.

* Record exact tool versions and commands.

* Preserve useful intermediate artifacts.

* Cache outputs only when inputs are safely identified.

### F. Artifact verification engine

This is one of the most important parts of OmniBuild.

It should check:

* Is the file actually a valid executable or library?

* Is its binary format correct?

* Does its architecture match the requested architecture?

* Does its ABI match the requested ABI?

* Are dynamic library dependencies resolvable?

* Is the runtime or packaging layout complete?

* Did tests pass on the target environment?

* Was the artifact produced by the expected toolchain?

A binary inspection result must not be presented as a successful runtime test.

### G. Artifact registry

Store artifacts with metadata:

```text
artifacts/
  example-app/
    1.0.0/
      linux-x86_64/
      linux-arm64/
      windows-x86_64/
      macos-arm64/
      manifest.json
```

The manifest should record:

* Source commit and repository identity.

* Target triple and ABI.

* Compiler and linker versions.

* Build configuration.

* Dependency information.

* Artifact hash.

* Verification results.

* Build logs and provenance.

### 6\. The most important design decision: build host versus target

OmniBuild must distinguish three different concepts:

| Concept          | Meaning                                                 |
| ---------------- | ------------------------------------------------------- |
| Build host       | Machine executing the compiler or build process         |
| Target platform  | Platform the generated program is intended to run on    |
| Test environment | Machine or emulator used to execute and test the result |

For example, a Linux ARM64 worker might generate a Windows x86-64 executable. That does not prove the executable works on Windows.

The system should support this execution model:

Example: Linux ARM64 host

Linux x86-64 target

Cross-build

Windows x86-64 target

Cross-build

macOS ARM64 target

Special SDK / worker requirements

Windows runtime test

Windows worker needed

This is why remote workers matter. A platform can coordinate all builds from one interface without pretending every target can be fully built and tested on one machine.

Worker types:

* Linux x86-64 worker.

* Linux ARM64 worker.

* Windows worker.

* macOS worker.

* Optional emulator or virtualized test worker.

macOS support must respect Apple's SDK, hardware and distribution requirements. We should use properly licensed macOS workers for the relevant build and signing operations rather than promise unrestricted macOS cross-building.

### 7\. CLI design

The command line should be the first interface. A web dashboard can come later.

Proposed commands:

| Command           | Purpose                                              |
| ----------------- | ---------------------------------------------------- |
| omnibuild init    | Create project configuration                         |
| omnibuild detect  | Inspect project and identify build systems           |
| omnibuild inspect | Show languages, dependencies and detected toolchains |
| omnibuild targets | List supported target combinations                   |
| omnibuild plan    | Preview the build plan                               |
| omnibuild build   | Build selected targets                               |
| omnibuild test    | Test generated artifacts                             |
| omnibuild package | Package release outputs                              |
| omnibuild release | Run build, verification and packaging                |
| omnibuild workers | Inspect available build workers                      |
| omnibuild cache   | Inspect and manage cache                             |
| omnibuild doctor  | Diagnose installation and configuration problems     |

Example usage:

```bash
# Inspect the repository
omnibuild detect

# Show available targets
omnibuild targets

# Preview what will happen
omnibuild plan --all

# Build for Linux and Windows
omnibuild build \
  --target linux-x86_64 \
  --target linux-arm64 \
  --target windows-x86_64

# Run available tests
omnibuild test --all

# Generate release artifacts
omnibuild release --all
```

The `--all` option must mean all supported and configured targets, not every architecture and operating system in existence.

### Example user experience

```text
$ omnibuild release --all

OmniBuild 0.1

[1/6] Detecting project
      Language: Rust
      Build system: Cargo

[2/6] Resolving targets
      Linux x86-64       Ready
      Linux ARM64        Ready
      Windows x86-64     Ready
      macOS ARM64        Worker unavailable

[3/6] Building
      Linux x86-64       Success
      Linux ARM64        Success
      Windows x86-64     Success

[4/6] Inspecting artifacts
      ELF x86-64         Valid
      ELF ARM64          Valid
      PE x86-64          Valid

[5/6] Testing
      Linux x86-64       Passed
      Linux ARM64        Passed
      Windows x86-64     Not runtime-tested

[6/6] Packaging
      3 artifacts created

Release completed with 2 runtime-tested targets.
```

Notice the distinction between built, inspected and tested. That distinction is a core product feature, not just a log-formatting choice.

### 8\. Configuration format

Use a readable project-level file called `omnibuild.yaml`.

Example:

```yaml
version: 1

project:
  name: my-project
  source: .

build:
  system: auto
  mode: release

targets:
  - linux-x86_64
  - linux-arm64
  - windows-x86_64

toolchains:
  strategy: auto
  pin_versions: true

workers:
  strategy: auto
  allow_remote: true

verification:
  inspect_binary: true
  check_dependencies: true
  run_tests: true
  require_target_execution: false

cache:
  enabled: true

artifacts:
  directory: dist/
  generate_manifest: true
```

Important configuration rules:

* `auto` should be a discovery strategy, not permission to guess.

* Pinning must be available for reproducible builds.

* A target can explicitly require runtime testing.

* Users can disable remote execution.

* The project must remain buildable through its original build system without OmniBuild.

### 9\. Repository structure

I recommend Rust for the OmniBuild core, with adapters allowed to use their ecosystem's existing tooling.

Why Rust?

* Strong type safety for the execution engine.

* Good process and filesystem APIs.

* Suitable for a portable CLI.

* Good concurrency and async support.

* Easier to distribute as a single native executable than a large Python application.

This is a design recommendation, not a requirement. If the first prototype becomes significantly faster to build in Go, that is a reasonable alternative. Do not build two implementations.

Suggested monorepo:

```text
omnibuild/
│
├── Cargo.toml
├── README.md
├── LICENSE
├── CONTRIBUTING.md
│
├── crates/
│   ├── omnibuild-cli/
│   ├── core/
│   ├── project-detector/
│   ├── build-planner/
│   ├── execution-engine/
│   ├── toolchain-manager/
│   ├── target-spec/
│   ├── artifact-manager/
│   ├── verifier/
│   ├── cache/
│   ├── worker-protocol/
│   ├── configuration/
│   └── reporting/
│
├── adapters/
│   ├── cmake/
│   ├── cargo/
│   ├── go/
│   ├── flint/
│   ├── gradle/
│   ├── maven/
│   ├── npm/
│   └── python/
│
├── workers/
│   ├── linux/
│   ├── windows/
│   └── macos/
│
├── schemas/
│   ├── config.schema.json
│   ├── build-plan.schema.json
│   └── artifact-manifest.schema.json
│
├── tests/
│   ├── fixtures/
│   ├── integration/
│   ├── cross-build/
│   └── security/
│
├── docs/
│   ├── architecture.md
│   ├── supported-targets.md
│   ├── adapter-development.md
│   ├── worker-setup.md
│   └── security.md
│
└── examples/
    ├── c-hello/
    ├── cpp-cmake/
    ├── rust-hello/
    ├── go-hello/
    └── flint-hello/
```

Keep the CLI thin. The core planner, target model, execution engine and artifact verifier should be independent libraries so they can eventually power a dashboard or service.

### 10\. Cache and reproducible builds

A poor cache can silently give users the wrong executable. This deserves careful engineering.

A cache key should include at least:

```text
source content hash
+ build configuration
+ target triple
+ ABI
+ compiler identity and version
+ linker identity and version
+ dependency lockfile hash
+ relevant environment inputs
+ build adapter version
```

Do not cache a result solely because the source files have not changed.

Additional requirements:

* Separate host tools from target artifacts.

* Avoid caching secret-bearing outputs.

* Make cache entries immutable.

* Verify artifact hashes after retrieval.

* Provide `omnibuild cache clean`.

* Support local cache first and remote cache later.

* Record whether a build was cache-restored or executed again.

Reproducibility should be a measured capability. We should compare independent builds and document which toolchains produce byte-identical artifacts and which do not.

### 11\. Security model

OmniBuild will execute code from repositories. That is a major security risk.

A malicious build script could read environment secrets, access SSH keys, modify the host, or attack the worker.

The minimum security requirements:

* Run builds in isolated workers.

* Use unprivileged users.

* Avoid mounting the host home directory.

* Never expose cloud credentials to untrusted builds.

* Restrict network access by policy.

* Apply CPU, memory, storage and time limits.

* Use separate temporary directories per build.

* Validate artifact paths before packaging.

* Do not trust build logs as structured instructions.

* Record worker identity and build provenance.

For remote workers, use authenticated communication and short-lived job credentials. For public multi-user hosting, add tenant isolation, rate limits, audit logs and stronger sandboxing before accepting untrusted users.

Important: Containers alone are not a complete security boundary for arbitrary hostile code. The hosted service should use hardened isolation, such as disposable VMs or equivalent sandboxing, for untrusted workloads.

### 12\. Remote worker architecture

The first version can build locally. Remote execution should be introduced through a stable worker protocol.

A worker receives a job containing:

```text
job_id
source_snapshot
build_plan
target_specification
toolchain_requirements
resource_limits
verification_policy
```

It returns:

```text
job_id
status
logs
exit_codes
artifact_locations
artifact_hashes
verification_results
worker_metadata
```

The coordinator should not care whether the worker is local, on another machine, or hosted in a cloud environment.

Build the protocol around explicit, versioned schemas. Avoid tying it to a particular cloud provider.

Later, we can support:

* Self-hosted workers.

* A developer's own spare computers.

* Hosted Linux/Windows/macOS worker pools.

* ARM64 hardware workers.

* Dedicated workers for licensed SDKs.

### 13\. Flint integration: our first serious proof

Flint should be the first non-trivial project integrated with OmniBuild.

The existing Flint work gives us a head start: its compiler has target-aware LLVM code generation, target triples, target-specific data layouts and object emission. Previous project notes also record Linux ARM64-to-x86-64 object generation and a Windows x86-64 MinGW executable link from an ARM host. These are useful milestones, but they do not establish complete runtime compatibility across every platform.

### Flint integration stages

Stage 1 — Discovery

The Flint adapter detects:

* Flint project files.

* Compiler version.

* Runtime source files.

* Existing target configuration.

* Build flags.

* Required native libraries.

Stage 2 — Target mapping

Translate OmniBuild targets into Flint target triples.

For example:

```text
omnibuild target: linux-x86_64
Flint target:     x86_64-unknown-linux-gnu

omnibuild target: windows-x86_64
Flint target:     x86_64-windows-gnu

omnibuild target: macos-arm64
Flint target:     aarch64-apple-darwin
```

These are proposed mappings; the adapter must validate them against the actual triples Flint accepts.

Stage 3 — Build and link

* Invoke Flint's existing compiler.

* Supply the requested target.

* Resolve the correct linker and target libraries.

* Preserve build logs.

* Detect missing sysroots or runtime components.

* Never silently fall back to the host target.

Stage 4 — Verify

* Inspect the generated object or executable.

* Confirm the binary format and architecture.

* Check its dynamic dependencies.

* Run it on a compatible target worker.

* Record the exact verification level.

Stage 5 — Expand support

Use real Flint build failures to improve OmniBuild's toolchain manager, target model and diagnostics.

The key principle: OmniBuild should improve Flint's build experience without taking ownership of Flint's compiler backend.

### 14\. Development roadmap

I would build this in eight phases. The timeline below is a realistic planning estimate for a focused small team or a solo developer using AI coding agents; it is not a guaranteed delivery date.

* Phase 0 — Technical foundation

    Start here

    * Define target triple and ABI model.

    * Define adapter interface.

    * Define build plan and artifact manifest.

    * Set up Rust workspace.

    * Create CLI skeleton.

    * Establish error and logging formats.

    Exit condition: A working CLI can load configuration, detect a project and print a structured build plan.

* Phase 1 — Local build MVP

    * Implement project detection.

    * Implement CMake and Cargo adapters.

    * Implement Linux x86-64 native builds.

    * Add target-aware build planning.

    * Add artifact inspection.

    * Produce a manifest and build report.

    Exit condition: A CMake project and a Rust project can be built and verified through the same CLI.

* Phase 2 — Cross-compilation

    * Add Linux ARM64.

    * Add Windows x86-64 MinGW.

    * Integrate toolchain discovery.

    * Support sysroots and dependency resolution.

    * Add missing-toolchain diagnostics.

    * Verify binary formats and architecture.

    Exit condition: Build a real project for Linux x86-64, Linux ARM64 and Windows x86-64 from one supported host configuration.

* Phase 3 — Flint adapter

    * Integrate Flint target detection.

    * Map OmniBuild targets to valid Flint triples.

    * Handle Flint runtime dependencies.

    * Add cross-target artifact inspection.

    * Test generated binaries on compatible workers.

    Exit condition: Flint is a first-class OmniBuild project, with a transparent target support report.

* Phase 4 — Build quality

    * Add dependency lockfile analysis.

    * Add deterministic build inputs.

    * Implement local caching.

    * Add test execution and reports.

    * Add packaging and release manifests.

    * Add integration and regression tests.

    Exit condition: Repeat builds are safely cacheable, failures are diagnosable, and release artifacts have verifiable metadata.

* Phase 5 — Remote workers

    * Design and version worker protocol.

    * Add Linux worker service.

    * Add Windows worker.

    * Add macOS worker using a compliant environment.

    * Add job scheduling and cancellation.

    * Add isolated execution and resource limits.

    Exit condition: One CLI command can coordinate builds across multiple worker operating systems.

* Phase 6 — More language ecosystems

    * Add Go.

    * Add Java and .NET.

    * Add JavaScript and Python packaging.

    * Add Swift where suitable workers and SDKs are available.

    * Add adapter conformance tests.

    Exit condition: Several language ecosystems use the same planning, worker, artifact and reporting interfaces.

* Phase 7 — Public product

    * Add web dashboard.

    * Add user accounts and projects.

    * Add artifact downloads and build history.

    * Add hosted worker quotas.

    * Add billing only if a hosted business model is chosen.

    * Publish support matrix, documentation and security policy.

    Exit condition: External developers can reliably use OmniBuild without needing the creator to configure each project manually.

### What we should build first

Do not start with the dashboard, billing, cloud orchestration or ten language adapters.

The first deliverable should be a reliable local CLI that can detect, plan, build and verify CMake, Cargo and Flint projects. That is enough to prove the central architecture.

### 15\. Testing and release gates

Every feature should have an acceptance test. A successful compiler invocation is not sufficient.

| Test category         | What it proves                                       |
| --------------------- | ---------------------------------------------------- |
| Unit tests            | Target parsing, configuration and planning work      |
| Adapter tests         | Correct commands and arguments are generated         |
| Integration tests     | Real projects build through OmniBuild                |
| Cross-build tests     | Foreign-target artifacts are generated               |
| Binary inspection     | Format, architecture and ABI metadata are correct    |
| Runtime tests         | Artifacts actually execute on target environments    |
| Reproducibility tests | Identical inputs produce expected repeatable outputs |
| Security tests        | Build isolation and permission boundaries work       |
| Regression tests      | Previously supported builds continue working         |

Build a small permanent fixture suite:

* C hello world.

* C++ CMake project with a library.

* Rust CLI.

* Go CLI.

* Flint CLI.

* A project with a native dependency.

* A project that intentionally fails to link.

* A project that intentionally requires an unavailable SDK.

* A project with a malicious or suspicious build script for sandbox testing.

The expected result of an unsupported build is a clear failure, not a misleading success.

### 16\. Release strategy and business model

I recommend starting open-source and keeping the core build engine independent of any paid service.

A possible structure:

| Component                       | Suggested model            |
| ------------------------------- | -------------------------- |
| OmniBuild CLI                   | Open source                |
| Core planner and adapters       | Open source                |
| Local workers                   | Open source                |
| Self-hosted worker coordinator  | Open source initially      |
| Hosted worker infrastructure    | Potential paid service     |
| Managed artifact storage        | Potential paid service     |
| Enterprise controls and support | Future commercial offering |

This gives developers a reason to trust and adopt the tool. It also means the project remains useful even if hosted infrastructure is expensive.

### Possible product editions

* Community: Local builds, supported adapters, basic verification.

* Self-hosted: Multiple workers, shared cache and team configuration.

* Cloud: Hosted workers, artifact storage and build history.

* Enterprise: Private infrastructure, access controls, auditability and support.

These are future product possibilities, not features we should implement in the MVP.

### 17\. Main risks and how we address them

| Risk                     | Why it matters                                    | Response                                            |
| ------------------------ | ------------------------------------------------- | --------------------------------------------------- |
| Too many languages       | Each ecosystem has different build rules          | Adapter architecture; expand gradually              |
| Missing system libraries | Compilation can succeed while linking fails       | Toolchain and sysroot management                    |
| macOS constraints        | SDK, signing and worker requirements              | Compliant macOS workers                             |
| Runtime incompatibility  | A binary may not run on the intended OS           | Target execution testing                            |
| Build-script security    | Arbitrary code can attack workers                 | Isolated, disposable environments                   |
| Cache corruption         | Wrong artifacts can be distributed                | Complete cache keys and hash verification           |
| Scope explosion          | Building a universal platform is a huge task      | Strict MVP and tiered support                       |
| Competition              | Existing tools already solve parts of the problem | Focus on unified orchestration and verified outputs |

One more risk deserves attention: packaging is not the same as cross-compilation. A Python application, for example, may require a platform-specific interpreter, native extensions and bundled libraries. OmniBuild must let the Python adapter use an appropriate packaging tool rather than assume every language produces a self-contained executable.

### 18\. Success metrics

We need objective metrics rather than just counting supported operating systems.

| Metric                | Initial target                                  |
| --------------------- | ----------------------------------------------- |
| Project detection     | Correctly detect all MVP fixtures               |
| Build-plan accuracy   | No unsupported target silently marked ready     |
| Native build success  | 100% of supported MVP fixtures in CI            |
| Cross-build success   | 100% of supported target fixtures in CI         |
| Artifact verification | Every release artifact has a manifest and hash  |
| Failure diagnostics   | Missing toolchains produce actionable errors    |
| Cache correctness     | No incorrect cache reuse in regression tests    |
| Runtime verification  | Clearly distinguish tested and untested targets |
| Adapter consistency   | All adapters pass the common contract tests     |

We should measure build time and cache hit rates after the system is stable. Premature optimization would distract us from correctness.

### 19\. Decisions to lock before implementation

These are my proposed defaults:

| Decision         | Recommendation                                                                           |
| ---------------- | ---------------------------------------------------------------------------------------- |
| Project name     | OmniBuild — working name; check trademark and package availability before public release |
| Core language    | Rust                                                                                     |
| First interface  | CLI                                                                                      |
| Architecture     | Local-first, remote-worker-ready                                                         |
| Build system     | Adapter-based                                                                            |
| Initial adapters | CMake, Cargo, Flint                                                                      |
| Initial targets  | Linux x86-64, Linux ARM64, Windows x86-64 MinGW                                          |
| macOS            | Add compliant workers before claiming full support                                       |
| Cache            | Local cache first                                                                        |
| Security         | Isolated builds from the beginning                                                       |
| Dashboard        | Later                                                                                    |
| Business model   | Open-source core, optional hosted service                                                |

### 20\. The first milestone: OmniBuild 0.1

OmniBuild 0.1 — Local cross-build engine

MVP target

Must contain

* Rust CLI and workspace.

* `omnibuild.yaml` configuration.

* Project detector.

* Target and ABI model.

* Build planner.

* CMake adapter.

* Cargo adapter.

* Flint adapter.

* Linux x86-64 native build.

* Linux ARM64 cross-build.

* Windows x86-64 MinGW cross-build.

* Binary inspector.

* Artifact manifest and hashes.

* Clear build and verification report.

* Integration tests in CI.

Explicitly excluded from 0.1

* Cloud dashboard.

* Billing.

* Public hosted workers.

* Automatic support for every language.

* Universal macOS cross-linking.

* Guaranteed runtime testing on every target.

### Proposed first repository layout

I would create a separate repository:

```text
mukesh-craft/OmniBuild
```

Keep Flint independent. OmniBuild should integrate with it through a documented adapter interface.

### Final product vision

### One command. Multiple native releases.

A developer gives OmniBuild a project and a target list. OmniBuild discovers the build process, resolves the correct toolchains, builds the artifacts, tests what it can, and explains what remains unverified.

### The real product is not a compiler.

It is the infrastructure that makes different compilers, build systems, operating systems and architectures work together through one reliable workflow.

My strongest recommendation: make correctness and transparent compatibility the competitive advantage. Do not advertise "supports 20 platforms" when only object generation has been tested. A smaller number of genuinely verified targets will earn more developer trust.

For the next engineering step, I can turn this plan into a complete repository-ready specification: architecture decisions, crate interfaces, schemas, CLI behavior, test fixtures and a phased issue backlog.

---

# Implementation Addendum — Device-Researched Plan (2026-11-06)

This section records the verified environment, locked decisions, and the concrete
implementation plan for the first milestone. The original plan above remains the
product vision; this addendum is the build contract we are executing.

## A. Verified environment

The primary development device is Termux on Android aarch64, not a normal Linux
workstation. Every target claim below was checked on that device.

| Check | Verified result |
|---|---|
| Host | Termux aarch64, Android/bionic, host triple `aarch64-unknown-linux-android24` |
| Rust | `rustc` / `cargo` 1.99.1; Android std, `wasm32-wasip1` std and `rust-src` 1.99.1 installed |
| C toolchain | clang/clang++ 21.1.8, GNU ld 2.44, `ld.lld`, `lld`, `wasm-ld`, GNU make 4.4.1 |
| Other tools | Python 3.14.8, Node 24.22.2, git 2.56.0, tar, zip 3.0, zipinfo |
| Cross tools | cmake 4.4.5, ninja 1.13.2, zig 0.17.0, qemu-x86_64 11.1.3 (installed for Sprint 2) |
| WASI tools | rust-std 1.99.1 for `wasm32-wasip1`; wasmtime 50.0.1 |
| Not installed | go, java, dotnet, swift, MinGW/wine |
| Temp directory | `/tmp` does not exist; use `$TMPDIR` or Termux's `$PREFIX/tmp` |
| Flint | `flintc 0.0.23`; `--target x86_64-pc-linux-gnu` emitted a real ELF64 x86-64 object |
| clang cross-emit | produced ELF x86-64, ELF aarch64, COFF x86-64 and WASM objects |
| Cargo native | built and ran a native Android ARM64 binary |
| Rust cross | works via `-Z build-std=std,panic_abort` + `rust-src` + Zig; verified musl/gnu/aarch64/riscv64 and Windows inspection |
| Windows build-std helpers | `llvm-dlltool` and `llvm-ar` exposed as `x86_64-w64-mingw32-dlltool` / `-ar` |
| Runtime testing | Linux x86-64 musl-static runs via `qemu-x86_64`; Zig's glibc dynamic loader segfaults under Termux qemu. Windows cannot run on-device (no wine) |

Important consequence: on this device, **Android ARM64 is the only true native
target**. "Linux ARM64" is already a cross build because the host uses bionic,
not glibc.

## B. Locked decisions

1. **Core language:** Rust.
2. **Repository layout:** one `omnibuild` binary crate with modules first. Do not
   start with the 13-crate monorepo; split only when a second consumer exists.
3. **Target order by verifiability:** Android ARM64 native (run) → Linux x86-64
   cross (qemu run) → Windows x86-64 (inspect only) → WASM (emit only).
4. **Cross C/C++ toolchain:** Zig (`zig cc`, `zig c++`, `zig ar`) first; LLVM-MinGW
   for Windows. Do not hand-build a sysroot manager in 0.1.
5. **Verification levels:** `Built < Inspected < Executed < Tested`. Inspection must
   never be reported as runtime success.
6. **Rust cross is in 0.1 for registry targets.** Use installed target std when
   present, otherwise build std from `rust-src`; never silently fall back to the host.
7. **Flint cross-link and Flint WASI are implemented** for the verified targets;
   keep their known runtime limits explicit.
8. **Optional toolchains are installed per phase, with approval:** cmake, zig,
   qemu-user-x86-64, llvm-mingw-w64.
9. **Local-first.** No dashboard, billing, hosted workers, or remote execution in 0.1.

## C. Target registry for 0.1

| ID | OS | Arch | ABI | Tier | LLVM triple | Rust triple | Flint triple | Format |
|---|---|---|---|---|---|---|---|---|
| android-arm64 | android | aarch64 | bionic | tier1 | aarch64-unknown-linux-android24 | aarch64-linux-android | aarch64-unknown-linux-android24 | ELF |
| linux-x86_64 | linux | x86_64 | gnu | tier1 | x86_64-unknown-linux-gnu | x86_64-unknown-linux-gnu | x86_64-pc-linux-gnu | ELF |
| linux-x86_64-musl | linux | x86_64 | musl | tier1 | x86_64-unknown-linux-musl | x86_64-unknown-linux-musl | x86_64-unknown-linux-musl | ELF |
| linux-aarch64 | linux | aarch64 | gnu | tier1 | aarch64-unknown-linux-gnu | aarch64-unknown-linux-gnu | aarch64-unknown-linux-gnu | ELF |
| windows-x86_64 | windows | x86_64 | gnu | tier2 | x86_64-w64-windows-gnu | x86_64-pc-windows-gnu | x86_64-windows-gnu | PE/COFF |
| macos-arm64 | macos | aarch64 | darwin | unsupported | aarch64-apple-darwin | aarch64-apple-darwin | aarch64-apple-darwin | Mach-O |
| wasm32-wasi | wasi | wasm32 | wasi | tier3 | wasm32-unknown-wasi | wasm32-wasip1 | wasm32-unknown-wasi | WebAssembly |
| linux-riscv64 | linux | riscv64 | gnu | tier3 | riscv64-unknown-linux-gnu | riscv64gc-unknown-linux-gnu | riscv64-unknown-linux-gnu | ELF |

Rules:
- `--all` means all supported targets and excludes `unsupported`.
- Readiness values are `ready`, `blocked`, `experimental`, `unsupported`.
- A target is not "ready" unless the adapter implementation and required
  toolchain both exist for it.
- The registry also stores a Zig target triple per entry.
- `linux-x86_64-musl` is the runtime-testable Linux x86-64 target on this device.
  `linux-x86_64` (gnu) builds and inspects but cannot execute under Termux qemu.
- `wasm32-wasi` builds through Cargo (`wasm32-wasip1`), CMake/Zig and Flint, and
  executes through wasmtime when those tools are installed.

## D. Sprint plan

### Sprint 0 — Technical foundation (COMPLETE)

Delivered:
- Rust workspace and CLI: `init`, `detect`, `inspect`, `targets`, `plan`, `doctor`.
- Target/ABI registry with LLVM, Rust and Flint triples.
- `omnibuild.yaml` parsing, defaults and validation.
- Evidence-based project detector with monorepo support and ignored directories.
- Build planner with honest readiness, cross/native reporting, missing-tool hints.
- Toolchain discovery and Rust std detection.
- Fixtures and automated tests.

Acceptance evidence:
- `cargo fmt` clean.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test`: 14 unit tests + 4 CLI integration tests passed.
- Live checks: Rust fixture `ready`; Flint native `ready`, Flint cross `experimental`;
  C fixture `blocked` with `cmake` install hint; `doctor` reported all tools.

### Sprint 1 — Native build MVP (COMPLETE)

Delivered:
- In-process adapter contract with working Cargo and Flint adapters.
- Execution engine: direct process spawning (no shell), per-command logs,
  timeouts, exit codes and captured duration.
- Artifact verifier using the `object` crate: format, architecture, executable
  kind, size and SHA-256 hash.
- Per-target and release manifests recording source identity, target triples,
  toolchain versions, exact commands, logs, hashes and verification level.
- `omnibuild build` and `omnibuild release` CLI commands.
- Integration fixtures and end-to-end tests for Rust and Flint.

Acceptance evidence:
- `omnibuild release` on the Rust fixture → `android-arm64`, adapter `cargo`,
  status `success`, level `executed`, ELF/aarch64, manifest and hash written.
- `omnibuild release` on the Flint fixture → `android-arm64`, adapter `flint`,
  status `success`, level `executed`, ELF/aarch64, manifest and hash written.
- `cargo test`: 16 unit tests + 6 CLI/end-to-end tests passed.
- `cargo clippy --all-targets -- -D warnings` clean.

Remaining work from the original Sprint 1 scope:
- Go and other language adapters remain later sprints (CMake landed in Sprint 2).
- Package archives were not generated in this sprint; tar/zip packaging landed in
  Sprint 3.

### Sprint 2 — Cross-build (COMPLETE)

Delivered:
- CMake adapter (native and cross) with generated Zig compiler/archiver wrappers
  and a CMake toolchain file.
- Zig-backed cross C/C++ for Linux x86-64 (gnu and musl), Linux aarch64,
  Windows x86-64 and Linux riscv64.
- Flint cross-link spike completed: `flintc --target` cross-emit, Zig-compiled
  `runtime.c` + `flint_aegis.c`, Zig link, then inspect/execute.
- Runtime runner: host-native runs directly; `linux-x86_64-musl` runs through
  `qemu-x86_64`; other cross targets remain `inspected`.
- Added `linux-x86_64-musl` because Zig glibc dynamic binaries segfault under
  Termux qemu, while static musl runs reliably.
- Readiness now marks CMake/Flint cross `ready` when Zig (and Flint runtime
  sources) are present; Rust cross was `blocked` at that point (superseded in
  Sprint 5); Flint non-x86_64/aarch64 Linux targets are `experimental`.

Acceptance evidence:
- `release --all` on `c-hello`: android `executed`; linux-x86_64 `inspected`;
  linux-x86_64-musl `executed`; linux-aarch64 `inspected`; windows-x86_64
  `inspected` (PE/x86_64); linux-riscv64 `inspected`; wasm skipped with reason.
- `release --all` on `flint-hello`: android `executed`; linux-x86_64 `inspected`;
  linux-x86_64-musl `executed`; linux-aarch64 `inspected`; windows, wasm and
  riscv64 skipped with explicit reasons.
- `release --target linux-x86_64-musl` on both fixtures → `success`/`executed`,
  ELF/x86_64.
- `release --target windows-x86_64` on `c-hello` → `success`/`inspected`,
  PE/x86_64.
- `cargo test`: 16 unit tests + 9 CLI/end-to-end tests passed.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt` clean.

Remaining from the original Sprint 2 scope:
- Windows runtime execution is impossible here (no wine); inspected only.
- Flint cross-link is not implemented for Windows; it is skipped with a reason.
- Rust cross and Flint WASI landed in Sprint 5.

### Sprint 3 — Build quality (COMPLETE)

Delivered:
- Content-addressed cache v1 in `src/cache.rs`. The key includes the project tree
  hash, config, target/ABI/triple, build mode, adapter version, lockfile hashes,
  selected environment variables, and compiler/linker versions.
- Build integration: on a hit the cached artifact is restored, re-hashed,
  re-verified and re-run; commands are empty and the manifest records the cache
  key, hit/miss and directory. Misses store only successfully verified artifacts.
- `omnibuild package`: per-target `.tar.gz` and `.zip` archives containing the
  artifact, target manifest, release manifest and `SHA256SUMS`, plus a package
  manifest and top-level `SHA256SUMS`. If `zip` is absent, tar.gz is still
  produced and the package manifest records a note.
- `doctor` now reports `tar` and `zip`; CI workflow and `.gitignore` added.
- Regression coverage for cache hits and archive contents.

Acceptance evidence:
- Repeat `release` on the Rust fixture: first run miss with 1 command; second and
  third runs hit with 0 commands, identical key and artifact hash, level executed.
- `package` on the Rust fixture produced
  `rust-hello-android-arm64-0.1.0.tar.gz` and `.zip`, both with 64-char SHA-256
  hashes, plus `packages/manifest.json` and a two-line `SHA256SUMS`. Listings
  show `artifact/`, `manifest.json`, `release.json` and `SHA256SUMS` in both.
- `cargo test`: 18 unit tests + 11 CLI/end-to-end tests passed.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt` clean.
- GitHub Actions CI on `main` passed: run `42332796838`, job `test`, all steps
  successful. The CI log reported 18 unit tests and 11 CLI tests passing.

Remaining from the original Sprint 3 scope:
- The cache stores final artifacts only; compiler intermediate caching is future work.

### Sprint 4 — WASI support (COMPLETE)

Delivered:
- Cargo WASI cross-builds: `cargo build --target wasm32-wasip1`, with target-std
  detection in readiness and `.wasm` artifact discovery.
- CMake WASI cross-builds through the existing Zig compiler wrappers and a
  `CMAKE_SYSTEM_NAME=WASI` toolchain; artifacts are normalized to `.wasm`.
- WASM inspection: enabled the `object` crate `wasm` feature and accepted WASM
  modules as executable-kind artifacts.
- Runtime execution through `wasmtime run`; cached WASI artifacts are restored,
  re-inspected and re-executed.
- Packaging carries `.wasm` artifacts into the same tar.gz/zip manifests.

Acceptance evidence:
- Rust fixture `release --target wasm32-wasi` → `success`/`executed`,
  WebAssembly/wasm32, `.wasm` artifact; second run cache hit with 0 commands.
- C fixture `release --target wasm32-wasi` → `success`/`executed`,
  `c-hello.wasm`, adapter CMake.
- C fixture `release --all` → 3 executed, 4 inspected, 0 skipped, 0 failed;
  WASI is one of the executed targets.
- `package --target wasm32-wasi` produced tar.gz and zip archives containing the
  `.wasm` artifact and checksums.
- `cargo test`: 18 unit tests + 13 CLI/end-to-end tests passed.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt` clean.

Remaining from the WASI scope:
- Flint WASI landed in Sprint 5 using LLVM IR plus POSIX stubs.
- CI now provisions wasmtime, the Rust WASI std, Zig and qemu, so these tests run there.

### Sprint 5 — Rust cross-targets + Flint WASI (COMPLETE)

Delivered:
- Generic Cargo cross adapter for installed target std or build-std fallback.
- Zig linker wrapper per cross target, including aarch64 flag filtering and musl
  `-C link-self-contained=no`.
- Windows build-std helper wrappers for `llvm-dlltool`/`llvm-ar`, plus Zig filter
  rules for MinGW system libraries and `libunwind`.
- Readiness reports `rust-src`, Zig and Windows binutils requirements.
- Flint WASI pipeline: emit LLVM IR, rename `main` to `flint_entry`, sanitize target
  features, compile a C entry plus POSIX shim, compile the Flint runtime with WASI
  emulation, and link with Zig.

Acceptance evidence:
- Rust `linux-x86_64-musl` → `success`/`executed` through qemu.
- Rust `linux-x86_64`, `linux-aarch64` and `linux-riscv64` → `success`/`inspected`.
- Rust `windows-x86_64` → `success`/`inspected`, PE/x86_64.
- Flint `wasm32-wasi` → `success`/`executed`, WebAssembly/wasm32, output
  `hello from flint-hello`.
- Added CLI coverage for Flint WASI and Rust musl build-std cross (qemu executed);
  full suite passed with 18 unit and 15 CLI tests.
- `cargo fmt` and `cargo clippy --all-targets -- -D warnings` clean.

Remaining:
- Windows artifacts cannot be run on this device.
- Flint WASI command execution and full thread/signal semantics remain limited.
- Cross-target test coverage is manual for Rust; CI provisions the shared tools but
  does not build every Rust target.

### Deferred

- Dashboard, accounts, billing, hosted workers.
- Remote worker protocol and scheduling.
- macOS support (requires licensed Apple SDK/worker).
- Go, Java, .NET, JavaScript, Python and Swift adapters.
- Broader Flint WASI runtime coverage (commands, signals, threads).

## E. Repository layout

```text
omnibuild/
├── Cargo.toml
├── README.md
├── plan.md
├── src/
│   ├── main.rs
│   ├── adapter.rs
│   ├── build.rs
│   ├── cache.rs
│   ├── cli.rs
│   ├── config.rs
│   ├── detect.rs
│   ├── doctor.rs
│   ├── exec.rs
│   ├── manifest.rs
│   ├── package.rs
│   ├── plan.rs
│   ├── target.rs
│   ├── toolchain.rs
│   └── verify.rs
├── tests/
│   ├── cli.rs
│   └── fixtures/
│       ├── c-hello/
│       ├── flint-hello/
│       └── rust-hello/
├── .github/workflows/ci.yml
├── .gitignore
└── target/                 # build output, not committed
```

## F. Known limitations and open items

- Android/bionic is the only native runtime environment on this device.
- Rust cross-compilation covers the registry targets through installed target std
  or `rust-src` build-std; Windows is inspected only, and first builds can be slow.
- Zig glibc dynamic binaries segfault under Termux qemu; use
  `linux-x86_64-musl` for executed Linux x86-64 verification.
- Flint cross-link is implemented for Linux x86_64/aarch64 and WASI. Windows and
  riscv64 Flint targets are not implemented.
- Windows artifacts can be inspected but not executed here.
- macOS is excluded from 0.1.
- The cache is local and artifact-based; shared/remote caches, compiler
  intermediate caching, remote workers and the dashboard are not implemented.
- Cache keys hash the whole project tree, so generated files outside `dist/` and
  `.omnibuild-cache/` intentionally invalidate the cache.
- Project detection is marker-based; it does not yet parse full build graphs.
- Cargo, CMake and Flint adapters are implemented; Go, Java, JavaScript, Python,
  C# and Swift adapters are not.
- Cargo cross-builds require a target std or `rust-src`; Zig links every cross
  target. Windows build-std also needs `llvm-dlltool` and `llvm-ar`.
- Release manifests, hashes and `.tar.gz`/`.zip` archives exist. Zip output
  requires the system `zip` tool; otherwise packaging falls back to tar.gz with a
  manifest note.
- CMake cross targets are build/inspect except `linux-x86_64-musl` (qemu) and
  `wasm32-wasi` (wasmtime), which execute.
- CI passed for the pre-WASI `main` commit (run `42332796838`); the WASI, Rust cross
  and Flint WASI commits are local and not yet pushed. CI now provisions Zig,
  wasmtime, `rust-src`, the WASI std and qemu, but not Flint.

## G. Verification protocol

Every sprint must end with:
1. `cargo fmt`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test`
4. A focused end-to-end run of the new command against the fixtures.
5. A truthful update to this addendum recording what was verified and what was not.
