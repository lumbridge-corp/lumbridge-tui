//! Pane launchers (`[[ui.launchers]]`): pill buttons on the focused or hovered
//! pane's top border. A click types the command into that pane; when an agent
//! already runs there, or on right-click, the pane is split and the command
//! goes into the new half. Either way it starts in the pane's directory and on
//! its machine.
//!
//! This uses only generation-1 endpoint features (`pane.split` plus ordinary
//! pane input), so it works against stock servers.

use super::*;
use crate::config::{PaneLauncherConfig, PaneLauncherStyle};

/// Border columns kept free for the pane title before buttons are dropped.
const TITLE_RESERVE: u16 = 14;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LauncherHit {
    pub(super) rect: Rect,
    pub(super) pane_id: String,
    pub(super) index: usize,
}

/// Draws as many launchers as fit, right-aligned, between `left` and `right`
/// (exclusive) on row `y`, and returns their hit areas.
#[allow(clippy::too_many_arguments)] // one call site; a struct would only rename the arguments
fn place_launchers(
    buffer: &mut Buffer,
    y: u16,
    left: u16,
    right: u16,
    pane_id: &str,
    launchers: &[PaneLauncherConfig],
    hovered: Option<&(String, usize)>,
    palette: &Palette,
) -> Vec<LauncherHit> {
    let style = crate::config::SidebarButtonStyle::Pill;
    let available = right.saturating_sub(left);
    let mut used = 0u16;
    let mut count = 0;
    for launcher in launchers {
        let gap = u16::from(count > 0);
        let width = render::button_width(&launcher.label, style);
        if used.saturating_add(gap).saturating_add(width) > available {
            break;
        }
        used = used.saturating_add(gap).saturating_add(width);
        count += 1;
    }
    let mut x = right.saturating_sub(used);
    let mut hits = Vec::with_capacity(count);
    for (index, launcher) in launchers.iter().enumerate().take(count) {
        let fg = match launcher.style {
            PaneLauncherStyle::Normal => palette.text,
            PaneLauncherStyle::Warn => palette.yellow,
        };
        let is_hovered = hovered.is_some_and(|(id, i)| id == pane_id && *i == index);
        let rect = render::put_button(
            buffer,
            x,
            y,
            &launcher.label,
            fg,
            style,
            is_hovered,
            false,
            palette,
        );
        x = rect.right().saturating_add(1);
        hits.push(LauncherHit {
            rect,
            pane_id: pane_id.to_owned(),
            index,
        });
    }
    hits
}

/// Launchers on the top border of each pane in `shown`. Panes without a top
/// border (a lone pane in `auto` border mode) are skipped; see
/// [`render_tab_bar_launchers`].
pub(super) fn render_pane_launchers(
    buffer: &mut Buffer,
    panes: &[PaneHit],
    shown: &[&str],
    launchers: &[PaneLauncherConfig],
    hovered: Option<&(String, usize)>,
    palette: &Palette,
) -> Vec<LauncherHit> {
    let mut hits = Vec::new();
    for pane in panes {
        if pane.popup || !has_top_border(pane) || !shown.contains(&pane.pane_id.as_str()) {
            continue;
        }
        let rect = pane.rect;
        // Keep the corner and one line cell on the right, and room for the title.
        let right = rect.right().saturating_sub(2);
        let left = rect
            .x
            .saturating_add(1)
            .saturating_add(TITLE_RESERVE.min(rect.width / 3));
        hits.extend(place_launchers(
            buffer,
            rect.y,
            left,
            right,
            &pane.pane_id,
            launchers,
            hovered,
            palette,
        ));
    }
    hits
}

/// Launchers in the free part of the tab bar, for a focused pane that has no
/// border to carry them.
pub(super) fn render_tab_bar_launchers(
    buffer: &mut Buffer,
    slot: Rect,
    pane_id: &str,
    launchers: &[PaneLauncherConfig],
    hovered: Option<&(String, usize)>,
    palette: &Palette,
) -> Vec<LauncherHit> {
    place_launchers(
        buffer,
        slot.y,
        slot.x,
        slot.right(),
        pane_id,
        launchers,
        hovered,
        palette,
    )
}

pub(super) fn has_top_border(pane: &PaneHit) -> bool {
    pane.inner_rect.y > pane.rect.y
}

/// Splits along the pane's longer side (cells are about twice as tall as they
/// are wide); `flip` takes the other side.
pub(super) fn launch_direction(rect: Rect, flip: bool) -> crate::api::schema::SplitDirection {
    let wide = u32::from(rect.width) >= u32::from(rect.height) * 2;
    if wide != flip {
        crate::api::schema::SplitDirection::Right
    } else {
        crate::api::schema::SplitDirection::Down
    }
}

