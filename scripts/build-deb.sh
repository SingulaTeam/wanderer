#!/usr/bin/env bash
# Builds the Debian package: the app, the tunnel service and its unit, and
# the pinned Xray.
#
#   scripts/build-deb.sh
#
# The package goes to the release folder (see release.sh). Installing it asks
# for a password once; the service then starts on every boot and the app
# never asks again.
set -euo pipefail

source "$(dirname "$0")/release.sh"
cd "$ROOT"

[[ -x target/xray/xray ]] || scripts/fetch-xray.sh
capped cargo build --release -p wanderer-daemon
(cd app && npm ci --silent && capped npx tauri build --bundles deb)

publish target/release/bundle/deb/Wanderer_"$(wanderer_version)"_amd64.deb
