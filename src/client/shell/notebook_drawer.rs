//! Notebook drawer: search the notebook folder (`[ui.notebook] dir`), preview
//! a note, open it in an editor split, send it to the pane that was focused
//! (usually an agent), copy it, or start a new note.
//!
//! Notes are read from disk when the drawer opens or reloads, never while
//! rendering. Opening runs the editor on the local machine, so it needs the
//! local machine active; sending and copying work against any machine.

use super::render::{display_width, put_right_text, put_text};
use super::*;
use crate::client::notebook::{self, NoteEntry};
use crossterm::event::{MouseButton, MouseEventKind};
use unicode_width::UnicodeWidthChar;

const DRAWER_MAX_WIDTH: u16 = 110;
const LIST_WIDTH: u16 = 34;
/// Narrower than this, the preview is left out.
const PREVIEW_MIN_WIDTH: u16 = 72;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NotebookButton {
    Open,
    Send,
    Copy,
    New,
}

#[derive(Debug)]
pub(super) struct ClientNotebookDrawer {
    pub(super) notes: Vec<NoteEntry>,
    pub(super) query: TextEditor,
    /// Index into the filtered list.
    pub(super) selected: usize,
    pub(super) list_scroll: usize,
    pub(super) preview_scroll: usize,
    /// The pane focused when the drawer opened; "send" pastes into it.
    pub(super) target_pane: Option<String>,
    pub(super) target_label: String,
    /// A one-line result ("copied", or why something failed).
    pub(super) message: Option<String>,
}

pub(super) struct NotebookRender {
    pub(super) area: Rect,
    pub(super) rows: Vec<(Rect, usize)>,
    pub(super) list: Rect,
    pub(super) preview: Rect,
    pub(super) buttons: Vec<(Rect, NotebookButton)>,
    pub(super) close: Rect,
    pub(super) cursor: Option<crate::protocol::CursorState>,
}

/// Filtered note indices, best first. Every query term must match the title,
/// tags or path (fuzzy) or appear in the body (plain substring, ranked lower).
pub(super) fn filter_notes(notes: &[NoteEntry], query: &str) -> Vec<usize> {
    let query = query.trim();
    if query.is_empty() {
        return (0..notes.len()).collect();
    }
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut scored: Vec<(i32, usize)> = notes
        .iter()
        .enumerate()
        .filter_map(|(index, note)| {
            let heading = format!("{} {} {}", note.title, note.tags.join(" "), note.relative);
            if let Some(score) = super::palette::fuzzy_score(query, &heading) {
                return Some((score + 1000, index));
            }
            let body = note.body.to_lowercase();
            terms
                .iter()
                .all(|term| body.contains(term.as_str()) || heading.to_lowercase().contains(term))
                .then_some((0, index))
        })
        .collect();
    // Stable sort keeps newest-first among equal scores.
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, index)| index).collect()
}

/// Wraps `text` to `width` display columns, breaking after the last space
/// that fits (or mid-word when a word is wider than the line). Tabs become
/// four spaces.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut line: Vec<(char, usize)> = Vec::new();
        let mut used = 0;
        for ch in raw.replace('\t', "    ").chars() {
            if ch.is_control() {
                continue;
            }
            let ch_width = ch.width().unwrap_or(0);
            if used + ch_width > width && !line.is_empty() {
                let rest = match line.iter().rposition(|(c, _)| *c == ' ') {
                    Some(space) if space > 0 => line.split_off(space + 1),
                    _ => Vec::new(),
                };
                lines.push(
                    line.iter()
                        .map(|(c, _)| *c)
                        .collect::<String>()
                        .trim_end()
                        .to_owned(),
                );
                line = rest;
                used = line.iter().map(|(_, w)| w).sum();
            }
            line.push((ch, ch_width));
            used += ch_width;
        }
        lines.push(line.iter().map(|(c, _)| *c).collect());
    }
    lines
}

fn age(modified: std::time::SystemTime) -> String {
    std::time::SystemTime::now()
        .duration_since(modified)
        .map(super::agent_clock::format_elapsed)
        .unwrap_or_default()
}

