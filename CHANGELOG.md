# Changelog

All notable changes to Wanderer are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/SingulaTeam/wanderer/compare/v0.4.4...HEAD
[0.4.4]: https://github.com/SingulaTeam/wanderer/releases/tag/v0.4.4
