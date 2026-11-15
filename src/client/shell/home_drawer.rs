//! Lumbridge Home: a small launcher popup opened from the "⌂ Lumbridge" sidebar
//! button or the palette's "home" entry.
//!
//! Each tile either opens another client overlay (the notebook, the palette)
//! or runs a program in a split beside the focused pane, exactly like a
//! `[[ui.launchers]]` button. Whether a program is on `PATH` is resolved when
//! Home opens and kept in the overlay, never looked up while rendering.
//!
//! Running a program uses only generation-1 endpoint features (`pane.split`
//! plus ordinary pane input), so it works against stock servers.

use super::render::{display_width, put_right_text, put_text};
use super::*;
use crossterm::event::{MouseButton, MouseEventKind};
use std::ffi::OsStr;

const DRAWER_WIDTH: u16 = 56;
/// The narrowest Home that still reads; below this nothing is drawn.
const MIN_DRAWER_WIDTH: u16 = 30;
/// Rows around the tile list: border, title, rule, rule, footer, border.
const CHROME_ROWS: u16 = 6;

/// What choosing a tile does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HomeAction {
    Notes,
    Palette,
    /// Runs `command` in a split when `binary` is on `PATH`; otherwise the
    /// tile is disabled and shows `missing`.
    Run {
        binary: &'static str,
        command: &'static str,
        missing: &'static str,
    },
}

#[derive(Clone, Copy, Debug)]
pub(super) struct HomeTile {
    pub(super) icon: &'static str,
    pub(super) label: &'static str,
    pub(super) detail: &'static str,
    pub(super) action: HomeAction,
}

/// The Home tiles, in order; number keys follow this order. Adding a tile is
/// one line. Only tools that ship with Lumbridge TUI belong here; a program of
/// your own goes in your config as a `[[ui.launchers]]` button.
#[rustfmt::skip] // keep one tile per line
pub(super) const HOME_TILES: &[HomeTile] = &[
    HomeTile { icon: "✎", label: "Notes", detail: "search and open notes", action: HomeAction::Notes },
    HomeTile { icon: "◈", label: "Fleet", detail: "machines and services", action: HomeAction::Run { binary: "lumbridge-fleet", command: "lumbridge-fleet --table", missing: "lumbridge-fleet is not on PATH" } },
    HomeTile { icon: "⌕", label: "Palette", detail: "every command", action: HomeAction::Palette },
];

/// Whether a tile can run now, decided when Home opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum HomeTileState {
    Ready,
    /// Shown dimmed with this hint; choosing it only shows the hint.
    Disabled(String),
}

#[derive(Debug)]
pub(super) struct ClientHomeOverlay {
    /// Index into [`HOME_TILES`].
    pub(super) selected: usize,
    /// One entry per tile in [`HOME_TILES`].
    pub(super) tiles: Vec<HomeTileState>,
    /// A one-line result (why a tile could not run).
    pub(super) message: Option<String>,
}

pub(super) struct HomeRender {
    pub(super) area: Rect,
    pub(super) rows: Vec<(Rect, usize)>,
    pub(super) close: Rect,
}

/// Each tile's state. `path_var` is the `PATH` to search; `local` and
/// `has_pane` say whether a program could run beside a focused pane on this
/// computer (where `PATH` was searched).
pub(super) fn resolve_tiles(
    tiles: &[HomeTile],
    path_var: Option<&OsStr>,
    local: bool,
    has_pane: bool,
) -> Vec<HomeTileState> {
    tiles
        .iter()
        .map(|tile| match tile.action {
            HomeAction::Notes | HomeAction::Palette => HomeTileState::Ready,
            HomeAction::Run {
                binary, missing, ..
            } => {
                if !path_var.is_some_and(|path| crate::config::executable_on_path(binary, path)) {
                    HomeTileState::Disabled(missing.to_owned())
                } else if !local {
                    HomeTileState::Disabled("needs the local machine active".to_owned())
                } else if !has_pane {
                    HomeTileState::Disabled("needs a focused pane".to_owned())
                } else {
                    HomeTileState::Ready
                }
            }
        })
        .collect()
}