pub(super) fn render_notebook_drawer(
    buffer: &mut Buffer,
    drawer: &mut ClientNotebookDrawer,
    dir_label: &str,
    can_open: bool,
    left: u16,
    palette: &Palette,
) -> Option<NotebookRender> {
    let screen = buffer.area;
    let width = DRAWER_MAX_WIDTH.min(screen.right().saturating_sub(left).saturating_sub(1));
    if width < 40 || screen.height < 12 {
        return None;
    }
    let area = Rect::new(left, screen.y + 1, width, screen.height.saturating_sub(2));
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
    let filtered = filter_notes(&drawer.notes, drawer.query.as_str());
    drawer.selected = drawer.selected.min(filtered.len().saturating_sub(1));

    // Title row.
    put_text(
        buffer,
        inner.x,
        inner.y,
        inner.width,
        "notebook",
        panel(
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        ),
    );
    let count = if filtered.len() == drawer.notes.len() {
        let count = drawer.notes.len();
        format!(
            "{dir_label} · {count} note{}",
            if count == 1 { "" } else { "s" }
        )
    } else {
        format!("{dir_label} · {} of {}", filtered.len(), drawer.notes.len())
    };
    put_text(
        buffer,
        inner.x + 10,
        inner.y,
        inner.width.saturating_sub(13),
        &count,
        panel(Style::default().fg(palette.overlay0)),
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

    // Search row.
    put_text(
        buffer,
        inner.x,
        inner.y + 1,
        2,
        "› ",
        panel(
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
    );
    let query_area = Rect::new(inner.x + 2, inner.y + 1, inner.width.saturating_sub(2), 1);
    let cursor = super::text_editor::render(
        buffer,
        query_area,
        &drawer.query,
        panel(Style::default().fg(palette.text)),
    );
    if drawer.query.as_str().is_empty() {
        put_text(
            buffer,
            query_area.x,
            query_area.y,
            query_area.width,
            "search notes, or type a title and press ctrl-n",
            panel(Style::default().fg(palette.overlay0)),
        );
    }
    let rule = "─".repeat(usize::from(inner.width));
    let rule_style = panel(Style::default().fg(palette.surface1));
    put_text(buffer, inner.x, inner.y + 2, inner.width, &rule, rule_style);

    // Body: list, then preview when there is room.
    let body = Rect::new(
        inner.x,
        inner.y + 3,
        inner.width,
        inner.height.saturating_sub(5),
    );
    let with_preview = inner.width >= PREVIEW_MIN_WIDTH;
    let list = if with_preview {
        Rect::new(body.x, body.y, LIST_WIDTH, body.height)
    } else {
        body
    };
    let visible = usize::from(list.height);
    if drawer.selected < drawer.list_scroll {
        drawer.list_scroll = drawer.selected;
    } else if visible > 0 && drawer.selected >= drawer.list_scroll + visible {
        drawer.list_scroll = drawer.selected + 1 - visible;
    }
    let mut rows = Vec::new();
    if filtered.is_empty() {
        let empty = if drawer.notes.is_empty() {
            "no notes yet: type a title, press ctrl-n"
        } else {
            "no matching notes"
        };
        put_text(
            buffer,
            list.x,
            list.y,
            list.width,
            empty,
            panel(Style::default().fg(palette.overlay0)),
        );
    }
    for (offset, (position, &index)) in filtered
        .iter()
        .enumerate()
        .skip(drawer.list_scroll)
        .take(visible)
        .enumerate()
    {
        let note = &drawer.notes[index];
        let y = list.y + offset as u16;
        let row = Rect::new(list.x, y, list.width, 1);
        let selected = position == drawer.selected;
        let style = if selected {
            Style::default()
                .fg(palette.text)
                .bg(palette.active_row_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            panel(Style::default().fg(palette.subtext0))
        };
        for x in row.x..row.right() {
            buffer[(x, y)].set_symbol(" ").set_style(style);
        }
        let when = age(note.modified);
        let title_room = list.width.saturating_sub(display_width(&when) + 2);
        put_text(
            buffer,
            row.x,
            y,
            title_room,
            &crate::ui::truncate_end(&note.title, usize::from(title_room)),
            style,
        );
        put_right_text(
            buffer,
            Rect::new(row.x, y, row.width.saturating_sub(1), 1),
            y,
            &when,
            style.fg(palette.overlay0),
        );
        rows.push((row, position));
    }

    let preview = if with_preview {
        let divider_x = list.right() + 1;
        for y in body.y..body.bottom() {
            put_text(buffer, divider_x, y, 1, "│", rule_style);
        }
        Rect::new(
            divider_x + 2,
            body.y,
            body.right().saturating_sub(divider_x + 2),
            body.height,
        )
    } else {
        Rect::default()
    };
    if let (true, Some(&index)) = (with_preview, filtered.get(drawer.selected)) {
        let note = &drawer.notes[index];
        let mut lines: Vec<(String, Style)> = vec![(
            note.title.clone(),
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        )];
        let mut meta = note.relative.clone();
        if !note.tags.is_empty() {
            meta = format!("{} · {meta}", note.tags.join(", "));
        }
        lines.push((meta, Style::default().fg(palette.overlay0)));
        lines.push((String::new(), Style::default()));
        // The title is already the first line; drop a heading that repeats it.
        let body = note.body.trim_start();
        let body = body
            .strip_prefix(&format!("# {}", note.title))
            .filter(|rest| rest.is_empty() || rest.starts_with('\n'))
            .unwrap_or(body)
            .trim_start_matches('\n');
        for line in wrap(body, usize::from(preview.width)) {
            let style = if line.starts_with('#') {
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.text)
            };
            lines.push((line, style));
        }
        drawer.preview_scroll = drawer
            .preview_scroll
            .min(lines.len().saturating_sub(usize::from(preview.height)));
        for (offset, (text, style)) in lines
            .iter()
            .skip(drawer.preview_scroll)
            .take(usize::from(preview.height))
            .enumerate()
        {
            put_text(
                buffer,
                preview.x,
                preview.y + offset as u16,
                preview.width,
                text,
                panel(*style),
            );
        }
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
    let mut buttons = Vec::new();
    let mut x = inner.x;
    let has_note = !filtered.is_empty();
    let send_label = format!("send to {}", drawer.target_label);
    for (label, button, enabled) in [
        ("open", NotebookButton::Open, has_note && can_open),
        (
            send_label.as_str(),
            NotebookButton::Send,
            has_note && drawer.target_pane.is_some(),
        ),
        ("copy", NotebookButton::Copy, has_note),
        ("+ new", NotebookButton::New, true),
    ] {
        if !enabled {
            continue;
        }
        let rect = super::render::put_button(
            buffer,
            x,
            footer_y,
            label,
            palette.subtext0,
            crate::config::SidebarButtonStyle::Pill,
            false,
            false,
            palette,
        );
        x = rect.right() + 1;
        buttons.push((rect, button));
    }
    let hint = drawer
        .message
        .clone()
        .unwrap_or_else(|| "enter open · ^s send · ^y copy · ^n new · esc".to_owned());
    let room = inner.right().saturating_sub(x + 1);
    if display_width(&hint) <= room {
        let color = if drawer.message.is_some() {
            palette.yellow
        } else {
            palette.overlay0
        };
        put_right_text(
            buffer,
            inner,
            footer_y,
            &hint,
            panel(Style::default().fg(color)),
        );
    }
    Some(NotebookRender {
        area,
        rows,
        list,
        preview,
        buttons,
        close,
        cursor,
    })
}

impl ClientShellState {
    /// Opens the drawer with the notebook freshly read from disk.
    pub(super) fn open_notebook(&mut self) {
        let target_pane = self.focused_pane_id();
        let target_label = target_pane
            .as_deref()
            .and_then(|pane_id| {
                let snapshot = self.snapshot.as_deref()?;
                let agent = snapshot
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id == pane_id)?;
                agent.display_agent.clone().or_else(|| agent.agent.clone())
            })
            .unwrap_or_else(|| "pane".to_owned());
        self.overlay = Some(ClientShellOverlay::Notebook(ClientNotebookDrawer {
            notes: notebook::load_notes(&self.config.notebook.resolved_dir()),
            query: TextEditor::default(),
            selected: 0,
            list_scroll: 0,
            preview_scroll: 0,
            target_pane,
            target_label,
            message: None,
        }));
    }

    pub(super) fn notebook_dir_label(&self) -> String {
        let dir = self.config.notebook.dir.trim();
        if dir.is_empty() {
            "~/notes".to_owned()
        } else {
            dir.to_owned()
        }
    }

    /// Opening edits a file on this computer, so it needs the local machine
    /// active (a remote machine's pane could not reach the file).
    pub(super) fn notebook_can_open(&self) -> bool {
        self.active_endpoint_id.is_local() && self.focused_pane_id().is_some()
    }

    fn notebook(&mut self) -> Option<&mut ClientNotebookDrawer> {
        match self.overlay.as_mut() {
            Some(ClientShellOverlay::Notebook(drawer)) => Some(drawer),
            _ => None,
        }
    }

    fn selected_note(&self) -> Option<NoteEntry> {
        let Some(ClientShellOverlay::Notebook(drawer)) = self.overlay.as_ref() else {
            return None;
        };
        let filtered = filter_notes(&drawer.notes, drawer.query.as_str());
        filtered
            .get(drawer.selected)
            .map(|&index| drawer.notes[index].clone())
    }

    fn set_notebook_message(&mut self, message: impl Into<String>) {
        if let Some(drawer) = self.notebook() {
            drawer.message = Some(message.into());
        }
    }

    fn open_note_path(&mut self, path: &std::path::Path, outcome: &mut ClientShellInput) {
        if !self.notebook_can_open() {
            self.set_notebook_message("open needs the local machine active");
            return;
        }
        let Some(pane_id) = self.focused_pane_id() else {
            return;
        };
        let command = notebook::editor_command(
            &self.config.notebook.resolved_editor(),
            &self.config.notebook.resolved_dir(),
            path,
        );
        self.overlay = None;
        self.split_and_run(pane_id, command, false, outcome);
    }

    pub(super) fn notebook_action(
        &mut self,
        button: NotebookButton,
        outcome: &mut ClientShellInput,
    ) {
        outcome.repaint = true;
        match button {
            NotebookButton::Open => {
                if let Some(note) = self.selected_note() {
                    self.open_note_path(&note.path, outcome);
                }
            }
            NotebookButton::Send => {
                let target = self
                    .notebook()
                    .and_then(|drawer| drawer.target_pane.clone());
                let (Some(note), Some(pane_id)) = (self.selected_note(), target) else {
                    return;
                };
                self.overlay = None;
                super::push_target_event(
                    ClientInputTarget::Pane(pane_id),
                    crate::protocol::ClientPaneInputEvent::Paste(notebook::note_for_agent(&note)),
                    outcome,
                );
            }
            NotebookButton::Copy => {
                if let Some(note) = self.selected_note() {
                    outcome.actions.push(ClientShellAction::ClipboardWrite(
                        notebook::note_for_agent(&note).into_bytes(),
                    ));
                    self.set_notebook_message(format!("copied “{}”", note.title));
                }
            }
            NotebookButton::New => {
                let title = self
                    .notebook()
                    .map(|drawer| drawer.query.as_str().to_owned())
                    .unwrap_or_default();
                let dir = self.config.notebook.resolved_dir();
                match notebook::create_note(&dir, &title) {
                    Ok(path) => {
                        if self.notebook_can_open() {
                            self.open_note_path(&path, outcome);
                        } else {
                            let notes = notebook::load_notes(&dir);
                            if let Some(drawer) = self.notebook() {
                                drawer.notes = notes;
                                drawer.query.clear();
                                drawer.selected = 0;
                                drawer.message = Some(format!(
                                    "created {}",
                                    path.file_name()
                                        .map(|name| name.to_string_lossy().into_owned())
                                        .unwrap_or_default()
                                ));
                            }
                        }
                    }
                    Err(err) => self.set_notebook_message(format!("could not create note: {err}")),
                }
            }
        }
    }

    fn reload_notebook(&mut self) {
        let notes = notebook::load_notes(&self.config.notebook.resolved_dir());
        if let Some(drawer) = self.notebook() {
            drawer.notes = notes;
            drawer.message = Some("reloaded".to_owned());
        }
    }

    fn move_notebook_selection(&mut self, delta: isize) {
        if let Some(drawer) = self.notebook() {
            drawer.selected = drawer.selected.saturating_add_signed(delta);
            drawer.preview_scroll = 0;
        }
    }

    fn scroll_notebook_preview(&mut self, delta: isize) {
        if let Some(drawer) = self.notebook() {
            drawer.preview_scroll = drawer.preview_scroll.saturating_add_signed(delta);
        }
    }

    pub(super) fn insert_notebook_text(&mut self, text: &str) -> bool {
        let Some(drawer) = self.notebook() else {
            return false;
        };
        if drawer.query.insert(text) {
            drawer.selected = 0;
            drawer.list_scroll = 0;
            drawer.preview_scroll = 0;
            drawer.message = None;
        }
        true
    }

    /// Keys while the drawer is open. Returns whether it is open.
    pub(super) fn route_notebook_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        use crossterm::event::KeyModifiers;
        if self.notebook().is_none() {
            return false;
        }
        let (code, modifiers) = crate::config::normalize_key_combo((key.code, key.modifiers));
        let control = modifiers == KeyModifiers::CONTROL;
        outcome.repaint = true;
        match code {
            KeyCode::Esc => self.overlay = None,
            KeyCode::Enter => self.notebook_action(NotebookButton::Open, outcome),
            KeyCode::Char('s') if control => self.notebook_action(NotebookButton::Send, outcome),
            KeyCode::Char('y') if control => self.notebook_action(NotebookButton::Copy, outcome),
            KeyCode::Char('n') if control => self.notebook_action(NotebookButton::New, outcome),
            KeyCode::Char('r') if control => self.reload_notebook(),
            KeyCode::Up => self.move_notebook_selection(-1),
            KeyCode::Down | KeyCode::Tab => self.move_notebook_selection(1),
            KeyCode::BackTab => self.move_notebook_selection(-1),
            KeyCode::PageDown => self.scroll_notebook_preview(8),
            KeyCode::PageUp => self.scroll_notebook_preview(-8),
            _ => {
                if let Some(drawer) = self.notebook() {
                    if drawer.query.handle_key(key) == Some(true) {
                        drawer.selected = 0;
                        drawer.list_scroll = 0;
                        drawer.preview_scroll = 0;
                        drawer.message = None;
                    }
                }
            }
        }
        true
    }

    /// Mouse while the drawer is open. Returns whether the event was consumed.
    pub(super) fn route_notebook_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if self.notebook().is_none() {
            return false;
        }
        let point = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                outcome.repaint = true;
                if super::contains(self.hits.notebook_close, point) {
                    self.overlay = None;
                } else if let Some(button) = self
                    .hits
                    .notebook_buttons
                    .iter()
                    .find(|(rect, _)| super::contains(*rect, point))
                    .map(|(_, button)| *button)
                {
                    self.notebook_action(button, outcome);
                } else if let Some(position) = self
                    .hits
                    .notebook_rows
                    .iter()
                    .find(|(rect, _)| super::contains(*rect, point))
                    .map(|(_, position)| *position)
                {
                    // A click selects; a click on the selected note opens it.
                    let already = self
                        .notebook()
                        .is_some_and(|drawer| drawer.selected == position);
                    if already {
                        self.notebook_action(NotebookButton::Open, outcome);
                    } else if let Some(drawer) = self.notebook() {
                        drawer.selected = position;
                        drawer.preview_scroll = 0;
                    }
                } else if !super::contains(self.hits.notebook_area, point) {
                    self.overlay = None;
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let delta = if mouse.kind == MouseEventKind::ScrollDown {
                    3
                } else {
                    -3
                };
                if super::contains(self.hits.notebook_preview, point) {
                    self.scroll_notebook_preview(delta);
                    outcome.repaint = true;
                } else if super::contains(self.hits.notebook_list, point) {
                    self.move_notebook_selection(delta.signum());
                    outcome.repaint = true;
                }
            }
            _ => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: &str, body: &str, tags: &[&str]) -> NoteEntry {
        NoteEntry {
            path: std::path::PathBuf::from(format!("/n/{title}.md")),
            relative: format!("{title}.md"),
            title: title.to_owned(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            modified: std::time::SystemTime::UNIX_EPOCH,
            body: body.to_owned(),
        }
    }

    #[test]
    fn filters_by_title_then_body() {
        let notes = vec![
            note("groceries", "milk and eggs", &[]),
            note("nas ports", "8080 down", &["fleet"]),
            note("ideas", "a nas in the closet", &[]),
        ];
        assert_eq!(filter_notes(&notes, ""), vec![0, 1, 2]);
        // Title/tag matches rank above body matches.
        assert_eq!(filter_notes(&notes, "nas"), vec![1, 2]);
        assert_eq!(filter_notes(&notes, "fleet"), vec![1]);
        assert_eq!(filter_notes(&notes, "eggs"), vec![0]);
        assert!(filter_notes(&notes, "zebra").is_empty());
    }

    #[test]
    fn wraps_by_display_width() {
        assert_eq!(wrap("abcdef", 4), vec!["abcd", "ef"]);
        assert_eq!(wrap("one two three", 8), vec!["one two", "three"]);
        assert_eq!(wrap("a\n\nb", 4), vec!["a", "", "b"]);
        assert_eq!(wrap("模块组织", 4), vec!["模块", "组织"]);
    }
}
