//! Command palette: one fuzzy-searchable list of agents, workspaces, machines,
//! lumbridge-tui actions and endpoint commands (custom commands and plugin actions).
//!
//! Choosing an entry does exactly what its keybinding or navigator row would:
//! jumps reuse the navigator's activation, actions go through
//! `record_binding`, and commands through `invoke_command`.

use super::render::{display_width, put_right_text, put_text};
use super::*;
use crate::input::KeybindAction;
use crossterm::event::{MouseButton, MouseEventKind};

const POPUP_WIDTH: u16 = 84;
const POPUP_HEIGHT: u16 = 22;
/// Rows above the list: query line and separator.
const HEADER_ROWS: u16 = 2;
/// Rows below the list: separator and key hints.
const FOOTER_ROWS: u16 = 2;

#[derive(Debug, Default)]
pub(super) struct ClientPaletteOverlay {
    pub(super) query: TextEditor,
    /// Index into the ranked results.
    pub(super) selected: usize,
    pub(super) scroll: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PaletteTarget {
    Navigate(ClientNavigatorTarget),
    MachineDetails(ClientEndpointId),
    Action(KeybindAction),
    Command {
        command_id: String,
        action: crate::protocol::ClientShellCommandAction,
    },
    /// Index into `[[ui.launchers]]`; runs beside the focused pane.
    Launcher(usize),
    Notebook,
    Home,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaletteKind {
    Agent,
    Workspace,
    Machine,
    Action,
    Command,
}

impl PaletteKind {
    fn tag(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Workspace => "space",
            Self::Machine => "machine",
            Self::Action => "action",
            Self::Command => "command",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct PaletteItem {
    pub(super) kind: PaletteKind,
    pub(super) label: String,
    /// Context shown dimmed after the label, e.g. `build · ~/src/app`.
    pub(super) detail: String,
    /// Right-aligned hint, e.g. the action's keybinding.
    pub(super) hint: String,
    pub(super) status: Option<crate::api::schema::AgentStatus>,
    pub(super) stale: bool,
    pub(super) target: PaletteTarget,
}

/// Actions offered in the palette, in the order shown for an empty query.
const ACTIONS: &[(KeybindAction, &str)] = &[
    (KeybindAction::NewWorkspace, "new workspace"),
    (KeybindAction::NewTab, "new tab"),
    (KeybindAction::SplitVertical, "split pane side by side"),
    (KeybindAction::SplitHorizontal, "split pane stacked"),
    (KeybindAction::Zoom, "zoom pane"),
    (KeybindAction::ClosePane, "close pane"),
    (KeybindAction::RenamePane, "rename pane"),
    (KeybindAction::RenameTab, "rename tab"),
    (KeybindAction::CloseTab, "close tab"),
    (KeybindAction::NextTab, "next tab"),
    (KeybindAction::PreviousTab, "previous tab"),
    (KeybindAction::RenameWorkspace, "rename workspace"),
    (KeybindAction::CloseWorkspace, "close workspace"),
    (KeybindAction::NewWorktree, "new git worktree"),
    (KeybindAction::OpenWorktree, "open git worktree"),
    (KeybindAction::RemoveWorktree, "remove git worktree"),
    (KeybindAction::LastPane, "last pane"),
    (KeybindAction::EnterResizeMode, "resize panes"),
    (KeybindAction::CopyMode, "copy mode"),
    (KeybindAction::EditScrollback, "edit scrollback in $EDITOR"),
    (KeybindAction::ClearPane, "clear pane"),
    (KeybindAction::ToggleSidebar, "toggle sidebar"),
    (KeybindAction::OpenNavigator, "session navigator"),
    (
        KeybindAction::OpenNotificationTarget,
        "open notification target",
    ),
    (KeybindAction::Settings, "settings"),
    (KeybindAction::Help, "keybinds"),
    (KeybindAction::ReloadConfig, "reload config"),
    (KeybindAction::Detach, "detach"),
];

fn action_binding(keybinds: &crate::config::Keybinds, action: KeybindAction) -> Option<String> {
    let bindings = match action {
        KeybindAction::NewWorkspace => &keybinds.new_workspace,
        KeybindAction::NewTab => &keybinds.new_tab,
        KeybindAction::SplitVertical => &keybinds.split_vertical,
        KeybindAction::SplitHorizontal => &keybinds.split_horizontal,
        KeybindAction::Zoom => &keybinds.zoom,
        KeybindAction::ClosePane => &keybinds.close_pane,
        KeybindAction::RenamePane => &keybinds.rename_pane,
        KeybindAction::RenameTab => &keybinds.rename_tab,
        KeybindAction::CloseTab => &keybinds.close_tab,
        KeybindAction::NextTab => &keybinds.next_tab,
        KeybindAction::PreviousTab => &keybinds.previous_tab,
        KeybindAction::RenameWorkspace => &keybinds.rename_workspace,
        KeybindAction::CloseWorkspace => &keybinds.close_workspace,
        KeybindAction::NewWorktree => &keybinds.new_worktree,
        KeybindAction::OpenWorktree => &keybinds.open_worktree,
        KeybindAction::RemoveWorktree => &keybinds.remove_worktree,
        KeybindAction::LastPane => &keybinds.last_pane,
        KeybindAction::EnterResizeMode => &keybinds.resize_mode,
        KeybindAction::CopyMode => &keybinds.copy_mode,
        KeybindAction::EditScrollback => &keybinds.edit_scrollback,
        KeybindAction::ClearPane => &keybinds.clear_pane,
        KeybindAction::ToggleSidebar => &keybinds.toggle_sidebar,
        KeybindAction::OpenNavigator => &keybinds.goto,
        KeybindAction::OpenNotificationTarget => &keybinds.open_notification_target,
        KeybindAction::Settings => &keybinds.settings,
        KeybindAction::Help => &keybinds.help,
        KeybindAction::ReloadConfig => &keybinds.reload_config,
        KeybindAction::Detach => &keybinds.detach,
        _ => return None,
    };
    bindings.label()
}

/// Every palette entry, before filtering: agents needing attention first,
/// then workspaces, machines, actions and commands.
pub(super) fn palette_items(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    snapshot: Option<&ClientShellSnapshot>,
    keybinds: &crate::config::Keybinds,
) -> Vec<PaletteItem> {
    let mut items = Vec::new();
    let multi_machine = endpoints.len() > 1;
    for row in super::aggregate_navigation::aggregate_agent_rows(
        endpoints,
        active_endpoint_id,
        crate::config::AgentPanelSortConfig::Priority,
    ) {
        let snapshot = row.endpoint.snapshot;
        let agent = row.agent;
        let agent_label = agent
            .display_agent
            .as_deref()
            .or(agent.name.as_deref())
            .or(agent.agent.as_deref());
        let label =
            crate::ui::meaningful_task_title(agent.terminal_title_stripped.as_deref(), agent_label)
                .or(agent_label)
                .unwrap_or("agent")
                .to_owned();
        let workspace = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == agent.workspace_id)
            .map(|workspace| workspace.label.as_str())
            .unwrap_or_default();
        let detail = if multi_machine {
            format!("{} · {workspace}", row.endpoint.label)
        } else {
            workspace.to_owned()
        };
        items.push(PaletteItem {
            kind: PaletteKind::Agent,
            label,
            detail,
            hint: agent_label.unwrap_or_default().to_owned(),
            status: Some(agent.agent_status),
            stale: row.endpoint.stale(),
            target: PaletteTarget::Navigate(ClientNavigatorTarget::Pane {
                endpoint_id: row.endpoint.endpoint_id.clone(),
                pane_id: agent.pane_id.clone(),
            }),
        });
    }
    for endpoint in endpoints {
        let Some(snapshot) = endpoint.snapshot.as_deref() else {
            continue;
        };
        for workspace in &snapshot.workspaces {
            items.push(PaletteItem {
                kind: PaletteKind::Workspace,
                label: workspace.label.clone(),
                detail: if multi_machine {
                    endpoint.label.clone()
                } else {
                    String::new()
                },
                hint: String::new(),
                status: Some(workspace.agent_status),
                stale: !endpoint.endpoint_id.is_local()
                    && endpoint.status != ClientEndpointStatus::Online,
                target: PaletteTarget::Navigate(ClientNavigatorTarget::Workspace {
                    endpoint_id: endpoint.endpoint_id.clone(),
                    workspace_id: workspace.workspace_id.clone(),
                }),
            });
        }
    }
    if multi_machine {
        for endpoint in endpoints {
            let online = endpoint.status == ClientEndpointStatus::Online;
            items.push(PaletteItem {
                kind: PaletteKind::Machine,
                label: endpoint.label.clone(),
                detail: if online { "online" } else { "offline" }.to_owned(),
                hint: String::new(),
                status: None,
                stale: !online && !endpoint.endpoint_id.is_local(),
                target: PaletteTarget::Navigate(ClientNavigatorTarget::Machine {
                    endpoint_id: endpoint.endpoint_id.clone(),
                }),
            });
        }
        for endpoint in endpoints {
            items.push(PaletteItem {
                kind: PaletteKind::Machine,
                label: format!("{} details", endpoint.label),
                detail: "health, services, alerts, agents".to_owned(),
                hint: String::new(),
                status: None,
                stale: false,
                target: PaletteTarget::MachineDetails(endpoint.endpoint_id.clone()),
            });
        }
    }
    for &(action, label) in ACTIONS {
        items.push(PaletteItem {
            kind: PaletteKind::Action,
            label: label.to_owned(),
            detail: String::new(),
            hint: action_binding(keybinds, action).unwrap_or_default(),
            status: None,
            stale: false,
            target: PaletteTarget::Action(action),
        });
    }
    for command in snapshot.map_or(&[][..], |snapshot| snapshot.commands.as_slice()) {
        if command.action == crate::protocol::ClientShellCommandAction::Unknown {
            continue;
        }
        items.push(PaletteItem {
            kind: PaletteKind::Command,
            label: command
                .description
                .clone()
                .unwrap_or_else(|| command.command_id.clone()),
            detail: String::new(),
            hint: command.binding_label.clone(),
            status: None,
            stale: false,
            target: PaletteTarget::Command {
                command_id: command.command_id.clone(),
                action: command.action,
            },
        });
    }
    items
}

/// Scores `text` against a space-separated query; every term must match as a
/// subsequence. Word starts and runs of consecutive characters score higher.
pub(super) fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let haystack: String = text.iter().collect();
    let word_start = |index: usize| index == 0 || !text[index - 1].is_alphanumeric();
    let mut total = 0;
    for term in query.to_lowercase().split_whitespace() {
        let term: Vec<char> = term.chars().collect();
        let needle: String = term.iter().collect();
        let contiguous = haystack.contains(&needle);
        // A term starts at a word boundary ("nt" for "new tab") unless it
        // appears verbatim; letters scattered through a long title are noise.
        let first = term.first()?;
        let start = (0..text.len())
            .find(|&index| text[index] == *first && word_start(index))
            .or_else(|| {
                contiguous
                    .then(|| haystack.find(&needle))
                    .flatten()
                    .map(|byte| haystack[..byte].chars().count())
            })?;
        let mut score = 10 + if word_start(start) { 15 } else { 0 } - start.min(10) as i32;
        let mut last = start;
        for ch in &term[1..] {
            let found = text[last + 1..]
                .iter()
                .position(|candidate| candidate == ch)?;
            let index = last + 1 + found;
            score += 10;
            if word_start(index) {
                score += 15;
            }
            if index == last + 1 {
                score += 8;
            } else {
                score -= (index - last - 1).min(10) as i32;
            }
            last = index;
        }
        if contiguous {
            score += 20;
        }
        total += score;
    }
    Some(total)
}

/// Indices into `items`, best match first. An empty query keeps every item
/// in its natural order.
pub(super) fn rank(items: &[PaletteItem], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..items.len()).collect();
    }
    let mut scored = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            // The label carries the most weight; context and kind still match.
            let label = fuzzy_score(query, &item.label).map(|score| score + 30);
            let full = fuzzy_score(
                query,
                &format!("{} {} {}", item.label, item.detail, item.kind.tag()),
            );
            label.max(full).map(|score| (index, score))
        })
        .collect::<Vec<_>>();
    scored.sort_by_key(|(index, score)| (std::cmp::Reverse(*score), *index));
    scored.into_iter().map(|(index, _)| index).collect()
}

