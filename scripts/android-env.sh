#!/usr/bin/env bash
# The Android build environment for Wanderer.
#
# Source this before any `tauri android` command or scripts/build-xray-aar.sh:
#
#   source scripts/android-env.sh
#   (cd app && npx tauri android build)
#
# Every path can be overridden by exporting the variable beforehand, which is
# what CI does (the GitHub Actions runner provides its own SDK/NDK/JDK).

# ── JDK 17 ────────────────────────────────────────────────────────────────────
# Gradle 8.x + Android Gradle Plugin 8.x require JDK 17. Distro JDKs are often
# still on 11, so a user-space Temurin install is used as the fallback.
if [ -z "${JAVA_HOME:-}" ] || ! "${JAVA_HOME}/bin/java" -version 2>&1 | grep -q '"17'; then
	for candidate in \
		"$HOME/.local/share/singula-toolchain/jdk-17" \
		"/usr/lib/jvm/java-17-openjdk-amd64" \
		"/usr/lib/jvm/temurin-17-jdk-amd64"; do
		if [ -x "$candidate/bin/javac" ]; then
			export JAVA_HOME="$candidate"
			break
		fi
	done
fi

# ── Android SDK / NDK ─────────────────────────────────────────────────────────
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_SDK_ROOT="$ANDROID_HOME"

if [ -z "${NDK_HOME:-}" ] && [ -d "$ANDROID_HOME/ndk" ]; then
	# Pick the highest installed NDK revision.
	_ndk=$(ls -1 "$ANDROID_HOME/ndk" 2>/dev/null | sort -V | tail -1)
	[ -n "$_ndk" ] && export NDK_HOME="$ANDROID_HOME/ndk/$_ndk"
	unset _ndk
fi

# ── Go (for the Xray AAR) ──
export GOPATH="${GOPATH:-$HOME/go}"

export PATH="${JAVA_HOME:+$JAVA_HOME/bin:}$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin:$GOPATH/bin:$PATH"

if [ "${WANDERER_ENV_QUIET:-0}" != "1" ]; then
	echo "JAVA_HOME   = ${JAVA_HOME:-<unset>}"
	echo "ANDROID_HOME= ${ANDROID_HOME}"
	echo "NDK_HOME    = ${NDK_HOME:-<unset>}"
fi
