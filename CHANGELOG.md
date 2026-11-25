# Changelog

All notable changes to Wanderer are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

- `url` 2.5.8 → 2.5.9 (RUSTSEC-2026-0592), through the SDK's HTTP client.
- `cookie` 0.18.2 → 0.18.3 (RUSTSEC-2026-0594), through Tauri.

### Added

- A DNS setting, in the advanced mode: Cloudflare, Google, Quad9 or a
  resolver of one's own, by address, over DNS-over-HTTPS (`https://`) or
  plain DNS. Queries go through the tunnel either way; over plain DNS the
  exit node sees each name, and the app says so. Cloudflare stays the
  default.

### Fixed

- A connection of one hop to a country reaches that country's nodes wherever
  they are connected. Wanderer asked the fastest Control Plane for a node in
  the country, and a session on a right opens only at the Control Plane its
  node is connected to, so a country whose nodes were on another Control
  Plane was refused as having none. Wanderer now chooses the node and its
  Control Plane together from every Control Plane's list, as it already did
  for routes of several hops; the same goes for the node chosen after one
  failed.
- Falling back to another cover now reaches it. A node without a reverse
  proxy serves each cover on a port of its own, and Wanderer tried every
  cover on the port of the one the node chose, where the others are not
  served. Each cover is tried on the port the node names for it (Singula Rust
  SDK 0.2.3); a node that names none is tried on its chosen cover only.

## [0.5.0] - 2026-11-21

### Security

- `rustix` 1.1.5 → 1.1.6 (RUSTSEC-2026-0486), in the service and through
  Tauri's single-instance plugin.

### Changed

- Connections run on the Singula network's shared journal (Singula Rust SDK
  0.2.2): each connection of the subscription proves its day once and is kept
  by pings the Control Planes record together. Connections no longer wait
  for a session that just ended to let go.
- When the network closes a connection, Wanderer reconnects on its own and,
  once connected again, says what happened in a banner on the home screen:
  the node stopped counting time (Wanderer moves to another node), the
  connection stopped reaching the network, its details were used from
  several places, or another of the person's devices held that connection
  of the subscription. Routine closes, such as after a long idle, are only
  logged.
- When every connection of the subscription is held by the person's other
  devices, Wanderer stops and says so instead of taking one from them.
- A route needs one connection of the subscription per hop; a route longer
  than the subscription allows is refused with that reason.
- CI runs the tests that need a real Xray and nftables, and the leak test;
  a release is published only when every check passes on the tagged commit.

## [0.4.4] - 2026-11-06

### Changed

- The price of each Orbit size is read from the subscription contract
  (`mintCost`), so a running promotion's discount is shown and paid.
- Built on the published Singula Rust SDK, 0.1.0, instead of a local copy.
- Android: one APK per architecture — arm64, armv7 and x86_64 — instead of
  arm64 alone. With a release key (`keystore.properties`) they are release
  builds signed with it, each checked before it is published; without one,
  debug builds named as such.

### Added

- The repository is public, under the Mozilla Public License 2.0.

## Earlier versions

0.1.0 to 0.4.3 were released before the repository was public.

[Unreleased]: https://github.com/SingulaTeam/wanderer/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/SingulaTeam/wanderer/compare/v0.4.4...v0.5.0
[0.4.4]: https://github.com/SingulaTeam/wanderer/releases/tag/v0.4.4
