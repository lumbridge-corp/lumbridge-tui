# Changelog

Changes to Lumbridge TUI.

## Unreleased

## [0.1.0] - 2026-11-11

The first open-source release of Lumbridge TUI. It is built from source, on Linux and macOS.
`lumbridge-tui --version` prints 0.9.5, the package version.

- Package and binary `lumbridge-tui`, folders `~/.config/lumbridge-tui` and
  `~/.local/state/lumbridge-tui`, environment variables `LUMBRIDGE_TUI_*`, socket API schema
  `docs/api/lumbridge-api.schema.json`, skill `skills/lumbridge-tui`.
- The notes editor is Lumbridge Notes, a git submodule at `crates/lumbridge-notes`
  (binary `lumbridge-notes`), pinned to Lumbridge Notes v0.1.0.
- The Home drawer has only what ships with Lumbridge TUI: Notes, Fleet and Palette. Add programs of your own
  as `[[ui.launchers]]` buttons in your config.
- The fleet feeder is `contrib/lumbridge/lumbridge-fleet`; it reads `~/.config/lumbridge-tui/fleet.toml`.
  `contrib/lumbridge/fleet.toml` is an example with made-up machines.
- No network calls to any release or update service: no self-update, no background version
  checks, no release-notes or announcement fetch, no hosted agent-detection catalog, and no
  release downloads for remote installs. `lumbridge-tui update` only explains how to rebuild.
- The config warning banner drops its start, not its end, when it is wider than
  the screen, so `lumbridge-tui config check` stays readable at 80 columns.
- Help text, `Cargo.toml` and the docs point at github.com/lumbridge-corp/lumbridge-tui.
- `SECURITY.md`, a public `CONTRIBUTING.md`, and a quick start from a clean Ubuntu 24.04 machine in the README.
- `scripts/scrub-check` fails on private addresses, home folders, email addresses and token formats in the tree.
