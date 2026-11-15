# Lumbridge TUI

`lumbridge-tui` is a terminal workspace manager for AI coding agents: one Rust
binary that keeps agent terminals running in a background server, shows which
pane is working, blocked or idle, and lets agents drive panes, tabs and
workspaces through a CLI and a socket API. It runs inside any terminal.

It is part of [Lumbridge](https://lumbridgecorp.com), the open engine for agentic organizations
([all Lumbridge repositories](https://github.com/lumbridge-corp)).
Lumbridge TUI has its own server; it does not need Lumbridge Core.

- **Detach without stopping work.** Terminals keep running in a background
  server when the client closes or SSH drops. After a restart the saved layout
  comes back and supported agent sessions can resume; the processes
  themselves do not survive. See [docs/session-state.mdx](docs/session-state.mdx).
- **Several machines, one window.** Local work and saved SSH machines side by
  side. See [docs/connecting-machines.mdx](docs/connecting-machines.mdx).
- **Agent state.** Every pane is marked working, blocked or idle.
- **Agent-native.** Agents use the CLI and the socket API to spawn panes,
  prompt each other and wait for each other. See
  [docs/socket-api.mdx](docs/socket-api.mdx) and the skill in
  [skills/lumbridge-tui/SKILL.md](skills/lumbridge-tui/SKILL.md).
- **Keyboard and mouse.** tmux-style prefix keys (`ctrl+b`) and click, drag,
  split.
- **Notebook drawer** backed by [Lumbridge Notes](https://github.com/lumbridge-corp/lumbridge-notes)
  (the `crates/lumbridge-notes` submodule), and an optional machine sidebar fed by
  [contrib/lumbridge](contrib/lumbridge) (an example config with made-up machines).

## State

- Release v0.1.0. You build it from source. There is no installer, no download and no updater.
- Linux and macOS, on x86_64 and aarch64. The Windows code is kept, but not tested.
- Lumbridge TUI never checks for updates and sends no telemetry.
- `lumbridge-tui --version` prints 0.9.5. That is the version of the code line it continues; release v0.1.0 is this tree.

## Quick start

You need git, a C toolchain, Rust (the release pinned in `rust-toolchain.toml`, 1.96.1) and Zig 0.16.0
(exactly that release; it builds the vendored libghostty-vt).

On Ubuntu 24.04:

```sh
sudo apt-get update
sudo apt-get install -y git curl ca-certificates build-essential xz-utils

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.96.1
. "$HOME/.cargo/env"

# Zig 0.16.0 (on aarch64, use zig-aarch64-linux-0.16.0)
curl -fsSLO https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz
mkdir -p "$HOME/.local/bin"
tar -xf zig-x86_64-linux-0.16.0.tar.xz -C "$HOME/.local"
ln -sf "$HOME/.local/zig-x86_64-linux-0.16.0/zig" "$HOME/.local/bin/zig"
export PATH="$HOME/.local/bin:$PATH"
```

On macOS, install the Xcode Command Line Tools (`xcode-select --install`), Rust from <https://rustup.rs>,
and Zig 0.16.0 from <https://ziglang.org/download/>. Put `zig` and `~/.local/bin` on your PATH.

Get the source and build it. The notes editor is a git submodule, so clone with submodules.
The first build takes several minutes and downloads the Rust crates and Zig packages it needs:

```sh
git clone --recurse-submodules --shallow-submodules https://github.com/lumbridge-corp/lumbridge-tui.git
cd lumbridge-tui
cargo build --release --locked
install -D -m 755 target/release/lumbridge-tui "$HOME/.local/bin/lumbridge-tui"
cargo install --locked --path crates/lumbridge-notes --root "$HOME/.local"
```

The last line installs the notes editor (`lumbridge-notes`); it is optional.
Set `ZIG=/path/to/zig` when Zig 0.16.0 is not the `zig` on `PATH`.
A Nix flake is included too (`nix build '.?submodules=1'`).

Check it:

```sh
lumbridge-tui --version
lumbridge-notes --version
```

Then start it in a terminal, where the work lives:

```text
lumbridge-tui
```

`ctrl+b q` detaches, `lumbridge-tui` reattaches. `lumbridge-tui server stop` stops the server.
See [docs/quick-start.mdx](docs/quick-start.mdx) for a first session.

To remove it: `lumbridge-tui server stop`, delete `~/.local/bin/lumbridge-tui` and `~/.local/bin/lumbridge-notes`,
and the folders below.

## Folders

| What | Path |
|---|---|
| Config | `~/.config/lumbridge-tui/config.toml` (`$XDG_CONFIG_HOME`, or `LUMBRIDGE_TUI_CONFIG_PATH`) |
| Sockets, sessions, plugins | `~/.config/lumbridge-tui/` |
| State | `~/.local/state/lumbridge-tui/` (`$XDG_STATE_HOME`) |
| Worktrees | `~/.lumbridge-tui/worktrees` |

Debug builds use `lumbridge-tui-dev` instead.

## Your own tools

The Home drawer (the "⌂ Lumbridge" button) has the notebook, the machine overview
(`lumbridge-fleet`, when it is on `PATH`) and the command palette. Programs of your own go in
your config as buttons on the pane border, not in the source:

```toml
[[ui.launchers]]
label = "logs"
command = "journalctl --user -f"
```

See [docs/configuration.mdx](docs/configuration.mdx).

## Docs

Technical docs are in [docs/](docs/): [install](docs/install.mdx) · [quick start](docs/quick-start.mdx) ·
[concepts](docs/concepts.mdx) · [agents](docs/agents.mdx) ·
[keyboard](docs/keyboard.mdx) · [configuration](docs/configuration.mdx) ·
[config reference](docs/config-reference.mdx) ·
[session state](docs/session-state.mdx) ·
[connecting machines](docs/connecting-machines.mdx) ·
[remote](docs/persistence-remote.mdx) · [integrations](docs/integrations.mdx) ·
[add Lumbridge TUI support to an agent](docs/add-lumbridge-support.mdx) ·
[plugins](docs/plugins.mdx) · [socket API](docs/socket-api.mdx) ·
[CLI reference](docs/cli-reference.mdx) ·
[troubleshooting](docs/troubleshooting.mdx). The socket API schema is
[docs/api/lumbridge-api.schema.json](docs/api/lumbridge-api.schema.json).

## Work on the code

The checks are [scripts/ci.sh](scripts/ci.sh): format, clippy, the whole test suite (with
[cargo-nextest](https://nexte.st/)) and the maintenance tests. CI runs the same script.

```sh
git submodule update --init --depth 1   # if you cloned without submodules
cargo install --locked cargo-nextest
scripts/ci.sh
```

[just](https://just.systems/) has shortcuts: `just lint`, `just test`.
`scripts/scrub-check` looks for private addresses, home folders and tokens before anything is published.

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup and checks, and [SECURITY.md](SECURITY.md) to report a vulnerability.

## License

Lumbridge TUI started as a fork of [Herdr](https://github.com/herdrdev/herdr) (Apache-2.0). Thanks to its authors.

[Apache-2.0](LICENSE). Copyright 2026 Lumbridge Corp for its own work.
Code from other projects keeps its authors' copyright and notices: the vendored
[libghostty-vt](vendor/libghostty-vt) and [portable-pty](vendor/portable-pty) carry their own licence files,
and so does the Windows ConPTY package in [packaging/windows/licenses](packaging/windows/licenses).