pub(super) struct PaletteRender {
    pub(super) area: Rect,
    pub(super) rows: Vec<(Rect, usize)>,
    pub(super) cursor: Option<crate::protocol::CursorState>,
}

pub(super) fn render_palette(
    buffer: &mut Buffer,
    overlay: &mut ClientPaletteOverlay,
    items: &[PaletteItem],
    config: &ClientShellConfig,
) -> Option<PaletteRender> {
    let palette = &config.palette;
    let screen = buffer.area;
    let width = POPUP_WIDTH.min(screen.width.saturating_sub(4));
    let height = POPUP_HEIGHT.min(screen.height.saturating_sub(2));
    if width < 24 || height < HEADER_ROWS + FOOTER_ROWS + 3 {
        return None;
    }
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 3,
        width,
        height,
    );
    let background = Style::default().bg(palette.panel_bg).fg(palette.text);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            buffer[(x, y)].set_symbol(" ").set_style(background);
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

    let ranked = rank(items, overlay.query.as_str());
    overlay.selected = overlay.selected.min(ranked.len().saturating_sub(1));
    let count = format!("{} of {}", ranked.len(), items.len());
    put_text(
        buffer,
        inner.x,
        inner.y,
        2,
        "› ",
        Style::default()
            .fg(palette.accent)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    let query_area = Rect::new(
        inner.x + 2,
        inner.y,
        inner.width.saturating_sub(3 + display_width(&count)),
        1,
    );
    let cursor = super::text_editor::render(
        buffer,
        query_area,
        &overlay.query,
        Style::default().fg(palette.text).bg(palette.panel_bg),
    );
    if overlay.query.as_str().is_empty() {
        put_text(
            buffer,
            query_area.x,
            query_area.y,
            query_area.width,
            "jump to an agent, space or machine, or run an action",
            Style::default().fg(palette.overlay0).bg(palette.panel_bg),
        );
    }
    put_right_text(
        buffer,
        inner,
        inner.y,
        &count,
        Style::default().fg(palette.overlay0).bg(palette.panel_bg),
    );
    let rule = "─".repeat(inner.width as usize);
    let rule_style = Style::default().fg(palette.surface1).bg(palette.panel_bg);
    put_text(buffer, inner.x, inner.y + 1, inner.width, &rule, rule_style);
    let footer_y = inner.bottom().saturating_sub(1);
    put_text(
        buffer,
        inner.x,
        footer_y - 1,
        inner.width,
        &rule,
        rule_style,
    );
    put_text(
        buffer,
        inner.x,
        footer_y,
        inner.width,
        "↑↓ move   enter run   esc close",
        Style::default().fg(palette.overlay0).bg(palette.panel_bg),
    );

    let list = Rect::new(
        inner.x,
        inner.y + HEADER_ROWS,
        inner.width,
        inner.height.saturating_sub(HEADER_ROWS + FOOTER_ROWS),
    );
    let visible = usize::from(list.height);
    if ranked.is_empty() {
        put_text(
            buffer,
            list.x,
            list.y,
            list.width,
            "no matches",
            Style::default().fg(palette.overlay0).bg(palette.panel_bg),
        );
    }
    if overlay.selected < overlay.scroll {
        overlay.scroll = overlay.selected;
    } else if overlay.selected >= overlay.scroll + visible {
        overlay.scroll = overlay.selected + 1 - visible;
    }
    overlay.scroll = overlay.scroll.min(ranked.len().saturating_sub(visible));

    let mut rows = Vec::new();
    for (offset, &item_index) in ranked.iter().skip(overlay.scroll).take(visible).enumerate() {
        let item = &items[item_index];
        let position = overlay.scroll + offset;
        let y = list.y + offset as u16;
        let rect = Rect::new(area.x + 1, y, area.width - 2, 1);
        let selected = position == overlay.selected;
        let row_bg = if selected {
            palette.selection_bg
        } else {
            palette.panel_bg
        };
        buffer.set_style(rect, Style::default().bg(row_bg));
        let base = Style::default().bg(row_bg);
        let mut x = list.x;
        put_text(
            buffer,
            x,
            y,
            8,
            item.kind.tag(),
            base.fg(if selected {
                palette.accent
            } else {
                palette.overlay0
            }),
        );
        x += 8;
        if let Some(status) = item.status.filter(|_| item.kind == PaletteKind::Agent) {
            put_text(
                buffer,
                x,
                y,
                2,
                status_icon(status, config.status_indicators),
                base.fg(if item.stale {
                    palette.overlay0
                } else {
                    status_color(status, palette)
                }),
            );
            x += 2;
        }
        let hint_width = display_width(&item.hint);
        let right = list.right().saturating_sub(hint_width.saturating_add(1));
        let label_style = base
            .fg(if item.stale {
                palette.overlay0
            } else {
                palette.text
            })
            .add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
        let label_width = display_width(&item.label).min(right.saturating_sub(x));
        put_text(buffer, x, y, label_width, &item.label, label_style);
        x += label_width;
        if !item.detail.is_empty() && x + 3 < right {
            put_text(
                buffer,
                x,
                y,
                right - x,
                &format!("  {}", item.detail),
                base.fg(palette.overlay0),
            );
        }
        if hint_width > 0 {
            put_right_text(
                buffer,
                list,
                y,
                &item.hint,
                base.fg(if item.kind == PaletteKind::Agent {
                    palette.overlay0
                } else {
                    palette.mauve
                }),
            );
        }
        rows.push((rect, position));
    }
    Some(PaletteRender { area, rows, cursor })
}

