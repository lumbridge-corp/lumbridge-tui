//! Machine detail drawer: opens beside the sidebar with one machine's health
//! history, services, alerts and agents.
//!
//! Everything shown comes from data the client already has: the machine
//! feeder (tokens, `series`, services, `alerts`) and the endpoint snapshot.

use super::render::{display_width, put_right_text, put_text};
use super::*;
use crossterm::event::{MouseButton, MouseEventKind};

const DRAWER_WIDTH: u16 = 60;
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Series drawn first, in this order; any others follow alphabetically.
const PREFERRED_SERIES: [&str; 3] = ["load", "mem", "disk"];

#[derive(Debug)]
pub(super) struct ClientMachineDrawer {
    pub(super) endpoint_id: ClientEndpointId,
    /// Screen row of the machine in the sidebar; the drawer aligns to it.
    pub(super) anchor_y: u16,
    pub(super) scroll: usize,
}

pub(super) struct DrawerRender {
    pub(super) area: Rect,
    pub(super) agents: Vec<(Rect, String)>,
    pub(super) switch_button: Rect,
    pub(super) close_button: Rect,
}

/// Draws `points` as a sparkline `width` cells wide (newest on the right).
///
/// Percent series run up to 100 from the ten below their lowest decade, so a
/// disk at 80–91% shows its climb instead of a flat bar; others scale from 0
/// to their own peak (at least 1, so a load of 0.2 never looks full).
pub(super) fn sparkline(points: &[f64], width: usize, percent: bool) -> String {
    if points.is_empty() || width == 0 {
        return String::new();
    }
    let points = &points[points.len().saturating_sub(width)..];
    let (bottom, top) = if percent {
        let low = points.iter().copied().fold(f64::INFINITY, f64::min);
        (((low / 10.0).floor() * 10.0 - 10.0).max(0.0), 100.0)
    } else {
        (0.0, points.iter().copied().fold(1.0_f64, f64::max))
    };
    points
        .iter()
        .map(|value| {
            let level =
                (((value - bottom) / (top - bottom)).clamp(0.0, 1.0) * 7.0).round() as usize;
            SPARK[level.min(7)]
        })
        .collect()
}

/// One drawn line: spans plus, for agent lines, the pane to focus on click.
struct DrawerLine {
    spans: Vec<(String, Style)>,
    agent: Option<String>,
}

impl DrawerLine {
    fn new(spans: Vec<(String, Style)>) -> Self {
        Self { spans, agent: None }
    }
}

fn section(title: &str, palette: &Palette) -> DrawerLine {
    DrawerLine::new(vec![(
        title.to_owned(),
        Style::default()
            .fg(palette.accent)
            .add_modifier(Modifier::BOLD),
    )])
}

