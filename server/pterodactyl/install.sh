#!/bin/bash
# Open Grind Web — Pterodactyl installation, run by the egg's bootstrap from
# a fresh checkout at $OG_BUILD/src (install container: rust:1.95-bookworm).
#
# 1. Prebuilt: for a GitHub repository, download the `web-latest` release
#    built by .github/workflows/web-release.yml (seconds, little RAM/disk).
# 2. Otherwise build from source: the front (bun + vite) and og-server
#    (cargo). Needs about 4 GB of disk and 2-3 GB of RAM; everything happens
#    under $OG_BUILD on the server's volume, since Wings mounts the
#    container's /tmp as a small tmpfs.
#
# Set OG_BUILD_FROM_SOURCE=1 (egg variable or environment) to skip step 1.
# Reinstalling updates the app; /mnt/server/data (sessions, secrets) stays.
set -euo pipefail

OG_BUILD="${OG_BUILD:-/mnt/server/.build}"
export TMPDIR="$OG_BUILD/tmp"
mkdir -p "$TMPDIR" /mnt/server/data
cd "$OG_BUILD/src"

ASSET=open-grind-web-linux-x86_64.tar.gz

install_files() { # <dir holding og-server and build/>
	cp "$1/og-server" /mnt/server/og-server
	chmod +x /mnt/server/og-server
	rm -rf /mnt/server/build
	cp -r "$1/build" /mnt/server/build
}

release_base() { # https://github.com/<owner>/<repo>(.git) -> release download URL
	local repo
	repo="$(echo "${GIT_URL:-}" | sed -nE 's#^https://github\.com/([^/]+/[^/.]+)(\.git)?/?$#\1#p')"
	[ -n "$repo" ] && echo "https://github.com/$repo/releases/download/web-latest"
}

try_prebuilt() {
	[ "${OG_BUILD_FROM_SOURCE:-0}" = "1" ] && return 1
	[ -n "${GIT_TOKEN:-}" ] && return 1 # private repo: release assets need auth
	local base
	base="$(release_base)" || return 1
	[ -n "$base" ] || return 1
	echo "==> Prebuilt release: $base"
	curl -fsSL -o "$OG_BUILD/$ASSET" "$base/$ASSET" || return 1
	curl -fsSL -o "$OG_BUILD/$ASSET.sha256" "$base/$ASSET.sha256" || return 1
	(cd "$OG_BUILD" && sha256sum -c "$ASSET.sha256") || return 1
	tar -C "$OG_BUILD" -xzf "$OG_BUILD/$ASSET"
	echo "    built from $(cat "$OG_BUILD/open-grind-web/COMMIT" 2>/dev/null || echo '?')"
	install_files "$OG_BUILD/open-grind-web"
}

build_from_source() {
	echo "==> Building from source"
	export BUN_INSTALL="$OG_BUILD/bun"
	export BUN_INSTALL_CACHE_DIR="$OG_BUILD/bun-cache"
	export CARGO_TARGET_DIR="$OG_BUILD/target"
	export OG_WEB_BUILD=1 # lighter vite build (vite.config.mjs)

	echo "==> Tools"
	apt-get update -qq
	apt-get install -y -qq --no-install-recommends cmake clang libclang-dev unzip curl >/dev/null
	curl -fsSL https://bun.sh/install | bash >/dev/null
	export PATH="$BUN_INSTALL/bin:$PATH"

	# The Tauri toolchain file pins every mobile/desktop target; the web
	# build only needs the host one that ships with this image.
	rm -f rust-toolchain.toml

	echo "==> Front (vite build)"
	bun install --frozen-lockfile
	bunx vite build

	echo "==> Server (cargo build --release)"
	(cd server && cargo build --release --locked)

	mkdir -p "$OG_BUILD/out"
	cp "$CARGO_TARGET_DIR/release/og-server" "$OG_BUILD/out/"
	cp -r build "$OG_BUILD/out/build"
	install_files "$OG_BUILD/out"
}

if ! try_prebuilt; then
	echo "==> No usable prebuilt release, compiling instead"
	build_from_source
fi

echo "==> Clean up"
cd /mnt/server
rm -rf "$OG_BUILD"

echo "==> Done. Start the server; on first start without OG_PASSWORD_HASH the"
echo "    console shows a QR code to scan in your authenticator app."
