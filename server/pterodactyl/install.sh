#!/bin/bash
# Open Grind Web — Pterodactyl installation script.
# Runs in the install container (rust:1.95-bookworm), builds the web front
# (bun + vite) and og-server (cargo), and drops them into /mnt/server.
# Reinstalling updates the app; /mnt/server/data (sessions, secrets) stays.
set -euo pipefail

echo "==> Tools"
apt-get update -qq
apt-get install -y -qq --no-install-recommends git cmake clang libclang-dev unzip ca-certificates curl >/dev/null
curl -fsSL https://bun.sh/install | bash >/dev/null
export PATH="$HOME/.bun/bin:$PATH"

echo "==> Source: ${GIT_URL} (${GIT_BRANCH})"
URL="${GIT_URL}"
if [ -n "${GIT_TOKEN:-}" ]; then
	# private repository: https://<token>@host/owner/repo.git
	URL="$(echo "${GIT_URL}" | sed -E "s#^https://#https://${GIT_TOKEN}@#")"
fi
rm -rf /tmp/og-src
git clone --quiet --depth 1 --branch "${GIT_BRANCH}" "${URL}" /tmp/og-src
cd /tmp/og-src
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
cp server/target/release/og-server /mnt/server/og-server
chmod +x /mnt/server/og-server
rm -rf /mnt/server/build
cp -r build /mnt/server/build
rm -rf /tmp/og-src

echo "==> Done. Start the server; on first start without OG_PASSWORD_HASH the"
echo "    console shows a QR code to scan in your authenticator app."