#[allow(clippy::too_many_arguments)] // one renderer reading several client sources
fn drawer_lines(
    endpoint: &ClientShellEndpoint,
    feed: Option<&crate::client::machine_feed::MachineFeedEntry>,
    clock: &super::agent_clock::AgentClock,
    now: std::time::Instant,
    width: usize,
    config: &ClientShellConfig,
) -> Vec<DrawerLine> {
    use crate::client::machine_feed::ServiceState;
    let palette = &config.palette;
    let dim = Style::default().fg(palette.overlay0);
    let text = Style::default().fg(palette.text);
    let mut lines = Vec::new();

    if let Some(feed) = feed {
        let mut names = feed.series.keys().cloned().collect::<Vec<_>>();
        names.sort_by_key(|name| {
            (
                PREFERRED_SERIES
                    .iter()
                    .position(|preferred| preferred == name)
                    .unwrap_or(PREFERRED_SERIES.len()),
                name.clone(),
            )
        });
        if !names.is_empty() {
            lines.push(section("health · last hour", palette));
            for name in names {
                let points = &feed.series[&name];
                let current = feed.tokens.get(&name).cloned().unwrap_or_else(|| {
                    points
                        .last()
                        .map(|value| format!("{value:.1}"))
                        .unwrap_or_default()
                });
                let alarm = current.contains('!');
                let percent = current.contains('%');
                let spark_width = width.saturating_sub(15);
                let color = if alarm { palette.red } else { palette.teal };
                lines.push(DrawerLine::new(vec![
                    (format!("{name:<6}"), dim),
                    (
                        format!("{current:<7}"),
                        Style::default()
                            .fg(if alarm { palette.red } else { palette.text })
                            .add_modifier(Modifier::BOLD),
                    ),
                    (
                        sparkline(points, spark_width, percent),
                        Style::default().fg(color),
                    ),
                ]));
            }
            lines.push(DrawerLine::new(Vec::new()));
        }

        if !feed.services.is_empty() {
            let down = feed
                .services
                .iter()
                .filter(|service| service.state == ServiceState::Down)
                .count();
            lines.push(section(
                &if down > 0 {
                    format!("services · {down} down")
                } else {
                    "services".to_owned()
                },
                palette,
            ));
            let mut spans = Vec::new();
            let mut used = 0;
            for service in &feed.services {
                let (glyph, color) = match service.state {
                    ServiceState::Up => ("●", palette.green),
                    ServiceState::Down => ("●", palette.red),
                    ServiceState::Unknown => ("○", palette.overlay0),
                };
                let name = if service.name.is_empty() {
                    "service"
                } else {
                    service.name.as_str()
                };
                let cell = display_width(name) as usize + 4;
                if used > 0 && used + cell > width {
                    lines.push(DrawerLine::new(std::mem::take(&mut spans)));
                    used = 0;
                }
                spans.push((format!("{glyph} "), Style::default().fg(color)));
                spans.push((format!("{name}  "), text));
                used += cell;
            }
            if !spans.is_empty() {
                lines.push(DrawerLine::new(spans));
            }
            lines.push(DrawerLine::new(Vec::new()));
        }

        if !feed.alerts.is_empty() {
            lines.push(section(&format!("alerts · {}", feed.alerts.len()), palette));
            for alert in &feed.alerts {
                let critical = alert.severity == "critical";
                lines.push(DrawerLine::new(vec![
                    (
                        if critical { "▲ " } else { "△ " }.to_owned(),
                        Style::default().fg(if critical {
                            palette.red
                        } else {
                            palette.yellow
                        }),
                    ),
                    (
                        alert.name.clone(),
                        Style::default()
                            .fg(palette.text)
                            .add_modifier(Modifier::BOLD),
                    ),
                    (format!("  {}", alert.summary), dim),
                ]));
            }
            lines.push(DrawerLine::new(Vec::new()));
        }
    }

    if let Some(snapshot) = endpoint.snapshot.as_deref() {
        let mut agents = snapshot.agents.iter().collect::<Vec<_>>();
        agents.sort_by_key(|agent| {
            (
                std::cmp::Reverse(status_priority(agent.agent_status)),
                std::cmp::Reverse(agent.state_change_seq),
            )
        });
        if !agents.is_empty() {
            lines.push(section(&format!("agents · {}", agents.len()), palette));
        }
        for agent in agents {
            let agent_label = agent
                .display_agent
                .as_deref()
                .or(agent.name.as_deref())
                .or(agent.agent.as_deref());
            let task = crate::ui::meaningful_task_title(
                agent.terminal_title_stripped.as_deref(),
                agent_label,
            )
            .or(agent_label)
            .unwrap_or("agent");
            let workspace = snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == agent.workspace_id)
                .map(|workspace| workspace.label.as_str())
                .unwrap_or_default();
            let elapsed = clock
                .elapsed(&endpoint.endpoint_id, &agent.pane_id, now)
                .map(|elapsed| format!(" {elapsed}"))
                .unwrap_or_default();
            lines.push(DrawerLine {
                spans: vec![
                    (
                        format!(
                            "{} ",
                            status_icon(agent.agent_status, config.status_indicators)
                        ),
                        Style::default().fg(status_color(agent.agent_status, palette)),
                    ),
                    (
                        format!(
                            "{:<12}",
                            format!("{}{elapsed}", status_text(agent.agent_status))
                        ),
                        Style::default().fg(status_color(agent.agent_status, palette)),
                    ),
                    (task.to_owned(), text),
                    (format!("  {workspace}"), dim),
                ],
                agent: Some(agent.pane_id.clone()),
            });
        }
    }
    if lines.is_empty() {
        lines.push(DrawerLine::new(vec![(
            "no feeder data or agents for this machine".to_owned(),
            dim,
        )]));
    }
    while lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.pop();
    }
    lines
}

