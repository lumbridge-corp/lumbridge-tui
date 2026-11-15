use super::render::{display_width, put_right_text, put_text, ShellRenderState};
use super::*;

fn collapsed_groups_for_endpoint<'a>(
    state: &'a ShellRenderState<'_>,
    endpoint_id: &ClientEndpointId,
) -> Option<&'a HashSet<String>> {
    if endpoint_id.is_local() {
        Some(state.collapsed_groups)
    } else {
        state.remote_collapsed_groups.get(endpoint_id)
    }
}

pub(super) fn render_collapsed(
    buffer: &mut Buffer,
    area: Rect,
    config: &ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    super::render::render_sidebar_background(buffer, area, palette);
    let (workspace_area, divider_y, detail_area) = super::sidebar::collapsed_sidebar_sections(area);
    let workspace_area = super::sidebar::render_collapsed_home(
        buffer,
        workspace_area,
        palette,
        state.sidebar_ui.hover,
        hits,
    );
    let mut total_rows = 0usize;
    let mut selected_row = None;
    let reveal = std::mem::take(state.reveal_navigation_workspace);
    for endpoint in state.endpoints {
        total_rows += 1;
        if state.collapsed_endpoints.contains(&endpoint.endpoint_id) {
            continue;
        }
        if let Some(snapshot) = endpoint.snapshot.as_deref() {
            if reveal {
                if let Some(target) = state
                    .selected_workspace_id
                    .filter(|target| target.endpoint_id == endpoint.endpoint_id)
                {
                    selected_row = snapshot
                        .workspaces
                        .iter()
                        .position(|workspace| workspace.workspace_id == target.workspace_id)
                        .map(|index| total_rows + index);
                }
            }
            total_rows += snapshot.workspaces.len();
        }
    }
    let height = usize::from(workspace_area.height);
    let max_scroll = total_rows.saturating_sub(height);
    *state.workspace_scroll = (*state.workspace_scroll).min(max_scroll);
    if let Some(row) = selected_row {
        if row < *state.workspace_scroll {
            *state.workspace_scroll = row;
        } else if row >= state.workspace_scroll.saturating_add(height) {
            *state.workspace_scroll = row.saturating_add(1).saturating_sub(height).min(max_scroll);
        }
    }
    hits.workspace_max_scroll = max_scroll;
    let mut skip = *state.workspace_scroll;
    let mut y = workspace_area.y;
    for (index, endpoint) in state.endpoints.iter().enumerate() {
        if y >= workspace_area.bottom() {
            break;
        }
        let rect = Rect::new(workspace_area.x, y, workspace_area.width, 1);
        let active = &endpoint.endpoint_id == state.active_endpoint_id;
        let collapsed = state.collapsed_endpoints.contains(&endpoint.endpoint_id);
        if skip > 0 {
            skip -= 1;
        } else {
            if active && collapsed {
                buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
            }
            let label = if endpoint.endpoint_id.is_local() {
                "L".to_owned()
            } else {
                (index + 1).to_string()
            };
            let marker = if collapsed { "▸" } else { "▾" };
            put_text(
                buffer,
                rect.x,
                rect.y,
                rect.width.saturating_sub(1),
                &format!("{marker}{label}"),
                Style::default().fg(if endpoint.status == ClientEndpointStatus::Online {
                    palette.text
                } else {
                    palette.overlay0
                }),
            );
            let mut status_badge = Rect::default();
            if !endpoint.endpoint_id.is_local() {
                let (glyph, _, color) = endpoint_status_presentation(endpoint.status, palette);
                let width = display_width(glyph).min(rect.width);
                status_badge = Rect::new(rect.right().saturating_sub(width), rect.y, width, 1);
                put_right_text(
                    buffer,
                    rect,
                    rect.y,
                    glyph,
                    state.machine_diagnostics.badge_style(
                        endpoint,
                        palette,
                        Style::default().fg(color),
                    ),
                );
            }
            hits.machines.push(MachineHit {
                rect,
                status_badge,
                collapse_toggle: Rect::new(rect.x, rect.y, u16::from(rect.width > 1), 1),
                endpoint_id: endpoint.endpoint_id.clone(),
            });
            y = y.saturating_add(1);
        }
        if collapsed {
            continue;
        }
        let Some(snapshot) = endpoint.snapshot.as_deref() else {
            continue;
        };
        for workspace in &snapshot.workspaces {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if y >= workspace_area.bottom() {
                break;
            }
            let rect = Rect::new(workspace_area.x, y, workspace_area.width, 1);
            let focused = active && workspace.focused;
            let selected = state.selected_workspace_id.is_some_and(|target| {
                target.matches(&endpoint.endpoint_id, &workspace.workspace_id)
            });
            let selection_background = if palette.selection_bg == ratatui::style::Color::Reset {
                palette.active_row_bg
            } else {
                palette.selection_bg
            };
            if selected {
                buffer.set_style(rect, Style::default().bg(selection_background));
            } else if focused {
                buffer.set_style(
                    rect,
                    Style::default().bg(super::sidebar::workspace_active_background(
                        palette,
                        state.selected_workspace_id.is_some(),
                    )),
                );
            }
            let stale = endpoint.status != ClientEndpointStatus::Online;
            let number = format!(" {}", workspace.number);
            let number_width = super::render::display_width(&number).min(rect.width);
            let dim = if stale {
                Modifier::DIM
            } else {
                Modifier::empty()
            };
            put_text(
                buffer,
                rect.x,
                rect.y,
                number_width,
                &number,
                Style::default()
                    .fg(if focused && !stale {
                        palette.text
                    } else {
                        palette.overlay0
                    })
                    .add_modifier(dim),
            );
            put_text(
                buffer,
                rect.x.saturating_add(number_width),
                rect.y,
                rect.width.saturating_sub(number_width),
                status_icon(workspace.agent_status, config.status_indicators),
                Style::default()
                    .fg(if stale {
                        palette.overlay0
                    } else {
                        status_color(workspace.agent_status, palette)
                    })
                    .add_modifier(dim),
            );
            hits.workspaces.push(WorkspaceHit {
                rect,
                endpoint_id: endpoint.endpoint_id.clone(),
                workspace_id: workspace.workspace_id.clone(),
                indented: false,
                group_toggle: None,
            });
            y = y.saturating_add(1);
        }
    }
    if let Some(divider_y) = divider_y {
        put_text(
            buffer,
            workspace_area.x,
            divider_y,
            workspace_area.width,
            &"─".repeat(workspace_area.width as usize),
            Style::default().fg(palette.surface_dim),
        );
    }
    super::endpoint_agents::render_collapsed(
        buffer,
        detail_area,
        state.endpoints,
        state.active_endpoint_id,
        config,
        state.sidebar_ui,
        hits,
    );
    hits.sidebar_toggle = if area.is_empty() || workspace_area.width == 0 {
        Rect::default()
    } else {
        Rect::new(
            workspace_area.x + workspace_area.width / 2,
            area.bottom().saturating_sub(1),
            1,
            1,
        )
    };
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "»",
        Style::default().fg(palette.overlay0),
    );
}

