#!/usr/bin/env bash
# Builds Xray for Android as an AAR, from android/xray (Xray pinned in its
# go.mod), into the Android project's libs/.
#
# Needs Go, ANDROID_HOME and an NDK under it. The official gomobile, at the
# version in go.mod, is installed under target/: a gomobile already on PATH
# may be a fork (sing-box ships one) that binds against another package.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/app/src-tauri/gen/android/app/libs/xray.aar}"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
	ANDROID_NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)"
	export ANDROID_NDK_HOME
fi

mkdir -p "$(dirname "$out")"
cd "$root/android/xray"
export GOBIN="$root/target/gomobile"
export PATH="$GOBIN:$PATH"
go install golang.org/x/mobile/cmd/gomobile golang.org/x/mobile/cmd/gobind
gomobile init
# -checklinkname=0: Xray reaches into the runtime through linkname, which
# Go refuses by default since 1.23.
gomobile bind \
	-target=android/arm64,android/arm,android/amd64 \
	-androidapi 24 \
	-javapkg network.wanderer.app \
	-trimpath \
	-ldflags="-s -w -buildid= -checklinkname=0" \
	-o "$out" \
	.
echo "built $out"
