#!/usr/bin/env bash
# Builds the Android app, one APK per processor architecture, and puts them in
# the release folder (see release.sh):
#
#   Wanderer_<version>_arm64.apk    nearly every phone since 2017
#   Wanderer_<version>_armv7.apk    older and low-end 32-bit phones
#   Wanderer_<version>_x86_64.apk   emulators and Chromebooks
#
#   scripts/build-android.sh
#
# Needs the environment in android-env.sh. Xray's AAR is built once, by
# build-xray-aar.sh, and kept.
#
# With app/src-tauri/gen/android/keystore.properties (storeFile,
# storePassword, keyAlias, keyPassword) it builds release APKs signed with
# that key: the ones to hand out. Every APK is checked before it is
# published — signed with that key and not the debug one, not debuggable,
# and carrying both native libraries for its architecture — and the build
# fails rather than publish one that would not install or would not run.
#
# Without keystore.properties it builds debug APKs, named so, for testing
# only. A phone holding a release build cannot update to a debug one, or the
# other way round: the signatures differ, and Android refuses.
set -euo pipefail

source "$(dirname "$0")/release.sh"
source "$ROOT/scripts/android-env.sh"

ANDROID="$ROOT/app/src-tauri/gen/android"
AAR="$ANDROID/app/libs/xray.aar"
[[ -f "$AAR" ]] || "$ROOT/scripts/build-xray-aar.sh" "$AAR"

BUILD_TOOLS="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)"
APKSIGNER="$BUILD_TOOLS/apksigner"
AAPT="$BUILD_TOOLS/aapt"

# Gradle flavor : ABI : the name in the file.
ARCHES=(
	"arm64:arm64-v8a:arm64"
	"arm:armeabi-v7a:armv7"
	"x86_64:x86_64:x86_64"
)

if [[ -f "$ANDROID/keystore.properties" ]]; then
	PROFILE=release
	SUFFIX=""
	# The certificate the release key holds, to check every APK against.
	prop() { sed -n "s/^$1=//p" "$ANDROID/keystore.properties" | tail -1; }
	store="$(prop storeFile)"
	[[ "$store" = /* ]] || store="$ANDROID/$store"
	EXPECTED="$(keytool -exportcert -rfc -keystore "$store" -storepass "$(prop storePassword)" \
		-alias "$(prop keyAlias)" | openssl x509 -noout -fingerprint -sha256 | cut -d= -f2 | tr -d : | tr A-F a-f)"
	[[ -n "$EXPECTED" ]] || { echo "Cannot read the certificate from $store." >&2; exit 1; }
else
	PROFILE=debug
	SUFFIX="-debug"
	echo "No keystore.properties: building debug APKs, for testing only." >&2
fi

cd "$ROOT/app"
flag=(); [[ "$PROFILE" = debug ]] && flag=(--debug)
# One architecture at a time. Asked for all three at once, Gradle runs a cargo
# build for each in parallel; and a Gradle daemon left from an earlier build
# runs them outside the memory cap. So no daemon — Gradle and the cargo it
# starts stay inside `capped` — and one target per build.
export GRADLE_OPTS="${GRADLE_OPTS:-} -Dorg.gradle.daemon=false"
for target in aarch64 armv7 x86_64; do
	capped npx tauri android build "${flag[@]}" --apk --split-per-abi --target "$target"
done

# Checks one APK; prints why and returns 1 if it is not fit to hand out.
check() {
	local apk="$1" abi="$2"
	"$APKSIGNER" verify "$apk" || { echo "$apk: not validly signed" >&2; return 1; }
	local libs
	libs="$(unzip -Z1 "$apk" 'lib/*' 2>/dev/null || true)"
	for lib in libwanderer_app_lib.so libgojni.so; do
		grep -qx "lib/$abi/$lib" <<<"$libs" || { echo "$apk: lib/$abi/$lib is missing" >&2; return 1; }
	done
	if grep -v "^lib/$abi/" <<<"$libs" | grep -q .; then
		echo "$apk: carries libraries for another architecture" >&2
		return 1
	fi
	[[ "$PROFILE" = release ]] || return 0
	# The line's prefix differs between build-tools releases ("Signer #1", or
	# "Signer (minSdkVersion=…)" for a v3 signature); the digest does not.
	local certs cert
	certs="$("$APKSIGNER" verify --print-certs "$apk")"
	cert="$(grep -m1 -oE 'certificate SHA-256 digest: [0-9a-f]{64}' <<<"$certs" | awk '{print $NF}')"
	if [[ "$cert" != "$EXPECTED" ]]; then
		echo "$apk: signed with ${cert:-an unreadable certificate}, not the release key $EXPECTED" >&2
		echo "$certs" >&2
		return 1
	fi
	if "$AAPT" dump xmltree "$apk" AndroidManifest.xml | grep -q 'android:debuggable.*0xffffffff'; then
		echo "$apk: debuggable" >&2
		return 1
	fi
}

OUTS=()
for entry in "${ARCHES[@]}"; do
	IFS=: read -r flavor abi name <<<"$entry"
	apk="$ANDROID/app/build/outputs/apk/$flavor/$PROFILE/app-$flavor-$PROFILE.apk"
	[[ -f "$apk" ]] || { echo "Missing $apk" >&2; exit 1; }
	check "$apk" "$abi"
	out="$ROOT/target/Wanderer_$(wanderer_version)_$name$SUFFIX.apk"
	cp -f "$apk" "$out"
	OUTS+=("$out")
done
publish "${OUTS[@]}"