pub(super) fn render_expanded(
    buffer: &mut Buffer,
    area: Rect,
    active_snapshot: Option<&ClientShellSnapshot>,
    config: &ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    super::render::render_sidebar_background(buffer, area, palette);
    hits.sidebar_divider = if area.is_empty() {
        Rect::default()
    } else {
        Rect::new(area.right().saturating_sub(1), area.y, 1, area.height)
    };
    let empty_collapsed_groups = HashSet::new();

    enum Row {
        Endpoint(usize),
        Workspace {
            endpoint: usize,
            entry: WorkspaceEntry,
        },
    }
    let mut rows = Vec::new();
    for (endpoint_index, endpoint) in state.endpoints.iter().enumerate() {
        rows.push(Row::Endpoint(endpoint_index));
        if state.collapsed_endpoints.contains(&endpoint.endpoint_id) {
            continue;
        }
        if let Some(snapshot) = endpoint.snapshot.as_deref() {
            let collapsed_groups = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id)
                .unwrap_or(&empty_collapsed_groups);
            rows.extend(
                super::sidebar::workspace_entries(snapshot, collapsed_groups)
                    .into_iter()
                    .map(|entry| Row::Workspace {
                        endpoint: endpoint_index,
                        entry,
                    }),
            );
        }
    }
    let row_heights = rows
        .iter()
        .map(|row| match row {
            Row::Endpoint(index) => machine_header_rows(&state.endpoints[*index], config, state)
                .len()
                .clamp(1, u16::MAX as usize) as u16,
            Row::Workspace { endpoint, entry } => {
                let endpoint = &state.endpoints[*endpoint];
                let collapsed_groups = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id)
                    .unwrap_or(&empty_collapsed_groups);
                endpoint
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| {
                        let workspace = snapshot.workspaces.get(entry.index)?;
                        Some(
                            super::sidebar::workspace_rows(
                                workspace,
                                super::sidebar::displayed_workspace_status(
                                    snapshot,
                                    workspace,
                                    collapsed_groups,
                                ),
                                entry.indented,
                                &config.spaces,
                            )
                            .len()
                            .max(1)
                            .min(u16::MAX as usize) as u16,
                        )
                    })
                    .unwrap_or(1)
            }
        })
        .collect::<Vec<_>>();
    let gaps = rows
        .iter()
        .enumerate()
        .map(|(index, row)| match (row, rows.get(index + 1)) {
            (
                Row::Workspace { endpoint, .. },
                Some(Row::Workspace {
                    endpoint: next_endpoint,
                    entry,
                }),
            ) if endpoint == next_endpoint => u16::from(!entry.indented) * config.spaces.row_gap,
            (_, Some(Row::Endpoint(_))) => config.machines.row_gap,
            _ => 0,
        })
        .collect::<Vec<_>>();
    let split = if config.machines.auto_split && !state.sidebar_ui.split_manual {
        let machines_height = WORKSPACE_HEADER_ROWS
            .saturating_add(
                row_heights
                    .iter()
                    .fold(0u16, |sum, h| sum.saturating_add(*h)),
            )
            .saturating_add(gaps.iter().fold(0u16, |sum, g| sum.saturating_add(*g)))
            .saturating_add(1);
        let agents_height = super::endpoint_agents::content_height(
            state.endpoints,
            state.active_endpoint_id,
            config,
            state.sidebar_ui,
        );
        auto_section_split(area.height, machines_height, agents_height)
    } else {
        state.sidebar_section_split
    };
    let (workspace_area, detail_area) = crate::ui::expanded_sidebar_sections(area, split);
    hits.sidebar_section_divider = crate::ui::sidebar_section_divider_rect(area, split);
    super::sidebar::render_home_header(
        buffer,
        workspace_area,
        " machines",
        config,
        state.sidebar_ui.hover,
        hits,
    );

    let body = Rect::new(
        workspace_area.x,
        workspace_area.y.saturating_add(WORKSPACE_HEADER_ROWS),
        workspace_area.width,
        workspace_area
            .height
            .saturating_sub(WORKSPACE_HEADER_ROWS + 1),
    );
    hits.workspace_body = body;
    let reveal_navigation = !body.is_empty() && std::mem::take(state.reveal_navigation_workspace);
    let reveal_focus = !body.is_empty() && std::mem::take(state.reveal_focused_workspace);
    if reveal_navigation || reveal_focus {
        let selected_row = rows.iter().position(|row| match row {
            Row::Workspace { endpoint, entry } => {
                let endpoint = &state.endpoints[*endpoint];
                endpoint
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| snapshot.workspaces.get(entry.index))
                    .is_some_and(|workspace| {
                        if reveal_navigation {
                            state.selected_workspace_id.is_some_and(|target| {
                                target.matches(&endpoint.endpoint_id, &workspace.workspace_id)
                            })
                        } else {
                            &endpoint.endpoint_id == state.active_endpoint_id
                                && active_snapshot.is_some_and(|snapshot| {
                                    snapshot.focused_workspace_id.as_deref()
                                        == Some(workspace.workspace_id.as_str())
                                })
                        }
                    })
            }
            Row::Endpoint(_) => false,
        });
        if let Some(selected_row) = selected_row {
            *state.workspace_scroll = super::scroll::list_scroll_start_to_reveal(
                &row_heights,
                &gaps,
                body.height,
                *state.workspace_scroll,
                selected_row,
            );
        }
    }
    let metrics = super::scroll::list_scroll_metrics(
        &row_heights,
        &gaps,
        body.height,
        *state.workspace_scroll,
    );
    hits.workspace_max_scroll = metrics.max_offset_from_bottom;
    hits.workspace_scroll_metrics = Some(metrics);
    *state.workspace_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (row_index, row) in rows.iter().enumerate().skip(*state.workspace_scroll) {
        match row {
            Row::Endpoint(index) => {
                if y >= body.bottom() {
                    break;
                }
                let endpoint = &state.endpoints[*index];
                let header_rows = machine_header_rows(endpoint, config, state);
                let row_height = (header_rows.len().clamp(1, u16::MAX as usize) as u16)
                    .min(body.bottom().saturating_sub(y));
                let rect = Rect::new(body.x, y, content_width, row_height);
                let collapsed = state.collapsed_endpoints.contains(&endpoint.endpoint_id);
                let marker = if collapsed { "▸" } else { "▾" };
                let status_badge = render_endpoint_row(
                    buffer,
                    rect,
                    marker,
                    endpoint,
                    &header_rows,
                    collapsed && &endpoint.endpoint_id == state.active_endpoint_id,
                    state.machine_diagnostics,
                    palette,
                );
                hits.machines.push(MachineHit {
                    rect,
                    status_badge,
                    collapse_toggle: Rect::new(
                        rect.x.saturating_add(1),
                        rect.y,
                        u16::from(rect.width > 1),
                        1,
                    ),
                    endpoint_id: endpoint.endpoint_id.clone(),
                });
                y = y
                    .saturating_add(row_height)
                    .saturating_add(gaps.get(row_index).copied().unwrap_or(0));
            }
            Row::Workspace { endpoint, entry } => {
                let endpoint = &state.endpoints[*endpoint];
                let Some(snapshot) = endpoint.snapshot.as_deref() else {
                    continue;
                };
                let Some(workspace) = snapshot.workspaces.get(entry.index) else {
                    continue;
                };
                let collapsed_groups = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id)
                    .unwrap_or(&empty_collapsed_groups);
                let status = super::sidebar::displayed_workspace_status(
                    snapshot,
                    workspace,
                    collapsed_groups,
                );
                let tokens = super::sidebar::workspace_rows(
                    workspace,
                    status,
                    entry.indented,
                    &config.spaces,
                );
                let height = (tokens.len().max(1).min(u16::MAX as usize) as u16).min(body.height);
                if y.saturating_add(height) > body.bottom() {
                    break;
                }
                let rect = Rect::new(body.x, y, content_width, height);
                let nested = Rect::new(
                    rect.x.saturating_add(2),
                    rect.y,
                    rect.width.saturating_sub(2),
                    rect.height,
                );
                let endpoint_active = &endpoint.endpoint_id == state.active_endpoint_id;
                let selected = state.selected_workspace_id.is_some_and(|target| {
                    target.matches(&endpoint.endpoint_id, &workspace.workspace_id)
                });
                super::sidebar::render_workspace_rows(
                    buffer,
                    nested,
                    status,
                    config.status_indicators,
                    entry,
                    tokens,
                    endpoint_active && workspace.focused,
                    selected,
                    state.selected_workspace_id.is_some(),
                    false,
                    palette,
                );
                if endpoint.status != ClientEndpointStatus::Online {
                    buffer.set_style(
                        rect,
                        Style::default()
                            .fg(palette.overlay0)
                            .add_modifier(Modifier::DIM),
                    );
                }
                let group_toggle = super::sidebar::render_parent_group_toggle(
                    buffer,
                    rect,
                    snapshot,
                    entry.index,
                    collapsed_groups,
                    palette,
                );
                hits.workspaces.push(WorkspaceHit {
                    rect,
                    endpoint_id: endpoint.endpoint_id.clone(),
                    workspace_id: workspace.workspace_id.clone(),
                    indented: entry.indented,
                    group_toggle,
                });
                y = y
                    .saturating_add(height)
                    .saturating_add(gaps.get(row_index).copied().unwrap_or(0));
            }
        }
    }
    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.workspace_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }

    let footer_y = workspace_area.bottom().saturating_sub(1);
    if config.mouse_capture && config.sidebar_buttons == crate::config::SidebarButtonStyle::Pill {
        render_pill_footer(
            buffer,
            workspace_area,
            footer_y,
            &format!("+ new · {}", active_endpoint_label(state)),
            active_snapshot.is_some_and(super::global_menu::global_menu_attention),
            config,
            state.sidebar_ui.hover,
            hits,
        );
    } else if config.mouse_capture {
        let label = format!(" new · {}", active_endpoint_label(state));
        hits.new_workspace = Rect::new(
            workspace_area.x,
            footer_y,
            display_width(&label).min(workspace_area.width),
            u16::from(workspace_area.height > 0),
        );
        put_text(
            buffer,
            workspace_area.x,
            footer_y,
            workspace_area.width,
            &label,
            Style::default().fg(palette.overlay0),
        );
        let attention = active_snapshot.is_some_and(super::global_menu::global_menu_attention);
        let width = if attention { 8 } else { 6 }.min(workspace_area.width);
        hits.global_launcher = Rect::new(
            workspace_area.right().saturating_sub(width),
            footer_y,
            width,
            1,
        );
        put_right_text(
            buffer,
            workspace_area,
            footer_y,
            if attention { "● menu" } else { "menu" },
            Style::default().fg(if attention {
                palette.accent
            } else {
                palette.overlay0
            }),
        );
    }
    super::endpoint_agents::render_expanded(
        buffer,
        detail_area,
        active_snapshot.and_then(|snapshot| snapshot.agent_view_label.as_deref()),
        state.endpoints,
        state.active_endpoint_id,
        config,
        state.agent_scroll,
        state.sidebar_ui,
        hits,
    );
    hits.sidebar_toggle = Rect::new(
        area.right().saturating_sub(2),
        area.bottom().saturating_sub(1),
        u16::from(area.width > 1),
        u16::from(area.height > 0),
    );
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "«",
        Style::default().fg(palette.overlay0),
    );
}