impl ClientShellState {
    pub(super) fn open_palette(&mut self) {
        self.overlay = Some(ClientShellOverlay::Palette(ClientPaletteOverlay::default()));
    }

    pub(super) fn palette_entries(&self) -> Vec<PaletteItem> {
        let mut items = palette_items(
            &self.endpoints,
            &self.active_endpoint_id,
            self.snapshot.as_deref(),
            &self.config.keybinds.keybinds,
        );
        items.push(PaletteItem {
            kind: PaletteKind::Action,
            label: "notebook".to_owned(),
            detail: format!(
                "search, open or send notes in {}",
                self.notebook_dir_label()
            ),
            hint: String::new(),
            status: None,
            stale: false,
            target: PaletteTarget::Notebook,
        });
        items.push(PaletteItem {
            kind: PaletteKind::Action,
            label: "home".to_owned(),
            detail: "Lumbridge: notes, fleet, palette".to_owned(),
            hint: String::new(),
            status: None,
            stale: false,
            target: PaletteTarget::Home,
        });
        if self.focused_pane_id().is_some() {
            items.extend(
                self.config
                    .launchers
                    .iter()
                    .enumerate()
                    .map(|(index, launcher)| PaletteItem {
                        kind: PaletteKind::Command,
                        label: format!("new {} here", launcher.label),
                        detail: launcher.command.clone(),
                        hint: String::new(),
                        status: None,
                        stale: false,
                        target: PaletteTarget::Launcher(index),
                    }),
            );
        }
        items
    }