#[allow(clippy::too_many_arguments)] // one renderer reading several client sources
pub(super) fn render_machine_drawer(
    buffer: &mut Buffer,
    drawer: &mut ClientMachineDrawer,
    endpoint: &ClientShellEndpoint,
    feed: Option<&crate::client::machine_feed::MachineFeedEntry>,
    clock: &super::agent_clock::AgentClock,
    left: u16,
    config: &ClientShellConfig,
) -> Option<DrawerRender> {
    let palette = &config.palette;
    let screen = buffer.area;
    let width = DRAWER_WIDTH.min(screen.right().saturating_sub(left).saturating_sub(1));
    if width < 30 || screen.height < 10 {
        return None;
    }
    let inner_width = usize::from(width - 4);
    let lines = drawer_lines(
        endpoint,
        feed,
        clock,
        std::time::Instant::now(),
        inner_width,
        config,
    );
    // Border, title, rule, body, rule, footer.
    let chrome = 6;
    let height = (lines.len() as u16 + chrome).min(screen.height.saturating_sub(2));
    let y = drawer
        .anchor_y
        .saturating_sub(1)
        .min(screen.bottom().saturating_sub(height + 1))
        .max(screen.y + 1);
    let area = Rect::new(left, y, width, height);
    let background = Style::default().bg(palette.panel_bg).fg(palette.text);
    for row in area.y..area.bottom() {
        for column in area.x..area.right() {
            buffer[(column, row)].set_symbol(" ").set_style(background);
        }
    }
    {
        use ratatui::widgets::{Block, BorderType, Widget};
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(palette.accent).bg(palette.panel_bg))
            .render(area, buffer);
    }
    let inner = Rect::new(area.x + 2, area.y + 1, area.width - 4, area.height - 2);
    let on_panel = |style: Style| style.bg(palette.panel_bg);

    let online = endpoint.status == ClientEndpointStatus::Online || endpoint.endpoint_id.is_local();
    put_text(
        buffer,
        inner.x,
        inner.y,
        inner.width,
        &endpoint.label,
        on_panel(
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        ),
    );
    let close_button = Rect::new(inner.right().saturating_sub(1), inner.y, 1, 1);
    put_text(
        buffer,
        close_button.x,
        close_button.y,
        1,
        "×",
        on_panel(Style::default().fg(palette.overlay1)),
    );
    let status = if online { "online" } else { "offline" };
    put_text(
        buffer,
        inner.x + display_width(&endpoint.label) + 2,
        inner.y,
        inner
            .width
            .saturating_sub(display_width(&endpoint.label) + 4),
        status,
        on_panel(Style::default().fg(if online {
            palette.green
        } else {
            palette.overlay0
        })),
    );
    let rule = "─".repeat(usize::from(inner.width));
    let rule_style = on_panel(Style::default().fg(palette.surface1));
    put_text(buffer, inner.x, inner.y + 1, inner.width, &rule, rule_style);

    let body = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(4),
    );
    let visible = usize::from(body.height);
    drawer.scroll = drawer.scroll.min(lines.len().saturating_sub(visible));
    let mut agents = Vec::new();
    for (offset, line) in lines.iter().skip(drawer.scroll).take(visible).enumerate() {
        let y = body.y + offset as u16;
        let mut x = body.x;
        for (text, style) in &line.spans {
            if x >= body.right() {
                break;
            }
            let room = body.right() - x;
            let shown = if display_width(text) <= room {
                text.clone()
            } else {
                crate::ui::truncate_end(text, usize::from(room))
            };
            put_text(buffer, x, y, room, &shown, on_panel(*style));
            x += display_width(&shown);
        }
        if let Some(pane_id) = &line.agent {
            agents.push((Rect::new(area.x + 1, y, area.width - 2, 1), pane_id.clone()));
        }
    }
    if lines.len() > visible {
        let more = format!(
            "{}–{} of {}",
            drawer.scroll + 1,
            (drawer.scroll + visible).min(lines.len()),
            lines.len()
        );
        put_right_text(
            buffer,
            inner,
            inner.y,
            &more,
            on_panel(Style::default().fg(palette.overlay0)),
        );
    }

    let footer_y = inner.bottom().saturating_sub(1);
    put_text(
        buffer,
        inner.x,
        footer_y - 1,
        inner.width,
        &rule,
        rule_style,
    );
    let switch_label = format!("switch to {}", endpoint.label);
    let switch_button = if endpoint.endpoint_id.is_local() || online {
        super::render::put_button(
            buffer,
            inner.x,
            footer_y,
            &switch_label,
            palette.subtext0,
            crate::config::SidebarButtonStyle::Pill,
            false,
            false,
            palette,
        )
    } else {
        Rect::default()
    };
    put_right_text(
        buffer,
        inner,
        footer_y,
        "enter switch · esc close",
        on_panel(Style::default().fg(palette.overlay0)),
    );
    Some(DrawerRender {
        area,
        agents,
        switch_button,
        close_button,
    })
}