/// Footer with filled `new` and `menu` buttons that light up under the mouse.
#[allow(clippy::too_many_arguments)] // mirrors the other sidebar renderers
pub(super) fn render_pill_footer(
    buffer: &mut Buffer,
    area: Rect,
    y: u16,
    new_label: &str,
    attention: bool,
    config: &ClientShellConfig,
    hover: Option<ChromeButton>,
    hits: &mut ShellHitMap,
) {
    if area.height == 0 || area.width < 4 {
        return;
    }
    let kind = config.sidebar_buttons;
    let palette = &config.palette;
    let menu_label = if attention { "● menu" } else { "≡ menu" };
    let menu_width = super::render::button_width(menu_label, kind).min(area.width);
    let menu_x = area.right().saturating_sub(menu_width);
    hits.global_launcher = super::render::put_button(
        buffer,
        menu_x,
        y,
        menu_label,
        if attention {
            palette.accent
        } else {
            palette.subtext0
        },
        kind,
        hover == Some(ChromeButton::GlobalMenu),
        false,
        palette,
    );
    let find_width = super::render::button_width("⌕", kind);
    let find_x = menu_x.saturating_sub(find_width.saturating_add(1));
    let new_x = area.x.saturating_add(1);
    let menu_x = if find_x > new_x.saturating_add(6) {
        hits.palette_button = super::render::put_button(
            buffer,
            find_x,
            y,
            "⌕",
            palette.subtext0,
            kind,
            hover == Some(ChromeButton::Palette),
            false,
            palette,
        );
        // The notebook button sits left of the palette button when it fits.
        let notes_width = super::render::button_width("✎", kind);
        let notes_x = find_x.saturating_sub(notes_width.saturating_add(1));
        if notes_x > new_x.saturating_add(6) {
            hits.notebook_button = super::render::put_button(
                buffer,
                notes_x,
                y,
                "✎",
                palette.subtext0,
                kind,
                hover == Some(ChromeButton::Notebook),
                false,
                palette,
            );
            notes_x
        } else {
            find_x
        }
    } else {
        menu_x
    };
    let room = menu_x.saturating_sub(new_x.saturating_add(1));
    let new_label = if super::render::button_width(new_label, kind) <= room {
        new_label
    } else {
        "+ new"
    };
    if super::render::button_width(new_label, kind) <= room {
        hits.new_workspace = super::render::put_button(
            buffer,
            new_x,
            y,
            new_label,
            palette.subtext0,
            kind,
            hover == Some(ChromeButton::NewWorkspace),
            false,
            palette,
        );
    }
}

