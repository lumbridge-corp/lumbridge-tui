# Security

Report security bugs by email to security@lumbridgecorp.com. Do not open a public issue with exploit details.

Include your `lumbridge-tui --version`, OS, whether the problem is local or over SSH, steps to reproduce, and the
impact you expect. Remove secrets from logs and examples. A small reproduction helps.

The latest release and the latest `main` are supported. We answer in the same email thread. There is no fixed
response deadline.

## Trust model

- The server and its socket belong to your user account. Any program running as you can use the socket API and
  the CLI: read pane output, type into panes, start programs. That is by design; it is how agents drive panes.
- Remote machines are reached over your own SSH. Lumbridge TUI uses your SSH configuration and keys and does not
  relax host-key checks.
- Plugins and agent integrations you install run as you. They are not sandboxed. Read them first.
- Lumbridge TUI never checks for updates and sends no telemetry.