impl ClientShellState {
    pub(super) fn open_machine_drawer(&mut self, endpoint_id: ClientEndpointId) {
        let anchor_y = self
            .hits
            .machines
            .iter()
            .find(|hit| hit.endpoint_id == endpoint_id)
            .map_or(2, |hit| hit.rect.y);
        self.overlay = Some(ClientShellOverlay::MachineDrawer(ClientMachineDrawer {
            endpoint_id,
            anchor_y,
            scroll: 0,
        }));
    }

    fn drawer_endpoint(&self) -> Option<ClientEndpointId> {
        match self.overlay.as_ref() {
            Some(ClientShellOverlay::MachineDrawer(drawer)) => Some(drawer.endpoint_id.clone()),
            _ => None,
        }
    }

    fn switch_to_drawer_machine(&mut self, outcome: &mut ClientShellInput) {
        let Some(endpoint_id) = self.drawer_endpoint() else {
            return;
        };
        self.overlay = None;
        outcome.repaint = true;
        self.activate_navigator_target(ClientNavigatorTarget::Machine { endpoint_id }, outcome);
    }

    fn scroll_drawer(&mut self, delta: isize) {
        if let Some(ClientShellOverlay::MachineDrawer(drawer)) = self.overlay.as_mut() {
            drawer.scroll = drawer.scroll.saturating_add_signed(delta);
        }
    }

    /// Keys while the drawer is open. Returns whether it is open.
    pub(super) fn route_drawer_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if !matches!(self.overlay, Some(ClientShellOverlay::MachineDrawer(_))) {
            return false;
        }
        let (code, _) = crate::config::normalize_key_combo((key.code, key.modifiers));
        outcome.repaint = true;
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.overlay = None,
            KeyCode::Enter => self.switch_to_drawer_machine(outcome),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_drawer(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_drawer(-1),
            KeyCode::PageDown => self.scroll_drawer(8),
            KeyCode::PageUp => self.scroll_drawer(-8),
            _ => {}
        }
        true
    }

    /// Mouse while the drawer is open. Returns whether the event was consumed.
    pub(super) fn route_drawer_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let Some(endpoint_id) = self.drawer_endpoint() else {
            return false;
        };
        let point = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if super::contains(self.hits.drawer_close, point) {
                    self.overlay = None;
                    outcome.repaint = true;
                } else if super::contains(self.hits.drawer_switch, point) {
                    self.switch_to_drawer_machine(outcome);
                } else if let Some(pane_id) = self
                    .hits
                    .drawer_agents
                    .iter()
                    .find(|(rect, _)| super::contains(*rect, point))
                    .map(|(_, pane_id)| pane_id.clone())
                {
                    self.overlay = None;
                    outcome.repaint = true;
                    self.activate_navigator_target(
                        ClientNavigatorTarget::Pane {
                            endpoint_id,
                            pane_id,
                        },
                        outcome,
                    );
                } else if !super::contains(self.hits.drawer_area, point) {
                    // A click elsewhere closes the drawer and is not passed on,
                    // so it cannot also act on whatever sits underneath.
                    self.overlay = None;
                    outcome.repaint = true;
                }
            }
            MouseEventKind::ScrollDown if super::contains(self.hits.drawer_area, point) => {
                self.scroll_drawer(3);
                outcome.repaint = true;
            }
            MouseEventKind::ScrollUp if super::contains(self.hits.drawer_area, point) => {
                self.scroll_drawer(-3);
                outcome.repaint = true;
            }
            _ => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparkline_scales_percent_to_100_and_others_to_their_peak() {
        assert_eq!(sparkline(&[0.0, 50.0, 100.0], 10, true), "▁▅█");
        // A high percent series still shows its movement.
        assert_eq!(sparkline(&[80.0, 85.0, 91.0], 10, true), "▃▅▆");
        assert_eq!(sparkline(&[0.0, 2.0, 4.0], 10, false), "▁▅█");
        // Values under 1 never fill the chart: a load of 0.5 is not "full".
        assert_eq!(sparkline(&[0.5], 10, false), "▅");
        // Only the newest `width` points fit.
        assert_eq!(sparkline(&[100.0, 0.0, 0.0], 2, true), "▁▁");
        assert_eq!(sparkline(&[], 10, true), "");
    }
}
