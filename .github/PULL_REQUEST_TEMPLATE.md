<!--
Thanks for the pull request. A reviewer reads this before the code, so say
what they need to know. Delete any section that does not apply.

A vulnerability fix does not belong here: see SECURITY.md.
-->

## What changes

<!-- One or two sentences. What can someone do after this that they could not before, or what now behaves differently? -->

## Why

<!-- The problem, or the issue this closes: "Closes #123". -->

## How it was checked

<!-- Tests added or changed, and what you ran by hand: which platforms, the leak test, a real connection. -->

## The tunnel

<!--
Does this touch the daemon, the Network Guard, the Xray configuration or the
Android VPN service? If so: on which platforms you checked that nothing
leaves around the tunnel, while connected and while reconnecting.
-->

- [ ] Does not touch the tunnel or the guard
- [ ] Touches them; checked on: <!-- Linux, Windows, Android; the leak test -->

## Checklist

- [ ] `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass
- [ ] `cargo test --workspace` and `cargo deny check` pass
- [ ] `npx tsc --noEmit` passes in `app/`
- [ ] New UI text is in both `en.ts` and `ru.ts`, and the screen works on a narrow phone
- [ ] `CHANGELOG.md` has an entry under *Unreleased*, if users would notice the change
