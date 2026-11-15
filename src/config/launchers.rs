//! Pane launchers: buttons on the focused pane's border that open a new pane
//! running a command (`[[ui.launchers]]`).

use serde::Deserialize;

/// Most launchers kept; more would not fit on a pane border anyway.
pub const MAX_PANE_LAUNCHERS: usize = 8;
const MAX_LABEL_WIDTH: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneLauncherStyle {
    #[default]
    Normal,
    /// Drawn in the warning colour, for commands that skip safety prompts.
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct PaneLauncherConfig {
    pub label: String,
    pub command: String,
    pub style: PaneLauncherStyle,
}

/// Drops entries without a label or command, strips control characters and
/// newlines (a newline in `command` would run a second command), and caps the
/// list.
pub fn sanitized_launchers(launchers: &[PaneLauncherConfig]) -> Vec<PaneLauncherConfig> {
    launchers
        .iter()
        .filter_map(|launcher| {
            let label: String = launcher
                .label
                .chars()
                .filter(|ch| !ch.is_control())
                .take(MAX_LABEL_WIDTH)
                .collect();
            let command: String = launcher
                .command
                .chars()
                .filter(|ch| !ch.is_control())
                .collect();
            let label = label.trim().to_owned();
            let command = command.trim().to_owned();
            (!label.is_empty() && !command.is_empty()).then_some(PaneLauncherConfig {
                label,
                command,
                style: launcher.style,
            })
        })
        .take(MAX_PANE_LAUNCHERS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_launchers_and_drops_unusable_entries() {
        let config: crate::config::Config = toml::from_str(
            r#"
[[ui.launchers]]
label = "claude!"
command = "claude --dangerously-skip-permissions"
style = "warn"

[[ui.launchers]]
label = "codex"
command = "codex\nrm -rf ~"

[[ui.launchers]]
label = ""
command = "nothing"
"#,
        )
        .unwrap();
        let launchers = sanitized_launchers(&config.ui.launchers);
        assert_eq!(launchers.len(), 2);
        assert_eq!(launchers[0].style, PaneLauncherStyle::Warn);
        assert_eq!(launchers[1].command, "codexrm -rf ~");
        assert!(crate::config::Config::default().ui.launchers.is_empty());
    }
}
