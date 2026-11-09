#!/usr/bin/env python3
"""Verify that checked-in packaging metadata is mutually consistent."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tarfile
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PACKAGING = ROOT / "packaging"


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise SystemExit(f"cannot read {path}: {exc}") from exc


def find_one(pattern: str, text: str, label: str, errors: list[str]) -> str:
    match = re.search(pattern, text, re.MULTILINE)
    if not match:
        errors.append(f"{label}: pattern not found")
        return ""
    return match.group(1)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def archive_members(path: Path) -> list[str]:
    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as archive:
            names = archive.namelist()
    else:
        with tarfile.open(path, "r:gz") as archive:
            names = archive.getnames()
    return [name[2:] if name.startswith("./") else name for name in names]


def load_sums(path: Path) -> dict[str, str]:
    sums: dict[str, str] = {}
    for line in read(path).splitlines():
        line = line.strip()
        if not line:
            continue
        parts = line.split()
        if len(parts) < 2:
            continue
        sums[parts[-1].lstrip("*")] = parts[0].lower()
    return sums


def release_asset(version: str, suffix: str) -> str:
    return f"omnibuild-{version}-{suffix}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sums", type=Path, help="SHA256SUMS file from the release")
    parser.add_argument("--assets", type=Path, help="directory containing release assets")
    parser.add_argument("--source-archive", type=Path, help="source tarball for the Termux recipe")
    args = parser.parse_args()

    errors: list[str] = []
    checks: list[str] = []

    cargo = read(ROOT / "Cargo.toml")
    version = find_one(r'^version\s*=\s*"([^"]+)"', cargo, "Cargo.toml", errors)
    if not version:
        print("cannot determine crate version", file=sys.stderr)
        return 1
    checks.append(f"crate version {version}")

    termux_path = PACKAGING / "termux/omnibuild/build.sh"
    termux = read(termux_path)
    termux_version = find_one(
        r'^TERMUX_PKG_VERSION=["\']?([^"\'\s]+)', termux, "Termux version", errors
    )
    termux_source_url = find_one(
        r"^TERMUX_PKG_SRCURL=(.+)$", termux, "Termux source URL", errors
    ).strip('"\'')
    termux_source_sha = find_one(
        r"^TERMUX_PKG_SHA256=([0-9a-fA-F]{64})", termux, "Termux source hash", errors
    ).lower()
    if termux_version != version:
        errors.append(f"Termux version {termux_version!r} != crate version {version!r}")
    if (
        f"/v{version}.tar.gz" not in termux_source_url
        and "/v${TERMUX_PKG_VERSION}.tar.gz" not in termux_source_url
    ):
        errors.append("Termux source URL does not point at the crate version")
    if not re.fullmatch(r"[0-9a-f]{64}", termux_source_sha):
        errors.append("Termux source SHA-256 is invalid")
    if "TERMUX_PKG_BLACKLISTED_ARCHES" not in termux:
        errors.append("Termux recipe does not restrict unsupported architectures")

    scoop_path = PACKAGING / "scoop/omnibuild.json"
    try:
        scoop = json.loads(read(scoop_path))
    except json.JSONDecodeError as exc:
        errors.append(f"Scoop manifest is invalid JSON: {exc}")
        scoop = {}
    if scoop:
        if scoop.get("version") != version:
            errors.append("Scoop version does not match the crate version")
        scoop_arch = scoop.get("architecture", {}).get("64bit", {})
        scoop_url = str(scoop_arch.get("url", ""))
        scoop_hash = str(scoop_arch.get("hash", "")).lower()
        if release_asset(version, "windows-x86_64.zip") not in scoop_url:
            errors.append("Scoop URL does not use the expected Windows archive")
        if not re.fullmatch(r"[0-9a-f]{64}", scoop_hash):
            errors.append("Scoop hash is not a SHA-256 value")
        autoupdate = (
            scoop.get("autoupdate", {}).get("architecture", {}).get("64bit", {})
        )
        if "$version" not in str(autoupdate.get("url", "")):
            errors.append("Scoop autoupdate URL does not use $version")
        autoupdate_hash = scoop.get("autoupdate", {}).get("hash", {})
        if "$baseurl/SHA256SUMS" not in str(autoupdate_hash.get("url", "")):
            errors.append("Scoop autoupdate hash does not use the release SHA256SUMS")
        arm64 = scoop.get("architecture", {}).get("arm64", {})
        if arm64:
            arm_url = str(arm64.get("url", ""))
            arm_hash = str(arm64.get("hash", "")).lower()
            if release_asset(version, "windows-arm64.zip") not in arm_url:
                errors.append("Scoop ARM64 URL does not use the expected Windows archive")
            if arm_hash != "replace_with_windows_arm64_sha256" and not re.fullmatch(r"[0-9a-f]{64}", arm_hash):
                errors.append("Scoop ARM64 hash is not a SHA-256 value or placeholder")
        license_field = scoop.get("license")
        if not isinstance(license_field, dict) or not license_field.get("url"):
            errors.append("Scoop license must be an object with a license URL")
        checks.append("Scoop manifest")

    winget_dir = PACKAGING / "winget/manifests/m/mukesh-craft/omnibuild" / version
    winget_version_path = winget_dir / "mukesh-craft.omnibuild.yaml"
    winget_installer_path = winget_dir / "mukesh-craft.omnibuild.installer.yaml"
    winget_locale_path = winget_dir / "mukesh-craft.omnibuild.locale.en-US.yaml"
    winget_version_text = read(winget_version_path)
    winget_installer = read(winget_installer_path)
    winget_locale = read(winget_locale_path)
    for label, text in (
        ("Winget version", winget_version_text),
        ("Winget installer", winget_installer),
        ("Winget locale", winget_locale),
    ):
        value = find_one(r"^PackageVersion:\s*(\S+)", text, label, errors)
        if value and value != version:
            errors.append(f"{label} version {value!r} != crate version {version!r}")
    winget_url = find_one(
        r"^\s*InstallerUrl:\s*(\S+)", winget_installer, "Winget installer URL", errors
    )
    winget_hashes = [h.lower() for h in re.findall(r"^\s*InstallerSha256:\s*([0-9A-Fa-f]{64}|REPLACE_WITH_WINDOWS_ARM64_SHA256)", winget_installer, re.MULTILINE)]
    winget_hash = winget_hashes[0] if winget_hashes else find_one(
        r"^\s*InstallerSha256:\s*([0-9A-Fa-f]{64})",
        winget_installer,
        "Winget installer hash",
        errors,
    ).lower()
    if release_asset(version, "windows-x86_64.zip") not in winget_installer:
        errors.append("Winget installer URL does not use the expected Windows archive")
    if release_asset(version, "windows-arm64.zip") in winget_installer and "Architecture: arm64" not in winget_installer:
        errors.append("Winget ARM64 installer is missing an arm64 architecture entry")
    if "RelativeFilePath: omnibuild.exe" not in winget_installer:
        errors.append("Winget nested installer path is not omnibuild.exe")
    if "PortableCommandAlias: omnibuild" not in winget_installer:
        errors.append("Winget portable command alias is not omnibuild")
    checks.append("Winget manifests")

    nuspec_path = PACKAGING / "chocolatey/omnibuild.nuspec"
    try:
        nuspec_root = ET.fromstring(read(nuspec_path))
    except ET.ParseError as exc:
        errors.append(f"Chocolatey nuspec is invalid XML: {exc}")
        nuspec_root = None
    nuspec_values: dict[str, str] = {}
    if nuspec_root is not None:
        for element in nuspec_root.iter():
            tag = element.tag.rsplit("}", 1)[-1]
            if tag in {"id", "version", "licenseUrl", "projectUrl"} and element.text:
                nuspec_values[tag] = element.text.strip()
        if nuspec_values.get("id") != "omnibuild":
            errors.append("Chocolatey package id is not omnibuild")
        if nuspec_values.get("version") != version:
            errors.append("Chocolatey version does not match the crate version")
        if not nuspec_values.get("licenseUrl"):
            errors.append("Chocolatey nuspec is missing licenseUrl")
        checks.append("Chocolatey nuspec")

    choco_install = read(PACKAGING / "chocolatey/tools/chocolateyInstall.ps1")
    choco_url = find_one(
        r"^\$url64\s*=\s*'([^']+)'", choco_install, "Chocolatey URL", errors
    )
    choco_hash = find_one(
        r"^\$checksum64\s*=\s*'([0-9A-Fa-f]{64})'",
        choco_install,
        "Chocolatey checksum",
        errors,
    ).lower()
    if release_asset(version, "windows-x86_64.zip") not in choco_url:
        errors.append("Chocolatey URL does not use the expected Windows archive")
    if "Install-ChocolateyZipPackage" not in choco_install:
        errors.append("Chocolatey install script does not call Install-ChocolateyZipPackage")
    if "Install-BinFile" not in choco_install:
        errors.append("Chocolatey install script does not create a shim")
    checks.append("Chocolatey install script")

    formula_path = PACKAGING / "homebrew/omnibuild.rb"
    formula = read(formula_path)
    formula_version = find_one(
        r'^\s*version\s+"([^"]+)"', formula, "Homebrew version", errors
    )
    if formula_version != version:
        errors.append("Homebrew version does not match the crate version")
    if "depends_on :macos" not in formula:
        errors.append("Homebrew formula must be restricted to macOS")
    arm_hash = find_one(
        r'omnibuild-[^"]+-aarch64-apple-darwin\.tar\.gz"\s*\n\s*sha256\s+"([0-9a-fA-F]{64})"',
        formula,
        "Homebrew ARM64 SHA-256",
        errors,
    ).lower()
    if release_asset(version, "aarch64-apple-darwin.tar.gz") not in formula:
        errors.append("Homebrew formula is missing the macOS ARM64 archive")
    intel_placeholder = "REPLACE_WITH_X86_64_APPLE_DARWIN_SHA256" in formula
    if release_asset(version, "x86_64-apple-darwin.tar.gz") not in formula and not intel_placeholder:
        errors.append("Homebrew formula should declare an Intel macOS archive or placeholder")
    formula_hash = arm_hash
    checks.append("Homebrew formula")

    debian_control = read(PACKAGING / "debian/control.in")
    for required in (
        "Package: omnibuild",
        "Version: @VERSION@",
        "Architecture: @ARCH@",
        "Homepage: https://github.com/mukesh-craft/omnibuild",
    ):
        if required not in debian_control:
            errors.append(f"Debian control template is missing: {required}")
    debian_builder = read(PACKAGING / "debian/build-deb.sh")
    if "dpkg-deb" not in debian_builder or "amd64 | arm64" not in debian_builder:
        errors.append("Debian builder does not support the expected architectures")
    apt_builder = read(PACKAGING / "apt/build-apt-repo.sh")
    if "dpkg-scanpackages" not in apt_builder or "InRelease" not in apt_builder:
        errors.append("APT repository builder is missing index or signing logic")
    checks.append("Debian/APT metadata")

    windows_hash = scoop_hash if scoop else ""
    for label, value in (
        ("Winget", winget_hash),
        ("Chocolatey", choco_hash),
    ):
        if value and windows_hash and value != windows_hash:
            errors.append(f"{label} Windows hash differs from the Scoop hash")
    if formula_hash and not re.fullmatch(r"[0-9a-f]{64}", formula_hash):
        errors.append("Homebrew ARM64 hash is not a SHA-256 value")

    sums: dict[str, str] = {}
    if args.sums:
        sums = load_sums(args.sums)
        expected_windows = release_asset(version, "windows-x86_64.zip")
        expected_macos = release_asset(version, "aarch64-apple-darwin.tar.gz")
        expected_linux = release_asset(version, "linux-x86_64-musl.tar.gz")
        expected_android = release_asset(version, "aarch64-linux-android.tar.gz")
        for name in (expected_windows, expected_macos, expected_linux, expected_android):
            if name not in sums:
                errors.append(f"SHA256SUMS is missing {name}")
        if expected_windows in sums and windows_hash:
            if windows_hash != sums[expected_windows]:
                errors.append("Windows packaging hash does not match SHA256SUMS")
        if expected_macos in sums and formula_hash != sums[expected_macos]:
            errors.append("Homebrew hash does not match SHA256SUMS")
        checks.append(f"release sums ({len(sums)} entries)")

    if args.assets:
        assets = args.assets
        referenced = {
            "Windows": release_asset(version, "windows-x86_64.zip"),
            "macOS": release_asset(version, "aarch64-apple-darwin.tar.gz"),
        }
        for label, name in referenced.items():
            path = assets / name
            if not path.is_file():
                errors.append(f"{label} release asset is missing: {path}")
                continue
            actual = sha256_file(path)
            expected = windows_hash if label == "Windows" else formula_hash
            if expected and actual != expected:
                errors.append(f"{label} release asset hash does not match the manifest")

        layout_checks = {
            release_asset(version, "windows-x86_64.zip"): (
                "omnibuild.exe",
            ),
            release_asset(version, "aarch64-apple-darwin.tar.gz"): (
                "omnibuild",
                "README.md",
                "LICENSE",
                "COPYING",
            ),
            release_asset(version, "linux-x86_64-musl.tar.gz"): ("omnibuild",),
            release_asset(version, "aarch64-linux-android.tar.gz"): ("omnibuild",),
        }
        for name, required in layout_checks.items():
            path = assets / name
            if not path.is_file():
                continue
            try:
                members = archive_members(path)
            except (OSError, tarfile.TarError, zipfile.BadZipFile) as exc:
                errors.append(f"cannot read release asset {name}: {exc}")
                continue
            for member in required:
                if member not in members:
                    errors.append(f"{name} must contain {member} at the archive root")

        if sums:
            for name, expected in sorted(sums.items()):
                path = assets / name
                if not path.is_file():
                    errors.append(f"release asset is missing: {path}")
                elif sha256_file(path) != expected:
                    errors.append(f"release asset hash does not match SHA256SUMS: {name}")
        checks.append("release assets")

    if args.source_archive:
        if not args.source_archive.is_file():
            errors.append(f"source archive not found: {args.source_archive}")
        else:
            actual = sha256_file(args.source_archive)
            if actual != termux_source_sha:
                errors.append("Termux source hash does not match the source archive")
            checks.append("Termux source archive")

    for path in PACKAGING.rglob("*"):
        if path.is_file():
            text = read(path)
            if "REPLACE_ME" in text or "TODO:" in text:
                errors.append(f"placeholder text remains in {path.relative_to(ROOT)}")

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    for check in checks:
        print(f"ok: {check}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