pub(super) fn render_home_drawer(
    buffer: &mut Buffer,
    home: &ClientHomeOverlay,
    left: u16,
    palette: &Palette,
) -> Option<HomeRender> {
    let screen = buffer.area;
    // Beside the sidebar when it fits, else shifted left over the sidebar.
    let left = left
        .min(screen.right().saturating_sub(MIN_DRAWER_WIDTH + 1))
        .max(screen.x);
    let width = DRAWER_WIDTH.min(screen.right().saturating_sub(left).saturating_sub(1));
    let height = (HOME_TILES.len() as u16 + CHROME_ROWS).min(screen.height.saturating_sub(1));
    if width < MIN_DRAWER_WIDTH || height < CHROME_ROWS + 1 {
        return None;
    }
    let area = Rect::new(left, screen.y + 1, width, height);
    let panel = |style: Style| style.bg(palette.panel_bg);
    for row in area.y..area.bottom() {
        for column in area.x..area.right() {
            buffer[(column, row)]
                .set_symbol(" ")
                .set_style(panel(Style::default().fg(palette.text)));
        }
    }
    {
        use ratatui::widgets::{Block, BorderType, Widget};
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(panel(Style::default().fg(palette.accent)))
            .render(area, buffer);
    }
    let inner = Rect::new(area.x + 2, area.y + 1, area.width - 4, area.height - 2);

    // Title row.
    put_text(
        buffer,
        inner.x,
        inner.y,
        inner.width,
        "⌂ Lumbridge",
        panel(
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
    );
    let close = Rect::new(inner.right().saturating_sub(1), inner.y, 1, 1);
    put_text(
        buffer,
        close.x,
        close.y,
        1,
        "×",
        panel(Style::default().fg(palette.overlay1)),
    );
    let rule = "─".repeat(usize::from(inner.width));
    let rule_style = panel(Style::default().fg(palette.surface1));
    put_text(buffer, inner.x, inner.y + 1, inner.width, &rule, rule_style);

    // Tiles.
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(4),
    );
    // On a short screen the list shows the page holding the selected tile,
    // so it is always drawn and hit-testable. Paging (not minimal scrolling)
    // keeps the page still while the mouse hovers any drawn row, and needs
    // no scroll state.
    let visible = usize::from(list.height).max(1);
    let offset = home.selected / visible * visible;
    let mut rows = Vec::with_capacity(HOME_TILES.len().min(visible));
    for (slot, (index, tile)) in HOME_TILES
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible)
        .enumerate()
    {
        let y = list.y + slot as u16;
        let row = Rect::new(list.x, y, list.width, 1);
        let ready = home.tiles.get(index) == Some(&HomeTileState::Ready);
        let selected = index == home.selected;
        let base = if selected {
            Style::default().bg(palette.active_row_bg)
        } else {
            panel(Style::default())
        };
        for x in row.x..row.right() {
            buffer[(x, y)].set_symbol(" ").set_style(base);
        }
        let label_style = if ready {
            base.fg(palette.text).add_modifier(Modifier::BOLD)
        } else {
            base.fg(palette.overlay0)
        };
        let number = format!("{} ", index + 1);
        let mut x = row.x;
        put_text(buffer, x, y, 2, &number, base.fg(palette.overlay0));
        x = x.saturating_add(2);
        put_text(
            buffer,
            x,
            y,
            2,
            tile.icon,
            if ready {
                base.fg(palette.accent)
            } else {
                base.fg(palette.overlay0)
            },
        );
        x = x.saturating_add(2);
        put_text(
            buffer,
            x,
            y,
            row.right().saturating_sub(x),
            tile.label,
            label_style,
        );
        x = x.saturating_add(display_width(tile.label) + 2);
        let detail = match home.tiles.get(index) {
            Some(HomeTileState::Disabled(hint)) => hint.as_str(),
            _ => tile.detail,
        };
        if x < row.right() {
            put_right_text(
                buffer,
                Rect::new(x, y, row.right().saturating_sub(x), 1),
                y,
                detail,
                base.fg(palette.overlay0),
            );
        }
        rows.push((row, index));
    }

    // Footer.
    let footer_y = inner.bottom().saturating_sub(1);
    put_text(
        buffer,
        inner.x,
        footer_y - 1,
        inner.width,
        &rule,
        rule_style,
    );
    let (hint, color) = match home.message.as_deref() {
        Some(message) => (message, palette.yellow),
        None => ("↑↓ move · enter open · 1-9 · esc", palette.overlay0),
    };
    put_text(
        buffer,
        inner.x,
        footer_y,
        inner.width,
        hint,
        panel(Style::default().fg(color)),
    );
    Some(HomeRender { area, rows, close })
}