    fn palette_result_count(&self) -> usize {
        let Some(ClientShellOverlay::Palette(overlay)) = self.overlay.as_ref() else {
            return 0;
        };
        rank(&self.palette_entries(), overlay.query.as_str()).len()
    }

    pub(super) fn move_palette_selection(&mut self, delta: isize) {
        let count = self.palette_result_count();
        if let Some(ClientShellOverlay::Palette(overlay)) = self.overlay.as_mut() {
            let last = count.saturating_sub(1) as isize;
            overlay.selected = (overlay.selected as isize + delta).clamp(0, last) as usize;
        }
    }

    /// Runs the selected entry, or the entry at ranked `position`.
    pub(super) fn accept_palette(
        &mut self,
        position: Option<usize>,
        outcome: &mut ClientShellInput,
    ) {
        let Some(ClientShellOverlay::Palette(overlay)) = self.overlay.as_ref() else {
            return;
        };
        let items = self.palette_entries();
        let ranked = rank(&items, overlay.query.as_str());
        let Some(target) = ranked
            .get(position.unwrap_or(overlay.selected))
            .map(|index| items[*index].target.clone())
        else {
            return;
        };
        self.overlay = None;
        outcome.repaint = true;
        match target {
            PaletteTarget::Navigate(target) => {
                self.activate_navigator_target(target, outcome);
            }
            PaletteTarget::MachineDetails(endpoint_id) => self.open_machine_drawer(endpoint_id),
            PaletteTarget::Action(action) => {
                self.record_binding(crate::input::KeybindMatch::Action(action), outcome);
            }
            PaletteTarget::Command { command_id, action } => {
                self.invoke_command(command_id, action, outcome);
            }
            PaletteTarget::Notebook => self.open_notebook(),
            PaletteTarget::Home => self.open_home(),
            PaletteTarget::Launcher(index) => {
                if let Some(pane_id) = self.focused_pane_id() {
                    self.launch_here(pane_id, index, outcome);
                }
            }
        }
    }

