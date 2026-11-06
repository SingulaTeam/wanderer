# Shared by the build scripts; sourced, not run.
#
# Every build copies what it made to one folder per version, outside the
# repository, so the files to hand out are in one place and survive the next
# build of target/:
#
#   ../releases/wanderer/<version>/   (beside the repository)
#
# WANDERER_RELEASES names another parent folder.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# The version the app is built as, from its Tauri configuration.
wanderer_version() {
	python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' \
		"$ROOT/app/src-tauri/tauri.conf.json"
}

# Copies each file given to the release folder and says where it went.
publish() {
	local dir="${WANDERER_RELEASES:-$ROOT/../releases/wanderer}/$(wanderer_version)"
	mkdir -p "$dir"
	cp -f "$@" "$dir/"
	for f in "$@"; do
		echo "released $(cd "$dir" && pwd)/$(basename "$f")"
	done
}

# Runs a heavy build with a hard memory cap and two jobs: over the cap the
# build is killed instead of the machine freezing in swap, as a full release
# build of the SDK's proving crates did on a 16 GB machine.
#
# In CI the runner is a machine of its own, and has no user systemd: the
# command runs as is.
capped() {
	if [[ -n "${CI:-}" ]]; then
		"$@"
		return
	fi
	CARGO_BUILD_JOBS=2 systemd-run --user --scope --quiet \
		-p MemoryMax="${WANDERER_BUILD_MEMORY:-6G}" -p MemorySwapMax=0 "$@"
}
