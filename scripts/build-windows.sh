#!/usr/bin/env bash
# Builds the Windows installer from Linux and puts it in the release folder
# (see release.sh).
#
# Needs cargo-xwin (cargo install cargo-xwin), the x86_64-pc-windows-msvc
# target (rustup target add x86_64-pc-windows-msvc), makensis, and the app's
# npm dependencies.
set -euo pipefail

source "$(dirname "$0")/release.sh"
TARGET=x86_64-pc-windows-msvc

"$ROOT/scripts/fetch-xray.sh" windows
cd "$ROOT"
capped cargo xwin build --release -p wanderer-daemon --target "$TARGET"

# Tauri bundles external binaries named for their target.
BIN="$ROOT/target/windows-bin"
mkdir -p "$BIN"
cp "$ROOT/target/$TARGET/release/wandererd.exe" "$BIN/wandererd-$TARGET.exe"
cp "$ROOT/target/xray-windows/xray.exe" "$BIN/xray-$TARGET.exe"

# Tauri's installer template includes Win\RestartManager.nsh, which NSIS
# ships from 3.10 on; distributions carry older. The header alone is laid
# over a copy of the system's NSIS data, which makensis reads from NSISDIR.
NSIS="$ROOT/target/nsis"
if [[ ! -f "$NSIS/Include/Win/RestartManager.nsh" ]]; then
	rm -rf "$NSIS"
	cp -r /usr/share/nsis "$NSIS"
	ARCHIVE="$ROOT/target/nsis-3.11.zip"
	curl -fL --proto '=https' -o "$ARCHIVE" \
		"https://downloads.sourceforge.net/project/nsis/NSIS%203/3.11/nsis-3.11.zip"
	echo "c7d27f780ddb6cffb4730138cd1591e841f4b7edb155856901cdf5f214394fa1  $ARCHIVE" | sha256sum -c -
	unzip -o -q -j "$ARCHIVE" nsis-3.11/Include/Win/RestartManager.nsh -d "$NSIS/Include/Win"
fi
# The Tauri CLI does not pass NSISDIR on to makensis, so a wrapper first on
# PATH sets it.
mkdir -p "$NSIS/bin"
printf '#!/bin/sh\nNSISDIR=%q exec /usr/bin/makensis "$@"\n' "$NSIS" > "$NSIS/bin/makensis"
chmod 755 "$NSIS/bin/makensis"
export PATH="$NSIS/bin:$PATH"

cd "$ROOT/app"
capped npx tauri build --runner cargo-xwin --target "$TARGET" --bundles nsis
publish "$ROOT"/target/"$TARGET"/release/bundle/nsis/Wanderer_"$(wanderer_version)"_x64-setup.exe
