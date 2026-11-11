#!/bin/bash
# Open Grind Web — Pterodactyl installation, run by the egg's bootstrap from
# a fresh checkout at $OG_BUILD/src (in the install container,
# rust:1.95-bookworm). Builds the web front (bun + vite) and og-server
# (cargo) and installs them into /mnt/server.
#
# Everything happens under $OG_BUILD, on the server's own volume: Wings
# mounts the container's /tmp as a small tmpfs (100 MB by default), far too
# small for node_modules and a Rust build. Needs about 4 GB of free disk
# while installing; only ~30 MB stays afterwards.
#
# Reinstalling updates the app; /mnt/server/data (sessions, secrets) stays.
set -euo pipefail

OG_BUILD="${OG_BUILD:-/mnt/server/.build}"
export TMPDIR="$OG_BUILD/tmp"
export BUN_INSTALL="$OG_BUILD/bun"
export BUN_INSTALL_CACHE_DIR="$OG_BUILD/bun-cache"
export CARGO_TARGET_DIR="$OG_BUILD/target"
mkdir -p "$TMPDIR"
cd "$OG_BUILD/src"

echo "==> Tools"
apt-get update -qq
apt-get install -y -qq --no-install-recommends cmake clang libclang-dev unzip curl >/dev/null
curl -fsSL https://bun.sh/install | bash >/dev/null
export PATH="$BUN_INSTALL/bin:$PATH"

# The Tauri toolchain file pins every mobile/desktop target; the web build
# only needs the host one that ships with this image.
rm -f rust-toolchain.toml

echo "==> Front (vite build)"
bun install --frozen-lockfile
bunx vite build

echo "==> Server (cargo build --release)"
(cd server && cargo build --release --locked)

echo "==> Install"
mkdir -p /mnt/server/data
cp "$CARGO_TARGET_DIR/release/og-server" /mnt/server/og-server
chmod +x /mnt/server/og-server
rm -rf /mnt/server/build
cp -r build /mnt/server/build

echo "==> Clean up"
cd /mnt/server
rm -rf "$OG_BUILD"

echo "==> Done. Start the server; on first start without OG_PASSWORD_HASH the"
echo "    console shows a QR code to scan in your authenticator app."
