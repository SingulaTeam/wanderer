# Wanderer

Private, unblocked internet on the [Singula](https://singula.network) network, for Windows, Linux and Android.

Wanderer sends your traffic through nodes run by independent operators, disguised as an ordinary visit to a well-known site. You sign in with an email or a crypto wallet, take an Orbit (a subscription held in your wallet), and press connect. Sessions open with a zero-knowledge proof rather than your wallet, so the operator who serves one cannot tell whose it is.

Website: [wanderer.singula.network](https://wanderer.singula.network). Downloads are on the [releases page](https://github.com/SingulaTeam/wanderer/releases).

> **Open Alpha.** Wanderer runs on Singula's test network, Arbitrum Sepolia, with test USDC you can take in the app. Nothing real is spent.

## What it does

- **Anonymous sessions.** Each session is opened with a proof that the holder is a subscriber, not with the wallet. The Singula SDK builds the proof on the device.
- **Routes of one to three hops.** Pick a country for each hop, or leave it on Auto for the least loaded node. With several hops, no single operator sees both who you are and where you go; you can require every hop to belong to a different operator.
- **Nothing leaks.** From connect to disconnect, the Network Guard blocks everything that does not go through the tunnel — DNS and IPv6 included — and keeps blocking while Wanderer reconnects. The local network stays reachable.
- **Finds a way through.** Two transports, Vision and xHTTP, under REALITY covers. Wanderer tries them in turn and remembers what works on each network.
- **No gas.** Buying and activating an Orbit, and the test network's faucet, are signed by the wallet and relayed by a Control Plane; the wallet needs no ETH.

## How it is built

```text
app/                React UI (Vite), in a Tauri shell
app/src-tauri/      the Tauri app: commands, wallet, sign-in, tray, Android plugin
app/src-tauri/gen/android/
                    the Android project: VpnService, the tunnel plugin
crates/core/        the unprivileged core: Singula sessions, routes, the choice
                    of transport, keeping the tunnel up
crates/daemon/      wandererd, the privileged half on desktops: Xray with a TUN
                    interface, and the Network Guard (nftables on Linux, WFP on
                    Windows)
crates/tunnel/      what the app and the daemon agree on: the tunnel plan, the
                    Xray configuration, the guard's rules, the control protocol
android/xray/       Xray for Android, built as an AAR
scripts/            release builds, and the leak test
```

On a desktop the app runs as the user and asks `wandererd`, a system service, to bring the tunnel up over a local socket. On Android the same core drives a `VpnService`. Everything that touches Singula — Control Planes, proofs, permissions, the contracts — goes through the [Singula Rust SDK](https://github.com/SingulaTeam/rust-sdk).

## Building

You need Rust 1.87 or newer, Node.js 22, and [Tauri's prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform.

Sign-in uses [thirdweb](https://thirdweb.com). Create a client ID in its dashboard, allow your build's origins there, and put the ID in `app/.env`:

```text
cp app/.env.example app/.env
```

Without one the app builds and runs, but nobody can sign in.

### Checks

What CI runs:

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd app && npm ci && npx tsc --noEmit && npx vite build)
cargo deny check
```

### Packages

Each script puts what it builds in `../releases/wanderer/<version>/`, beside the repository.

| Platform | Script | Needs |
|---|---|---|
| Linux (`.deb`) | `scripts/build-deb.sh` | the Tauri prerequisites |
| Windows (installer) | `scripts/build-windows.sh` | `cargo-xwin`, the `x86_64-pc-windows-msvc` target, `makensis`; builds from Linux |
| Android (APKs for arm64, armv7, x86_64) | `scripts/build-android.sh` | Android SDK and NDK, JDK 17, Go; see `scripts/android-env.sh` |

`scripts/fetch-xray.sh` downloads the Xray release Wanderer is tested with, checked against pinned digests. For Android, `scripts/build-xray-aar.sh` builds Xray once into the Android project.

With `app/src-tauri/gen/android/keystore.properties` (`storeFile`, `storePassword`, `keyAlias`, `keyPassword`) the Android script builds release APKs signed with that key, and checks each one before publishing it: signed with that key, not debuggable, carrying both native libraries for its architecture. Without the file it builds debug APKs, named `-debug`, for testing.

### Releases

A tag `v<version>`, matching the version in `app/src-tauri/tauri.conf.json`, makes CI build every package with the same scripts and publish them as a GitHub release (`.github/workflows/release.yml`). A tag with a suffix, such as `v0.4.5-rc1`, makes a prerelease. The workflow needs the thirdweb client ID and the Android release key as repository secrets; it lists them at its top.

### Running from source

The app needs the daemon. On Linux, install a package once for the service, or run the daemon yourself as root:

```text
cargo build -p wanderer-daemon
scripts/fetch-xray.sh
sudo target/debug/wandererd --xray target/xray/xray
```

and then the app:

```text
cd app && npx tauri dev
```

### The leak test

`scripts/leak-test/run.sh` runs the daemon, Xray and the Network Guard for real against a node and an "internet" in network namespaces of their own, with no root and nothing changed outside the test, and checks that nothing leaves around the tunnel.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). To report a vulnerability, see [SECURITY.md](SECURITY.md); please do not open a public issue.

## License

Wanderer is licensed under the [Mozilla Public License 2.0](LICENSE). The packages also carry third-party software under its own terms; see [NOTICE](NOTICE).
