TERMUX_PKG_HOMEPAGE=https://github.com/mukesh-craft/omnibuild
TERMUX_PKG_DESCRIPTION="Local-first build orchestration and artifact verification"
TERMUX_PKG_LICENSE="custom"
TERMUX_PKG_LICENSE_FILE="LICENSE"
TERMUX_PKG_MAINTAINER="@mukesh-craft"
TERMUX_PKG_VERSION=0.1.1
TERMUX_PKG_SRCURL=https://github.com/mukesh-craft/omnibuild/archive/refs/tags/v${TERMUX_PKG_VERSION}.tar.gz
TERMUX_PKG_SHA256=156f1ecb02705678aa9e30c75153c9cf767260f5c5dffb784556b464e38178bd
TERMUX_PKG_BLACKLISTED_ARCHES="arm,i686,x86_64"
TERMUX_PKG_BUILD_IN_SRC=true
TERMUX_PKG_AUTO_UPDATE=true

termux_step_make() {
	termux_setup_rust
	cargo build \
		--jobs "$TERMUX_PKG_MAKE_PROCESSES" \
		--release \
		--locked \
		--target "$CARGO_TARGET_NAME"
}

termux_step_make_install() {
	install -Dm755 "target/${CARGO_TARGET_NAME}/release/omnibuild" \
		"$TERMUX_PREFIX/bin/omnibuild"
	install -Dm644 README.md "$TERMUX_PREFIX/share/doc/omnibuild/README.md"
	install -Dm644 LICENSE "$TERMUX_PREFIX/share/doc/omnibuild/LICENSE"
	install -Dm644 COPYING "$TERMUX_PREFIX/share/doc/omnibuild/COPYING"
}
