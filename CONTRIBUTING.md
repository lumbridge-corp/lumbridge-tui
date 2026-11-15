# Contributing to Lumbridge TUI

Thank you for looking. Lumbridge TUI is at v0.1 and maintained by a small team, so please read this first.

## Before you start

- **Bugs:** open an issue. Say what you did, what happened, what you expected, your OS and terminal, and the
  output of `lumbridge-tui --version` or the commit you built.
- **Security problems:** do not open an issue. See [SECURITY.md](SECURITY.md).
- **Small fixes** (typos, clear bugs, docs): send a pull request.
- **Larger changes** (new features, new settings, changes to the socket API or a file format): open an issue
  first and wait for a reply. We say no to many features to keep the program small. It is better to hear that
  before you write the code.

## Setup

Install what the README's quick start lists (git, a C toolchain, Rust as pinned in `rust-toolchain.toml`,
Zig 0.16.0), then:

```sh
git submodule update --init --depth 1   # crates/lumbridge-notes, a workspace member
rustup component add rustfmt clippy
cargo install --locked cargo-nextest
```

Python 3 runs the maintenance tests.

## Checks

```sh
scripts/ci.sh
```

That is what CI runs: `cargo fmt --check`, clippy with warnings as errors, the whole test suite with
cargo-nextest, and the maintenance tests in `scripts/`. Tests run in a throwaway home; they never touch your
`~/.config/lumbridge-tui` or a running server. Run it before you open a pull request. CI must pass before review.

Before a pull request, also run `scripts/scrub-check`. It fails on private network addresses, home folders,
email addresses and token formats that are not listed in `.scrub-allow`. Use made-up examples: user `sam`,
host `desk`, `example.com`, `192.0.2.x`.

## How the code is built

- **State is separate from runtime.** `AppState` is plain data, testable without PTYs or async. Workspace
  logic does not need real terminals.
- **Render is pure.** `compute_view()` handles geometry; `render()` only draws. Never change state while
  rendering.
- **Platform code is isolated.** OS-specific code lives in `src/platform/<os>.rs`; shared code has no
  `#[cfg(target_os)]`.
- **Detection reads a screen snapshot.** It never touches the parser or the viewport. When changing
  `src/detect/manifests/`, capture the real screen first (`lumbridge-tui agent read <pane> --source detection`).
- **Hot paths multiply.** Work reachable from rendering, layout, PTY parsing, detection or client frames runs
  per pane, per tab and per client. Do no file I/O, process-tree walks or large allocations there.
- **Reuse the UI.** New dialogs and screens follow the existing ones (mouse first, same close actions).

## What we look for

- One change per pull request. Small is good.
- Tests for behaviour you change or add.
- Docs in [docs/](docs/) updated in the same pull request when behaviour changes.
- A line in [CHANGELOG.md](CHANGELOG.md) under "Unreleased" for anything a user would notice.
- No new network calls, telemetry or update checks.
- No new dependency without a reason in the pull request text.

## Never commit

- Passwords, tokens, keys, or real `.env` files.
- Real names, email addresses, or messages of real people. Use made-up data: `example.com`, `.example`, `.test`.
- Private host names, private network addresses or internal links.
- Screenshots that show any of the above.

## Third-party code

- Say so in the pull request if any part of your change is copied or adapted from somewhere else, and give the
  source and its licence.
- Do not edit `LICENSE` or the licence files of the vendored code in `vendor/` and `packaging/windows/licenses`.
- Do not paste code written by a tool unless you have read it, tested it, and can explain it. You are
  responsible for what you send.

## Licence of your work

By sending a contribution you agree that it is licensed under Apache-2.0, like the rest of the project.

## Conduct

Be kind and direct. Argue about the work, not the person.
