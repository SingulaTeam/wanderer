#!/usr/bin/env bash
# Fetches the Xray build Wanderer is tested with, checks it, and unpacks it.
#
#   scripts/fetch-xray.sh            # Linux: target/xray/xray
#   scripts/fetch-xray.sh windows    # Windows: target/xray-windows/xray.exe
#                                    #   and wintun.dll beside it
#
# Every download is pinned by digest, not by name: a release can be replaced
# under the same tag (v26.10.17, a prerelease, was withdrawn that way).
# v26.10.21 is the first stable release with the TUN inbound's routing, DNS
# and leak options.
set -euo pipefail

VERSION=v26.10.21
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

fetch() { # url archive sha256
	curl -fL --proto '=https' -o "$2" "$1"
	echo "$3  $2" | sha256sum -c -
}

case "${1:-linux}" in
linux)
	OUT="$ROOT/target/xray"
	mkdir -p "$OUT"
	fetch "https://github.com/XTLS/Xray-core/releases/download/$VERSION/Xray-linux-64.zip" \
		"$OUT/Xray-linux-64.zip" \
		0eec3ed4163c273d8c8bf821f7d219e257921cf584aa3fb970715e458e631978
	unzip -o -q "$OUT/Xray-linux-64.zip" xray -d "$OUT"
	chmod 755 "$OUT/xray"
	"$OUT/xray" version | head -1
	;;
windows)
	OUT="$ROOT/target/xray-windows"
	mkdir -p "$OUT"
	fetch "https://github.com/XTLS/Xray-core/releases/download/$VERSION/Xray-windows-64.zip" \
		"$OUT/Xray-windows-64.zip" \
		fc58c39ee2f2717b4c54d28d374362389e5319b20ae44aea76ea45a5243ec827
	unzip -o -q "$OUT/Xray-windows-64.zip" xray.exe -d "$OUT"
	# Xray's TUN on Windows is Wintun, loaded from beside xray.exe.
	fetch https://www.wintun.net/builds/wintun-0.14.1.zip \
		"$OUT/wintun-0.14.1.zip" \
		07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51
	unzip -o -q -j "$OUT/wintun-0.14.1.zip" wintun/bin/amd64/wintun.dll -d "$OUT"
	ls -l "$OUT/xray.exe" "$OUT/wintun.dll"
	;;
*)
	echo "usage: $0 [linux|windows]" >&2
	exit 2
	;;
esac