    pub(super) fn insert_palette_text(&mut self, text: &str) -> bool {
        let Some(ClientShellOverlay::Palette(overlay)) = self.overlay.as_mut() else {
            return false;
        };
        if overlay.query.insert(text) {
            overlay.selected = 0;
            overlay.scroll = 0;
        }
        true
    }

    /// Keys while the palette is open. Returns whether the palette is open.
    pub(super) fn route_palette_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        use crossterm::event::KeyModifiers;
        if !matches!(self.overlay, Some(ClientShellOverlay::Palette(_))) {
            return false;
        }
        let (code, modifiers) = crate::config::normalize_key_combo((key.code, key.modifiers));
        let control = modifiers == KeyModifiers::CONTROL;
        outcome.repaint = true;
        match code {
            KeyCode::Esc => self.overlay = None,
            KeyCode::Enter => self.accept_palette(None, outcome),
            KeyCode::Up => self.move_palette_selection(-1),
            KeyCode::Down | KeyCode::Tab => self.move_palette_selection(1),
            KeyCode::BackTab => self.move_palette_selection(-1),
            KeyCode::PageUp => self.move_palette_selection(-8),
            KeyCode::PageDown => self.move_palette_selection(8),
            KeyCode::Char('p') | KeyCode::Char('k') if control => self.move_palette_selection(-1),
            KeyCode::Char('n') | KeyCode::Char('j') if control => self.move_palette_selection(1),
            _ => {
                if let Some(ClientShellOverlay::Palette(overlay)) = self.overlay.as_mut() {
                    if overlay.query.handle_key(key) == Some(true) {
                        overlay.selected = 0;
                        overlay.scroll = 0;
                    }
                }
            }
        }
        true
    }

    /// Mouse while the palette is open: click a row to run it, scroll to move,
    /// click outside to close. Returns whether the event was consumed.
    pub(super) fn route_palette_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if !matches!(self.overlay, Some(ClientShellOverlay::Palette(_))) {
            return false;
        }
        let point = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(position) = self
                    .hits
                    .palette_rows
                    .iter()
                    .find(|(rect, _)| super::contains(*rect, point))
                    .map(|(_, position)| *position)
                {
                    self.accept_palette(Some(position), outcome);
                } else if !super::contains(self.hits.palette_popup, point) {
                    self.overlay = None;
                    outcome.repaint = true;
                }
            }
            MouseEventKind::ScrollUp => {
                self.move_palette_selection(-3);
                outcome.repaint = true;
            }
            MouseEventKind::ScrollDown => {
                self.move_palette_selection(3);
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

    fn item(kind: PaletteKind, label: &str, detail: &str) -> PaletteItem {
        PaletteItem {
            kind,
            label: label.into(),
            detail: detail.into(),
            hint: String::new(),
            status: None,
            stale: false,
            target: PaletteTarget::Action(KeybindAction::NewTab),
        }
    }

    #[test]
    fn fuzzy_score_requires_every_term_and_prefers_word_starts() {
        assert!(fuzzy_score("nt", "new tab").is_some());
        assert!(fuzzy_score("xyz", "new tab").is_none());
        assert!(fuzzy_score("new gpu", "new tab").is_none());
        assert!(fuzzy_score("nt", "new tab") > fuzzy_score("nt", "mount"));
        assert!(fuzzy_score("tab", "new tab") > fuzzy_score("tab", "t a b"));
        assert!(fuzzy_score(
            "gpu",
            "Lumbridge TUI sidebar and left panel enhancements Local"
        )
        .is_none());
        assert!(fuzzy_score("nwtb", "new tab").is_some());
    }

    #[test]
    fn rank_puts_label_matches_first_and_keeps_order_for_empty_query() {
        let items = vec![
            item(PaletteKind::Workspace, "lumbridgecorp", "gpu"),
            item(PaletteKind::Agent, "Reviewer", "gpu · ~/src/app"),
            item(PaletteKind::Machine, "gpu", "online"),
        ];
        assert_eq!(rank(&items, ""), vec![0, 1, 2]);
        let ranked = rank(&items, "gpu");
        assert_eq!(ranked.first(), Some(&2), "exact label match wins");
        assert_eq!(ranked.len(), 3, "detail matches still count");
        assert_eq!(rank(&items, "reviewer"), vec![1]);
    }
}
