# Contributing

Thank you for helping. Wanderer stands between people and the network they
are trying to reach privately, so a change is held to a high bar: it has to
keep the tunnel tight on every platform, not only pass its own tests.

## Before you start

For anything larger than a fix, open an issue first and say what you intend.
It saves you writing something that does not fit, and us reviewing it.

To report a vulnerability, do **not** open an issue; see
[SECURITY.md](SECURITY.md).

## Building

See [Building](README.md#building) in the README. In short: Rust 1.87 or
newer, Node.js 22, Tauri's prerequisites, and a thirdweb client ID in
`app/.env` if you want to sign in.

## Before you open a pull request

Run what CI runs, and make it pass:

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cd app && npx tsc --noEmit && npx vite build
```

CI also runs clippy for Windows and for Android. If you change code under
`#[cfg(windows)]` or `#[cfg(target_os = "android")]`, check those targets too
(`cargo xwin clippy --target x86_64-pc-windows-msvc`, and the Android target
with the NDK from `scripts/android-env.sh`).

If your change touches the daemon, the Network Guard or the Xray
configuration, run the leak test as well:

```text
XRAY=target/xray/xray scripts/leak-test/run.sh
```

## What a change has to respect

- **Nothing leaves around the tunnel.** While connected, and while
  reconnecting, the Network Guard blocks everything that does not go through
  the tunnel, DNS and IPv6 included. A change that opens a path out needs a
  reason, a test, and the leak test passing.
- **The daemon does only what needs privileges.** Anything that can run as
  the user belongs in `crates/core` or the app, not in `wandererd`.
- **The app and the daemon agree through `crates/tunnel`.** A change to the
  control protocol bumps its version, so an old app and a new daemon refuse
  each other rather than misunderstand.
- **Everything about Singula goes through the SDK.** Proofs, permissions,
  Control Plane calls and contract reads come from
  [the Singula Rust SDK](https://github.com/SingulaTeam/rust-sdk), never
  reimplemented here.
- **No panics outside tests.** No `unwrap`, `expect` or `panic!`; return an
  error that says what happened.
- **Phones first.** Every screen has to work on a narrow phone in both
  languages before it works anywhere else.
- **Both languages.** Every string the UI shows exists in
  `app/src/i18n/en.ts` and `app/src/i18n/ru.ts`.
- **Comments say why.** The constraint, the failure being avoided, the
  platform's quirk — not a restatement of the code.

Note user-visible changes in [CHANGELOG.md](CHANGELOG.md) under
*Unreleased*.

## Pull requests

Keep a pull request to one change, and say in its description what it
changes, why, and how you checked it. The template asks for exactly that.

## License

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in Wanderer by you shall be licensed under the
Mozilla Public License 2.0, without any additional terms or conditions.