impl ClientShellState {
    /// Opens Home, deciding which tiles can run (one `PATH` search per
    /// program tile, here and not while rendering).
    pub(super) fn open_home(&mut self) {
        let path = std::env::var_os("PATH");
        self.open_home_with_path(path.as_deref());
    }

    pub(super) fn open_home_with_path(&mut self, path_var: Option<&OsStr>) {
        let tiles = resolve_tiles(
            HOME_TILES,
            path_var,
            self.active_endpoint_id.is_local(),
            self.focused_pane_id().is_some(),
        );
        self.overlay = Some(ClientShellOverlay::Home(ClientHomeOverlay {
            selected: 0,
            tiles,
            message: None,
        }));
    }

    fn home(&mut self) -> Option<&mut ClientHomeOverlay> {
        match self.overlay.as_mut() {
            Some(ClientShellOverlay::Home(home)) => Some(home),
            _ => None,
        }
    }

    fn move_home_selection(&mut self, delta: isize) {
        if let Some(home) = self.home() {
            let last = HOME_TILES.len().saturating_sub(1);
            home.selected = home.selected.saturating_add_signed(delta).min(last);
            home.message = None;
        }
    }

    /// Runs tile `index`. A disabled tile only shows why.
    pub(super) fn activate_home_tile(&mut self, index: usize, outcome: &mut ClientShellInput) {
        let Some(tile) = HOME_TILES.get(index) else {
            return;
        };
        let Some(home) = self.home() else {
            return;
        };
        outcome.repaint = true;
        home.selected = index;
        if let Some(HomeTileState::Disabled(hint)) = home.tiles.get(index) {
            home.message = Some(hint.clone());
            return;
        }
        match tile.action {
            HomeAction::Notes => self.open_notebook(),
            HomeAction::Palette => self.open_palette(),
            HomeAction::Run { command, .. } => {
                let Some(pane_id) = self.focused_pane_id() else {
                    if let Some(home) = self.home() {
                        home.message = Some("needs a focused pane".to_owned());
                    }
                    return;
                };
                self.overlay = None;
                self.split_and_run(pane_id, command.to_owned(), false, outcome);
            }
        }
    }