/// Split ratio that gives each sidebar section the rows its content needs.
///
/// When both fit, machines get exactly their height and agents the rest. When
/// they do not, neither section is squeezed below two fifths of the sidebar
/// unless its content is shorter than that.
pub(super) fn auto_section_split(total: u16, machines: u16, agents: u16) -> f32 {
    if total == 0 {
        return 0.5;
    }
    let machines_rows = if machines.saturating_add(agents) <= total {
        machines
    } else {
        let floor = total.saturating_mul(2) / 5;
        let agents_min = agents.min(floor);
        let machines_min = machines.min(floor);
        total
            .saturating_sub(agents_min)
            .min(machines)
            .max(machines_min)
    };
    f32::from(machines_rows) / f32::from(total)
}

fn active_endpoint_label<'a>(state: &'a ShellRenderState<'_>) -> &'a str {
    state
        .endpoints
        .iter()
        .find(|endpoint| &endpoint.endpoint_id == state.active_endpoint_id)
        .map_or("Local", |endpoint| endpoint.label.as_str())
}

/// Resolve the configured machine header rows for one endpoint.
///
/// Agent counts come from the endpoint's own snapshot; `$name` tokens and
/// service dots come from the machine feeder, keyed by the machine label.
fn machine_header_rows(
    endpoint: &ClientShellEndpoint,
    config: &ClientShellConfig,
    state: &ShellRenderState<'_>,
) -> Vec<Vec<crate::ui::ResolvedToken>> {
    use crate::api::schema::AgentStatus;
    use crate::client::machine_feed::ServiceState;

    let feed = state.machine_feed.entry(&endpoint.label);
    let services = feed
        .map(|entry| {
            entry
                .services
                .iter()
                .map(|service| match service.state {
                    ServiceState::Up => Some(true),
                    ServiceState::Down => Some(false),
                    ServiceState::Unknown => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let agents = endpoint.snapshot.as_deref().map(|snapshot| {
        let count = |status| {
            snapshot
                .agents
                .iter()
                .filter(|agent| agent.agent_status == status)
                .count()
        };
        crate::ui::MachineAgentCounts {
            total: snapshot.agents.len(),
            blocked: count(AgentStatus::Blocked),
            working: count(AgentStatus::Working),
        }
    });
    let mut rows = crate::ui::sidebar_machine_rows(
        &config.machines,
        crate::ui::MachineTokenContext {
            label: &endpoint.label,
            agents,
            tokens: feed.map(|entry| &entry.tokens),
            services: &services,
        },
    );
    if config.machines.fold_healthy {
        let offline =
            !endpoint.endpoint_id.is_local() && endpoint.status != ClientEndpointStatus::Online;
        let flagged = feed.is_some_and(|entry| {
            entry.tokens.values().any(|value| value.contains('!'))
                || entry
                    .services
                    .iter()
                    .any(|service| service.state == ServiceState::Down)
        });
        let blocked = agents.is_some_and(|counts| counts.blocked > 0);
        if !(offline || flagged || blocked) {
            rows.truncate(1);
        }
    }
    // A disconnected machine keeps its last snapshot; dim its agent counts so
    // they do not read as live.
    if endpoint.status != ClientEndpointStatus::Online && !endpoint.endpoint_id.is_local() {
        for token in rows.iter_mut().flatten() {
            if matches!(
                token.kind,
                crate::ui::ResolvedTokenKind::AgentCount(_)
                    | crate::ui::ResolvedTokenKind::BlockedCount(_)
                    | crate::ui::ResolvedTokenKind::WorkingCount(_)
            ) && token.style.dim.is_none()
            {
                token.style.dim = Some(true);
            }
        }
    }
    rows
}

#[allow(clippy::too_many_arguments)] // mirrors the other sidebar row renderers
fn render_endpoint_row(
    buffer: &mut Buffer,
    rect: Rect,
    marker: &str,
    endpoint: &ClientShellEndpoint,
    rows: &[Vec<crate::ui::ResolvedToken>],
    highlighted: bool,
    auth: &super::machine_diagnostics::MachineDiagnostics,
    palette: &Palette,
) -> Rect {
    use ratatui::text::Line;
    use ratatui::widgets::{Paragraph, Widget};

    if highlighted {
        buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
    }
    let (glyph, state, color) = endpoint_status_presentation(endpoint.status, palette);
    let state = if endpoint.status == ClientEndpointStatus::Online {
        ""
    } else {
        state
    };
    let signal = if auth.required_for(endpoint) {
        "! auth".to_owned()
    } else if endpoint.status == ClientEndpointStatus::Attention {
        "! error".to_owned()
    } else if endpoint.endpoint_id.is_local() {
        String::new()
    } else if state.is_empty() {
        glyph.to_owned()
    } else {
        format!("{glyph} {state}")
    };
    let signal_width = display_width(&signal).min(rect.width);
    let label_style = Style::default()
        .fg(
            if matches!(endpoint.status, ClientEndpointStatus::Disabled) {
                palette.overlay0
            } else {
                palette.text
            },
        )
        .add_modifier(Modifier::BOLD);
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width.saturating_sub(signal_width.saturating_add(1)),
        &format!(" {marker}"),
        label_style,
    );
    let content_x = rect.x.saturating_add(3);
    for (row_index, row) in rows.iter().enumerate() {
        let y = rect.y.saturating_add(row_index as u16);
        if y >= rect.bottom() {
            break;
        }
        let right = if row_index == 0 {
            rect.right().saturating_sub(signal_width.saturating_add(1))
        } else {
            rect.right().saturating_sub(1)
        };
        let width = right.saturating_sub(content_x);
        if width == 0 {
            continue;
        }
        let spans = crate::ui::resolved_token_spans(
            row,
            // Machine rows have no state_icon token.
            ("", Style::default()),
            Style::default().fg(palette.overlay1),
            label_style,
            Style::default().fg(palette.overlay1),
            Style::default().fg(palette.subtext0),
            palette,
            width as usize,
        );
        Paragraph::new(Line::from(spans)).render(Rect::new(content_x, y, width, 1), buffer);
    }
    put_right_text(
        buffer,
        Rect::new(rect.x, rect.y, rect.width, 1),
        rect.y,
        &signal,
        auth.badge_style(endpoint, palette, Style::default().fg(color)),
    );
    Rect::new(
        rect.right().saturating_sub(signal_width),
        rect.y,
        signal_width,
        1,
    )
}
