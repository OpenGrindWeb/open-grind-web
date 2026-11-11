#!/bin/bash
# Open Grind Web — the egg's installation script. Kept tiny on purpose: it
# clones the repository and runs server/pterodactyl/install.sh from it, so
# install fixes ship with a plain Reinstall instead of a new egg import.
# Works on the server's volume, not /tmp (a small tmpfs under Wings).
set -euo pipefail

export OG_BUILD=/mnt/server/.build
rm -rf "$OG_BUILD"
mkdir -p "$OG_BUILD/tmp"
export TMPDIR="$OG_BUILD/tmp"

apt-get update -qq
apt-get install -y -qq --no-install-recommends git ca-certificates >/dev/null

echo "==> Source: ${GIT_URL} (${GIT_BRANCH})"
URL="${GIT_URL}"
if [ -n "${GIT_TOKEN:-}" ]; then
	# private repository: https://<token>@host/owner/repo.git
	URL="$(echo "${GIT_URL}" | sed -E "s#^https://#https://${GIT_TOKEN}@#")"
fi
git clone --quiet --depth 1 --branch "${GIT_BRANCH}" "${URL}" "$OG_BUILD/src"

exec bash "$OG_BUILD/src/server/pterodactyl/install.sh"
