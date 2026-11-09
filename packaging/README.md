# Package-manager packaging

This directory contains package-manager recipes and manifests for OmniBuild
`v0.1.1`. They are checked in and validated, but none of the external registries
have been updated by this repository yet. Do not describe a package as available
until its install-and-run check passes on the target platform.

## Status

| Target | Recipe | `v0.1.1` coverage | External publication |
|---|---|---|---|
| Termux `pkg` | `termux/omnibuild/build.sh` | Android ARM64 | Pull request to `termux/termux-packages`, or a self-hosted Termux repo |
| Debian/Ubuntu `apt` | `debian/`, `apt/` | `amd64` from the static musl archive | Host and sign the generated apt repository |
| macOS Homebrew | `homebrew/omnibuild.rb` | macOS ARM64 | Publish a `homebrew-omnibuild` tap |
| Windows `winget` | `winget/manifests/` | Windows x64 | Pull request to `microsoft/winget-pkgs` |
| Windows `scoop` | `scoop/omnibuild.json` | Windows x64 | Publish a Scoop bucket |
| Windows Chocolatey | `chocolatey/` | Windows x64 | `choco push` with a Chocolatey API key |

The missing artifacts are real gaps, not omissions to hide:

- no glibc `linux-x86_64` archive (the `.deb` uses the static musl archive);
- no `linux-aarch64` archive, so no `arm64` `.deb` from `v0.1.1`;
- no `x86_64-apple-darwin` archive, so the Homebrew formula is ARM64-only;
- no Windows ARM64 archive.

## Input hashes

These are the `v0.1.1` archive hashes used by the checked-in metadata:

| Asset | SHA-256 |
|---|---|
| `omnibuild-0.1.1-windows-x86_64.zip` | `fc20b2030e9d08ad3576de2138e6ee147fd875e59ff1cbed52d68cf4dfd2632b` |
| `omnibuild-0.1.1-aarch64-apple-darwin.tar.gz` | `a12392676c97a9b648b77a27d30789b85d3155a3acf2b374903051928acf0b70` |
| `omnibuild-0.1.1-linux-x86_64-musl.tar.gz` | `e1a01f81e82a331b6529a046383cb404e45774fccd65e2b46d85a15fe80195f6` |
| `omnibuild-0.1.1-aarch64-linux-android.tar.gz` | `38581c519d797e59f9328265fa6d64b4951018e318e5957599ce0561120d8bc5` |
| source archive for the Termux recipe | `156f1ecb02705678aa9e30c75153c9cf767260f5c5dffb784556b464e38178bd` |

## Expected release layout

These manifests target the published `v0.1.1` assets:

- names are `omnibuild-<version>-<target>.tar.gz` or `.zip`;
- the binary is at the archive root (`omnibuild` or `omnibuild.exe`);
- the macOS and Linux archives include `README.md`, `LICENSE` and `COPYING`.

The generic `omnibuild package` command currently stages `artifact/<binary>` and names
archives `omnibuild-<target>-<version>`. Before using its output as a package-manager
release, either reshape it to the flat version-target layout or update the manifests,
verifier and workflow together. The verifier inspects the downloaded archives when
`--assets` is supplied.

## Validation

Run the metadata consistency check and shell syntax checks:

```bash
python3 scripts/verify-packaging.py
bash -n packaging/termux/omnibuild/build.sh
bash -n packaging/debian/build-deb.sh
bash -n packaging/apt/build-apt-repo.sh
```

Validate the Ruby formula on a host with Ruby:

```bash
ruby -c packaging/homebrew/omnibuild.rb
```

Validate the Chocolatey XML and Winget YAML on a host with the relevant tools:

```bash
choco pack packaging/chocolatey/omnibuild.nuspec --output-directory dist/packages
winget validate packaging/winget/manifests/m/mukesh-craft/omnibuild/0.1.1
```

`winget validate` is not available on Linux. Use `wingetcreate validate` on Windows
or rely on the submission PR's validation if the local tool is unavailable.

## Debian package and apt repository

Build the `amd64` `.deb` from the static musl release archive:

```bash
packaging/debian/build-deb.sh \
  dist/release/omnibuild-0.1.1-linux-x86_64-musl.tar.gz \
  0.1.1 \
  amd64 \
  dist/packages
```

Inspect it:

```bash
dpkg-deb --info dist/packages/omnibuild_0.1.1_amd64.deb
dpkg-deb --contents dist/packages/omnibuild_0.1.1_amd64.deb
```

Install a local file without a repository:

```bash
sudo apt install ./omnibuild_0.1.1_amd64.deb
```

Build an apt repository layout:

```bash
APT_GPG_KEY_ID='<key-id>' \
  packaging/apt/build-apt-repo.sh dist/packages dist/packages/apt-repo stable OmniBuild
```

The output contains `pool/`, `dists/stable/main/binary-amd64/Packages`,
`Packages.gz`, `Release`, `InRelease`, and `Release.gpg` when a key is supplied.
Publish the contents of `apt-repo/` over HTTPS. A client source line would be:

```text
deb [signed-by=/etc/apt/keyrings/omnibuild.gpg] https://example.invalid/omnibuild/ stable main
```

Replace the URL and keyring with the real publication host. Unsigned repositories
must not be documented as normal `apt` sources; if testing one, use
`[trusted=yes]` and label it insecure.

## Termux

`termux/omnibuild/build.sh` is intended to be copied to
`termux-packages/packages/omnibuild/build.sh` in a pull request. It builds from
the `v0.1.1` source tarball with `termux_setup_rust` and installs the binary into
`$TERMUX_PREFIX/bin`.

A self-hosted Termux repository is an alternative, but it must be signed and
documented like any other package source. `pkg install omnibuild` should not be
advertised until either the official recipe or a self-hosted repository is live.

## Homebrew

The formula is ARM64-only:

```bash
brew install --formula ./packaging/homebrew/omnibuild.rb
```

For a tap, create `mukesh-craft/homebrew-omnibuild`, put the formula at
`Formula/omnibuild.rb`, then users can run:

```bash
brew install mukesh-craft/omnibuild/omnibuild
```

The binary is ad-hoc signed, not Developer ID signed or notarized. Do not claim
notarized macOS distribution.

## Winget

Submit the three files under
`winget/manifests/m/mukesh-craft/omnibuild/0.1.1/` to
`microsoft/winget-pkgs`. The package identifier is `mukesh-craft.omnibuild`.
`wingetcreate` can submit or update the manifests, but the three-file layout here
is the canonical reviewed copy.

After acceptance:

```powershell
winget install --id mukesh-craft.omnibuild --exact
```

## Scoop

Commit `scoop/omnibuild.json` to a bucket repository. For a bucket named
`omnibuild`:

```powershell
scoop bucket add omnibuild https://github.com/mukesh-craft/scoop-bucket
scoop install omnibuild
```

The manifest uses `checkver.github` and `autoupdate`, so future release URLs and
hashes can be updated by Scoop tooling from the release `SHA256SUMS`. Do not
claim `scoop install omnibuild` works until the bucket is published.

## Chocolatey

Build the package on Windows:

```powershell
choco pack packaging/chocolatey/omnibuild.nuspec --output-directory dist/packages
```

The nuspec and install script use the Windows ZIP and its SHA-256. The install
script downloads the archive, verifies the checksum, extracts it, and creates an
`omnibuild` shim.

Publish only after local installation testing:

```powershell
choco push dist/packages/omnibuild.0.1.1.nupkg `
  --source https://push.chocolatey.org/ `
  --api-key $env:CHOCO_API_KEY
```

Chocolatey moderation is external. Do not claim `choco install omnibuild` works
until the package is approved.

## Release workflow

`.github/workflows/packages.yml` is a manual workflow. It downloads a published
release, verifies `SHA256SUMS`, the archive layouts and the Termux source tarball,
builds the `.deb`, apt repository and Chocolatey package, and uploads the generated
files as workflow artifacts. It installs the generated `.deb` and local Homebrew
formula, then runs `omnibuild --version` and `omnibuild doctor`; the Chocolatey job
runs the same smoke checks. The `upload_to_release` input is disabled by default;
when enabled it attaches only the `.deb` and `.nupkg` files to the release. The apt
repository remains a workflow artifact and must be hosted separately.

The workflow does not submit to Termux, Chocolatey, winget, Scoop or Homebrew.
Those actions require external repositories, accounts, API keys or GPG keys and
must be reviewed explicitly.

## External publication checklist

The repository cannot complete these steps without external credentials and review:

1. **Termux `pkg`**: open `termux/termux-packages` PR from `packaging/termux/omnibuild/build.sh`, or publish a signed self-hosted repo and verify `pkg install`.
2. **APT**: run `publish-apt-pages.yml` with `APT_GPG_PRIVATE_KEY`/`APT_GPG_KEY_ID` secrets, enable GitHub Pages, then verify `apt install omnibuild` with the published `signed-by` key.
3. **Homebrew**: publish `mukesh-craft/homebrew-omnibuild` containing `Formula/omnibuild.rb`; replace Intel SHA placeholder after `x86_64-apple-darwin` is released; verify `brew audit`/install on ARM and Intel Macs.
4. **Winget**: replace the ARM64 hash placeholder after a Windows ARM64 asset exists, then submit `packaging/winget/manifests/...` to `microsoft/winget-pkgs`.
5. **Scoop**: publish a bucket with `packaging/scoop/omnibuild.json`; replace ARM64 hash placeholder after release.
6. **Chocolatey**: run `choco push` with an API key; ARM64 is handled by script once a real ARM64 checksum is present.
7. **crates.io**: publish only after the package/license and release process are approved.

For future releases, run `release-assets.yml` to build native Linux glibc x86_64/aarch64, macOS Intel/ARM, and Windows x64/ARM64 assets in the flat `omnibuild-<version>-<target>` layout, then update placeholders and manifests before claiming coverage.
