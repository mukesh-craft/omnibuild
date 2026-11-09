#!/bin/sh
if [ -z "${BASH_VERSION:-}" ]; then
	exec bash "$0" "$@"
fi
set -euo pipefail

usage() {
	printf 'usage: %s <debs-dir> <output-dir> [suite] [origin]\n' "$0" >&2
	exit 2
}

[ "$#" -ge 2 ] || usage

debs_dir=$1
out_dir=$2
suite=${3:-stable}
origin=${4:-OmniBuild}

[ -d "$debs_dir" ] || {
	printf 'Debian package directory not found: %s\n' "$debs_dir" >&2
	exit 1
}

case "$out_dir" in
	'' | / | .)
		printf 'refusing unsafe output directory: %s\n' "$out_dir" >&2
		exit 1
		;;
esac

command -v dpkg-deb >/dev/null 2>&1 || {
	printf 'dpkg-deb is required\n' >&2
	exit 1
}
command -v dpkg-scanpackages >/dev/null 2>&1 || {
	printf 'dpkg-scanpackages is required (install dpkg-dev)\n' >&2
	exit 1
}
for tool in gzip md5sum sha1sum sha256sum find sort; do
	command -v "$tool" >/dev/null 2>&1 || {
		printf '%s is required\n' "$tool" >&2
		exit 1
	}
done

shopt -s nullglob
debs=("$debs_dir"/*.deb)
shopt -u nullglob
[ "${#debs[@]}" -gt 0 ] || {
	printf 'no .deb files found in %s\n' "$debs_dir" >&2
	exit 1
}

mkdir -p "$out_dir"
rm -rf "${out_dir:?}/dists" "${out_dir:?}/pool"
mkdir -p "$out_dir/pool/main/o/omnibuild"
cp -f "${debs[@]}" "$out_dir/pool/main/o/omnibuild/"

mapfile -t architectures < <(
	for deb in "${debs[@]}"; do
		dpkg-deb -f "$deb" Architecture
	done | LC_ALL=C sort -u
)

for arch in "${architectures[@]}"; do
	mkdir -p "$out_dir/dists/$suite/main/binary-$arch"
	(
		cd "$out_dir"
		dpkg-scanpackages --arch "$arch" --multiversion pool /dev/null \
			>"dists/$suite/main/binary-$arch/Packages"
	)
	gzip -9n -kf "$out_dir/dists/$suite/main/binary-$arch/Packages"
done

release_tmp=$(mktemp)
trap 'rm -f "$release_tmp"' EXIT
release_dir="$out_dir/dists/$suite"
(
	cd "$release_dir"
	find . -type f ! -name Release ! -name InRelease ! -name Release.gpg |
		LC_ALL=C sort |
		while IFS= read -r file; do
			clean=${file#./}
			size=$(wc -c <"$clean" | awk '{print $1}')
			printf '%s\t%s\t%s\t%s\t%s\n' \
				"$(md5sum "$clean" | awk '{print $1}')" \
				"$(sha1sum "$clean" | awk '{print $1}')" \
				"$(sha256sum "$clean" | awk '{print $1}')" \
				"$size" \
				"$clean"
		done
) >"$release_tmp"

{
	printf 'Origin: %s\n' "$origin"
	printf 'Label: %s\n' "$origin"
	printf 'Suite: %s\n' "$suite"
	printf 'Codename: %s\n' "$suite"
	printf 'Architectures: %s\n' "${architectures[*]}"
	printf 'Components: main\n'
	printf 'Description: %s self-hosted APT repository\n' "$origin"
	printf 'Date: %s\n' "$(LC_ALL=C date -Ru)"
	printf 'MD5Sum:\n'
	while IFS=$'\t' read -r md5 _ _ size path; do
		printf ' %s %s %s\n' "$md5" "$size" "$path"
	done <"$release_tmp"
	printf 'SHA1:\n'
	while IFS=$'\t' read -r _ sha1 _ size path; do
		printf ' %s %s %s\n' "$sha1" "$size" "$path"
	done <"$release_tmp"
	printf 'SHA256:\n'
	while IFS=$'\t' read -r _ _ sha256 size path; do
		printf ' %s %s %s\n' "$sha256" "$size" "$path"
	done <"$release_tmp"
} >"$release_dir/Release"

if [ -n "${APT_GPG_KEY_ID:-}" ]; then
	command -v gpg >/dev/null 2>&1 || {
		printf 'gpg is required when APT_GPG_KEY_ID is set\n' >&2
		exit 1
	}
	gpg --batch --yes --armor --local-user "$APT_GPG_KEY_ID" \
		--digest-algo SHA256 --clearsign \
		--output "$release_dir/InRelease" "$release_dir/Release"
	gpg --batch --yes --armor --local-user "$APT_GPG_KEY_ID" \
		--digest-algo SHA256 --detach-sign \
		--output "$release_dir/Release.gpg" "$release_dir/Release"
else
	printf 'warning: APT_GPG_KEY_ID is unset; repository is unsigned\n' >&2
fi

printf '%s\n' "$out_dir"