    /// Keys while Home is open. Returns whether it is open.
    pub(super) fn route_home_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let Some(home) = self.home() else {
            return false;
        };
        let selected = home.selected;
        let (code, _) = crate::config::normalize_key_combo((key.code, key.modifiers));
        outcome.repaint = true;
        match code {
            KeyCode::Esc => self.overlay = None,
            KeyCode::Enter => self.activate_home_tile(selected, outcome),
            KeyCode::Up | KeyCode::Left | KeyCode::BackTab | KeyCode::Char('k') => {
                self.move_home_selection(-1)
            }
            KeyCode::Down | KeyCode::Right | KeyCode::Tab | KeyCode::Char('j') => {
                self.move_home_selection(1)
            }
            KeyCode::Home => self.move_home_selection(-(HOME_TILES.len() as isize)),
            KeyCode::End => self.move_home_selection(HOME_TILES.len() as isize),
            KeyCode::Char(digit @ '1'..='9') => {
                let index = usize::from(digit as u8 - b'1');
                self.activate_home_tile(index, outcome);
            }
            _ => {}
        }
        true
    }

    /// Mouse while Home is open. Returns whether the event was consumed.
    pub(super) fn route_home_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if self.home().is_none() {
            return false;
        }
        let point = (mouse.column, mouse.row);
        let row = self
            .hits
            .home_rows
            .iter()
            .find(|(rect, _)| super::contains(*rect, point))
            .map(|(_, index)| *index);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                outcome.repaint = true;
                if super::contains(self.hits.home_close, point) {
                    self.overlay = None;
                } else if let Some(index) = row {
                    self.activate_home_tile(index, outcome);
                } else if !super::contains(self.hits.home_area, point) {
                    self.overlay = None;
                }
            }
            MouseEventKind::Moved => {
                if let (Some(index), Some(home)) = (row, self.home()) {
                    if home.selected != index {
                        home.selected = index;
                        home.message = None;
                        outcome.repaint = true;
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                self.move_home_selection(1);
                outcome.repaint = true;
            }
            MouseEventKind::ScrollUp => {
                self.move_home_selection(-1);
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

    fn fake_path(name: &str, binaries: &[&str]) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lumbridge-home-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        for binary in binaries {
            let path = dir.join(binary);
            std::fs::write(&path, "#!/bin/sh\n").expect("binary");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                    .expect("chmod");
            }
        }
        dir
    }

    fn tile_index(label: &str) -> usize {
        HOME_TILES
            .iter()
            .position(|tile| tile.label == label)
            .expect("tile")
    }

    #[test]
    fn tiles_are_ready_only_when_their_program_is_on_path() {
        let dir = fake_path("resolve", &["lumbridge-fleet"]);
        let tiles = resolve_tiles(HOME_TILES, Some(dir.as_os_str()), true, true);
        assert_eq!(tiles.len(), HOME_TILES.len());
        assert_eq!(tiles[tile_index("Notes")], HomeTileState::Ready);
        assert_eq!(tiles[tile_index("Palette")], HomeTileState::Ready);
        assert_eq!(tiles[tile_index("Fleet")], HomeTileState::Ready);
        let empty = fake_path("resolve-empty", &[]);
        let missing = resolve_tiles(HOME_TILES, Some(empty.as_os_str()), true, true);
        assert_eq!(
            missing[tile_index("Fleet")],
            HomeTileState::Disabled("lumbridge-fleet is not on PATH".into())
        );
        let _ = std::fs::remove_dir_all(&empty);

        // On PATH but not runnable here.
        let remote = resolve_tiles(HOME_TILES, Some(dir.as_os_str()), false, true);
        assert_eq!(
            remote[tile_index("Fleet")],
            HomeTileState::Disabled("needs the local machine active".into())
        );
        let paneless = resolve_tiles(HOME_TILES, Some(dir.as_os_str()), true, false);
        assert_eq!(
            paneless[tile_index("Fleet")],
            HomeTileState::Disabled("needs a focused pane".into())
        );
        // No PATH at all disables every program tile.
        let none = resolve_tiles(HOME_TILES, None, true, true);
        assert!(matches!(
            none[tile_index("Fleet")],
            HomeTileState::Disabled(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tiles_fit_the_number_keys() {
        assert!(HOME_TILES.len() <= 9);
    }

    #[test]
    fn renders_title_tiles_and_disabled_hints() {
        let palette = Palette::catppuccin();
        let home = ClientHomeOverlay {
            selected: 0,
            tiles: resolve_tiles(HOME_TILES, None, true, true),
            message: None,
        };
        let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 30));
        let rendered = render_home_drawer(&mut buffer, &home, 20, &palette).expect("drawn");
        assert_eq!(rendered.rows.len(), HOME_TILES.len());
        let text = (rendered.area.y..rendered.area.bottom())
            .map(|y| {
                (rendered.area.x..rendered.area.right())
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        for expected in [
            "Lumbridge",
            "Notes",
            "Fleet",
            "lumbridge-fleet is not on PATH",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in\n{text}");
        }
        // No room beside the sidebar: shifted left over it, not dropped.
        let mut tight = Buffer::empty(Rect::new(0, 0, 65, 30));
        let shifted = render_home_drawer(&mut tight, &home, 37, &palette).expect("shifted");
        assert_eq!(shifted.area.width, MIN_DRAWER_WIDTH);
        assert!(shifted.area.right() < 65);
        // A screen too narrow for any Home: nothing is drawn.
        let mut small = Buffer::empty(Rect::new(0, 0, 30, 30));
        assert!(render_home_drawer(&mut small, &home, 20, &palette).is_none());
    }

    #[test]
    fn short_screen_scrolls_to_keep_the_selected_tile_drawn() {
        let palette = Palette::catppuccin();
        let mut home = ClientHomeOverlay {
            selected: 0,
            tiles: resolve_tiles(HOME_TILES, None, true, true),
            message: None,
        };
        // 8 rows: 7 for the popup, so only 1 tile fits.
        let area = Rect::new(0, 0, 100, 8);
        for selected in 0..HOME_TILES.len() {
            home.selected = selected;
            let mut buffer = Buffer::empty(area);
            let rendered = render_home_drawer(&mut buffer, &home, 20, &palette).expect("drawn");
            assert!(rendered.rows.len() < HOME_TILES.len());
            assert!(
                rendered.rows.iter().any(|(_, index)| *index == selected),
                "selected tile {selected} not drawn: {:?}",
                rendered.rows
            );
            // Each row is drawn inside the popup, one per line.
            for (slot, (rect, _)) in rendered.rows.iter().enumerate() {
                assert_eq!(rect.y, rendered.rows[0].0.y + slot as u16);
                assert!(rect.bottom() <= rendered.area.bottom());
            }
        }
    }
}
