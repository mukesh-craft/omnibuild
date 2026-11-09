#!/bin/sh
if [ -z "${BASH_VERSION:-}" ]; then
	exec bash "$0" "$@"
fi
set -euo pipefail
umask 022

usage() {
	printf 'usage: %s <linux-archive.tar.gz> <version> <amd64|arm64> [output-dir]\n' "$0" >&2
	exit 2
}

[ "$#" -ge 3 ] || usage

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
archive=$1
version=$2
arch=$3
out_dir=${4:-"$repo_root/dist/packages"}

case "$arch" in
	amd64 | arm64) ;;
	*)
		printf 'unsupported Debian architecture: %s\n' "$arch" >&2
		exit 2
		;;
esac

case "$version" in
	'' | *[!0-9A-Za-z.+~]* | [!0-9]*)
		printf 'invalid Debian upstream version: %s\n' "$version" >&2
		exit 2
		;;
esac

[ -f "$archive" ] || {
	printf 'archive not found: %s\n' "$archive" >&2
	exit 1
}

command -v dpkg-deb >/dev/null 2>&1 || {
	printf 'dpkg-deb is required to build the package\n' >&2
	exit 1
}

tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

mkdir -p "$tmp_dir/src" "$tmp_dir/root/DEBIAN" "$tmp_dir/root/usr/bin" \
	"$tmp_dir/root/usr/share/doc/omnibuild" "$out_dir"
chmod 755 "$tmp_dir/root" "$tmp_dir/root/DEBIAN"

tar -xzf "$archive" -C "$tmp_dir/src"
binary=$(find "$tmp_dir/src" -maxdepth 2 -type f -name omnibuild -print -quit)
[ -n "$binary" ] || {
	printf 'omnibuild binary not found in %s\n' "$archive" >&2
	exit 1
}

install -m 755 "$binary" "$tmp_dir/root/usr/bin/omnibuild"
install -m 644 "$repo_root/README.md" "$tmp_dir/root/usr/share/doc/omnibuild/README.md"
install -m 644 "$repo_root/LICENSE" "$tmp_dir/root/usr/share/doc/omnibuild/LICENSE"
install -m 644 "$repo_root/COPYING" "$tmp_dir/root/usr/share/doc/omnibuild/COPYING"
install -m 644 "$script_dir/copyright" "$tmp_dir/root/usr/share/doc/omnibuild/copyright"

installed_size=$(du -sk "$tmp_dir/root/usr" | awk '{print $1}')
sed \
	-e "s/@VERSION@/$version/g" \
	-e "s/@ARCH@/$arch/g" \
	-e "s/@SIZE@/$installed_size/g" \
	"$script_dir/control.in" >"$tmp_dir/root/DEBIAN/control"

changelog="$tmp_dir/root/usr/share/doc/omnibuild/changelog.Debian"
printf 'omnibuild (%s) unstable; urgency=medium\n\n  * Release %s.\n\n -- mukesh-craft <mukesh-craft@users.noreply.github.com>  %s\n' \
	"$version" "$version" "$(LC_ALL=C date -Ru)" >"$changelog"
gzip -9n "$changelog"

deb_path="$out_dir/omnibuild_${version}_${arch}.deb"
rm -f "$deb_path"
build_args=(--build)
if dpkg-deb --help 2>&1 | grep -q -- '--root-owner-group'; then
	build_args=(--root-owner-group "${build_args[@]}")
fi
dpkg-deb "${build_args[@]}" "$tmp_dir/root" "$deb_path"
printf '%s\n' "$deb_path"