/// The input that types `command` and presses Enter.
pub(super) fn launch_input(command: &str) -> Vec<crate::protocol::ClientPaneInputEvent> {
    let mut events = vec![crate::protocol::ClientPaneInputEvent::TextCommit(
        command.to_owned(),
    )];
    events.extend(crate::protocol::ClientPaneInputEvent::from_terminal_key(
        crate::input::TerminalKey::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        ),
    ));
    events
}

impl ClientShellState {
    /// Splits `pane_id` and, once the server answers with the new pane, types
    /// launcher `index`'s command into it.
    pub(super) fn launch_in_pane(
        &mut self,
        pane_id: String,
        index: usize,
        flip: bool,
        outcome: &mut ClientShellInput,
    ) {
        let Some(launcher) = self.config.launchers.get(index) else {
            return;
        };
        let command = launcher.command.clone();
        self.split_and_run(pane_id, command, flip, outcome);
    }

    /// Splits `pane_id` along its longer side (`flip`: the other) and types
    /// `command` into the new pane once the server reports it.
    pub(super) fn split_and_run(
        &mut self,
        pane_id: String,
        command: String,
        flip: bool,
        outcome: &mut ClientShellInput,
    ) {
        let direction = self
            .hits
            .panes
            .iter()
            .find(|hit| hit.pane_id == pane_id)
            .map_or(crate::api::schema::SplitDirection::Right, |hit| {
                launch_direction(hit.rect, flip)
            });
        self.push_endpoint_method_with_kind(
            crate::api::schema::Method::PaneSplit(crate::api::schema::PaneSplitParams {
                workspace_id: None,
                target_pane_id: Some(pane_id),
                direction,
                ratio: None,
                cwd: None,
                focus: true,
                right_click: crate::api::schema::PaneRightClickTarget::default(),
                env: std::collections::HashMap::new(),
            }),
            PendingEndpointKind::PaneLaunch { command },
            outcome,
        );
        outcome.repaint = true;
    }

    /// Runs launcher `index` in `pane_id` itself, unless an agent is already
    /// running there (typing would go to the agent): then it splits.
    pub(super) fn launch_here(
        &mut self,
        pane_id: String,
        index: usize,
        outcome: &mut ClientShellInput,
    ) {
        let Some(launcher) = self.config.launchers.get(index) else {
            return;
        };
        let busy = self
            .snapshot
            .as_deref()
            .is_some_and(|snapshot| snapshot.agents.iter().any(|agent| agent.pane_id == pane_id));
        if busy {
            self.launch_in_pane(pane_id, index, false, outcome);
            return;
        }
        let command = launcher.command.clone();
        if self.focused_pane_id().as_deref() != Some(pane_id.as_str()) {
            self.push_endpoint_method(
                crate::api::schema::Method::PaneFocus(crate::api::schema::PaneTarget {
                    pane_id: pane_id.clone(),
                }),
                outcome,
            );
        }
        for event in launch_input(&command) {
            super::push_target_event(ClientInputTarget::Pane(pane_id.clone()), event, outcome);
        }
        outcome.repaint = true;
    }

    /// Left-click runs in the pane (see [`Self::launch_here`]); right-click
    /// always splits. Returns whether the event hit a launcher.
    pub(super) fn route_launcher_click(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        outcome: &mut ClientShellInput,
    ) -> bool {
        use crossterm::event::{MouseButton, MouseEventKind};
        let split = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => false,
            MouseEventKind::Down(MouseButton::Right) => true,
            _ => return false,
        };
        let point = (mouse.column, mouse.row);
        let Some(hit) = self
            .hits
            .launchers
            .iter()
            .find(|hit| super::contains(hit.rect, point))
            .cloned()
        else {
            return false;
        };
        if split {
            self.launch_in_pane(hit.pane_id, hit.index, false, outcome);
        } else {
            self.launch_here(hit.pane_id, hit.index, outcome);
        }
        true
    }

    /// Tracks the pane under the mouse (its launchers show too) and the
    /// launcher under the mouse (it lights up).
    pub(super) fn update_launcher_hover(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        outcome: &mut ClientShellInput,
    ) {
        if self.config.launchers.is_empty() {
            return;
        }
        let point = (mouse.column, mouse.row);
        let pane = self
            .hits
            .panes
            .iter()
            .find(|hit| !hit.popup && super::contains(hit.rect, point))
            .map(|hit| hit.pane_id.clone());
        let launcher = self
            .hits
            .launchers
            .iter()
            .find(|hit| super::contains(hit.rect, point))
            .map(|hit| (hit.pane_id.clone(), hit.index));
        if pane != self.hover_pane_id || launcher != self.launcher_hover {
            self.hover_pane_id = pane;
            self.launcher_hover = launcher;
            outcome.repaint = true;
        }
    }

    /// Free tab bar columns between the last tab control and the status text.
    pub(super) fn tab_bar_launcher_slot(
        &self,
        tab_bar: Rect,
        snapshot: &crate::protocol::ClientShellSnapshot,
    ) -> Option<Rect> {
        if tab_bar.is_empty() {
            return None;
        }
        let left = self
            .hits
            .tabs
            .iter()
            .map(|(rect, _)| rect.right())
            .chain([
                self.hits.new_tab.right(),
                self.hits.tab_scroll_right.right(),
            ])
            .max()
            .unwrap_or(tab_bar.x)
            .max(tab_bar.x)
            .saturating_add(2);
        let status = render::tab_bar_status_width(snapshot);
        let right = tab_bar
            .right()
            .saturating_sub(status)
            .saturating_sub(u16::from(status > 0));
        (right > left).then(|| Rect::new(left, tab_bar.y, right - left, 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launcher(label: &str, style: PaneLauncherStyle) -> PaneLauncherConfig {
        PaneLauncherConfig {
            label: label.to_owned(),
            command: format!("{label} --go"),
            style,
        }
    }

    fn pane(rect: Rect, inner: Rect) -> PaneHit {
        PaneHit {
            rect,
            inner_rect: inner,
            scrollbar_rect: None,
            scroll: None,
            pane_id: "p1".to_owned(),
            popup: false,
            mouse_reporting: false,
            sgr_pixel_mouse: false,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn row_text(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol().to_owned())
            .collect()
    }

    #[test]
    fn draws_right_aligned_on_the_top_border_and_drops_what_does_not_fit() {
        let palette = Palette::catppuccin();
        let launchers = [
            launcher("claude!", PaneLauncherStyle::Warn),
            launcher("codex", PaneLauncherStyle::Normal),
            launcher("a-very-long-one", PaneLauncherStyle::Normal),
        ];
        let rect = Rect::new(0, 0, 40, 10);
        let panes = [pane(rect, Rect::new(1, 1, 38, 8))];
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 10));
        let hits = render_pane_launchers(&mut buffer, &panes, &["p1"], &launchers, None, &palette);
        assert_eq!(hits.len(), 2, "the third does not fit beside the title");
        let row = row_text(&buffer, 0);
        assert!(row.contains(" claude!   codex "), "{row:?}");
        assert_eq!(hits[1].rect.right(), 38);
        assert_eq!(buffer[(hits[0].rect.x + 1, 0)].fg, palette.yellow);
    }

    #[test]
    fn skips_panes_without_a_top_border_or_not_shown() {
        let palette = Palette::catppuccin();
        let launchers = [launcher("codex", PaneLauncherStyle::Normal)];
        let rect = Rect::new(0, 0, 40, 10);
        let mut buffer = Buffer::empty(rect);
        let borderless = [pane(rect, rect)];
        assert!(render_pane_launchers(
            &mut buffer,
            &borderless,
            &["p1"],
            &launchers,
            None,
            &palette
        )
        .is_empty());
        let bordered = [pane(rect, Rect::new(1, 1, 38, 8))];
        assert!(
            render_pane_launchers(&mut buffer, &bordered, &["p2"], &launchers, None, &palette)
                .is_empty()
        );
    }

    #[test]
    fn splits_along_the_longer_side() {
        use crate::api::schema::SplitDirection;
        assert_eq!(
            launch_direction(Rect::new(0, 0, 160, 40), false),
            SplitDirection::Right
        );
        assert_eq!(
            launch_direction(Rect::new(0, 0, 60, 40), false),
            SplitDirection::Down
        );
        assert_eq!(
            launch_direction(Rect::new(0, 0, 160, 40), true),
            SplitDirection::Down
        );
    }

    #[test]
    fn launch_input_types_the_command_then_enter() {
        let events = launch_input("codex");
        assert_eq!(
            events[0],
            crate::protocol::ClientPaneInputEvent::TextCommit("codex".to_owned())
        );
        assert_eq!(events.len(), 2);
    }
}
