//! Notebook store: a folder of Markdown notes with optional TOML front matter.
//!
//! ```markdown
//! +++
//! title = "nas: backups failed"
//! tags = ["fleet", "nas"]
//! created = 2026-10-16T18:40:00Z
//! +++
//! The nightly backup job failed twice since…
//! ```
//!
//! Plain files on purpose: agents in panes read, grep and edit them with the
//! tools they already have, and the folder works with git. Every field is
//! optional; a note without front matter takes its title from its first
//! `# heading`, else its file name. YAML-style `---` front matter (`title:`,
//! `tags:`) is read too, since agents often write it.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Notes read per load; a bigger folder shows its newest ones.
pub(crate) const MAX_NOTES: usize = 2000;
/// Bytes of each note kept for preview and sending.
const MAX_NOTE_BYTES: usize = 256 * 1024;
const MAX_DEPTH: usize = 3;
/// Written once into a new notebook folder so agents working there know the format.
pub(crate) const GUIDE_FILE: &str = "AGENTS.md";
const GUIDE: &str = "# Notebook\n\n\
This folder is a notebook: one Markdown note per `.md` file, read by Lumbridge TUI's\n\
notebook drawer and by agents.\n\n\
- Optional TOML front matter between `+++` lines: `title` (string), `tags`\n  \
(list of strings), `created` (RFC 3339 date-time). Unknown keys are kept.\n\
- Without a `title`, the first `# heading` is the title, else the file name.\n\
- Subfolders are fine (up to three levels). Hidden files and folders are skipped.\n\
- Add to notes freely; keep each note about one thing so it is easy to find.\n\n\
## Templates and AI prompts\n\n\
lumbridge-notes reads these from `.lumbridge/` here; add or edit files to\n\
extend them.\n\n\
- `.lumbridge/templates/*.md`: front matter `title`, `description`; the body is the\n  \
note text, with `{{title}}`, `{{date}}` and `{{time}}` filled in.\n\
- `.lumbridge/prompts/*.md`: front matter `title`, `description` and `action`\n  \
(`replace`, `insert`, `append` or `answer`); the body is the instruction sent\n  \
to the AI together with the note or the selected text.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoteEntry {
    pub(crate) path: PathBuf,
    /// Path relative to the notebook folder, shown dimmed.
    pub(crate) relative: String,
    pub(crate) title: String,
    pub(crate) tags: Vec<String>,
    pub(crate) modified: SystemTime,
    /// The note without its front matter.
    pub(crate) body: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct FrontMatter {
    pub(crate) title: Option<String>,
    pub(crate) tags: Vec<String>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct TomlFront {
    title: Option<String>,
    tags: Vec<String>,
}

/// Splits `text` into front matter and body.
pub(crate) fn parse_note(text: &str) -> (FrontMatter, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for fence in ["+++", "---"] {
        let Some(rest) = text
            .strip_prefix(fence)
            .and_then(|rest| rest.strip_prefix('\n').or(rest.strip_prefix("\r\n")))
        else {
            continue;
        };
        let Some((header, body)) = split_at_fence(rest, fence) else {
            continue;
        };
        let front = if fence == "+++" {
            toml::from_str::<TomlFront>(header)
                .map(|front| FrontMatter {
                    title: front.title,
                    tags: front.tags,
                })
                .unwrap_or_default()
        } else {
            yaml_front(header)
        };
        return (front, body);
    }
    (FrontMatter::default(), text)
}

/// Finds a line that is exactly `fence`; returns the text before and after it.
fn split_at_fence<'a>(text: &'a str, fence: &str) -> Option<(&'a str, &'a str)> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == fence {
            return Some((&text[..offset], &text[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// The two YAML keys the drawer uses, read line by line (no YAML parser).
fn yaml_front(header: &str) -> FrontMatter {
    let unquote = |value: &str| value.trim().trim_matches(['"', '\'']).to_owned();
    let mut front = FrontMatter::default();
    for line in header.lines() {
        if let Some(value) = line.strip_prefix("title:") {
            front.title = Some(unquote(value)).filter(|title| !title.is_empty());
        } else if let Some(value) = line.strip_prefix("tags:") {
            front.tags = value
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(unquote)
                .filter(|tag| !tag.is_empty())
                .collect();
        }
    }
    front
}

fn heading_title(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .and_then(|line| line.strip_prefix("# "))
        .map(|title| title.trim().to_owned())
        .filter(|title| !title.is_empty())
}

fn clean(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_control()).collect()
}

/// Reads one note file.
fn read_note(root: &Path, path: &Path) -> Option<NoteEntry> {
    let metadata = std::fs::metadata(path).ok()?;
    let mut bytes = std::fs::read(path).ok()?;
    bytes.truncate(MAX_NOTE_BYTES);
    let text = String::from_utf8_lossy(&bytes);
    let (front, body) = parse_note(&text);
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = front.title.or_else(|| heading_title(body)).unwrap_or(stem);
    Some(NoteEntry {
        path: path.to_path_buf(),
        relative: clean(&relative),
        title: clean(&title),
        tags: front.tags.iter().map(|tag| clean(tag)).collect(),
        modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        body: body.to_owned(),
    })
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || (depth == 0 && name == GUIDE_FILE) {
            continue;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && depth + 1 < MAX_DEPTH {
            collect(&path, depth + 1, out);
        } else if kind.is_file() && name.ends_with(".md") {
            out.push(path);
        }
    }
}

/// Every note in `dir`, newest first. A missing folder is an empty notebook.
pub(crate) fn load_notes(dir: &Path) -> Vec<NoteEntry> {
    let mut paths = Vec::new();
    collect(dir, 0, &mut paths);
    let mut notes: Vec<NoteEntry> = paths
        .iter()
        .filter_map(|path| read_note(dir, path))
        .collect();
    notes.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.title.cmp(&b.title)));
    notes.truncate(MAX_NOTES);
    notes
}

/// File name for a new note: lowercase words joined by `-`.
pub(crate) fn slug(title: &str) -> String {
    let mut slug = String::new();
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            slug.extend(ch.to_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 60 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "note".to_owned()
    } else {
        slug.to_owned()
    }
}

/// The text of a new note.
pub(crate) fn new_note_text(title: &str, created: &str) -> String {
    let quoted = toml::Value::String(title.to_owned()).to_string();
    format!("+++\ntitle = {quoted}\ntags = []\ncreated = {created}\n+++\n\n# {title}\n\n")
}

/// Creates the folder (with its agent guide) if needed and a new note in it;
/// never overwrites an existing file.
pub(crate) fn create_note(dir: &Path, title: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let guide = dir.join(GUIDE_FILE);
    if !guide.exists() {
        std::fs::write(&guide, GUIDE)?;
    }
    let title = clean(title.trim());
    let title = if title.is_empty() {
        "untitled".to_owned()
    } else {
        title
    };
    let now = time::OffsetDateTime::now_utc();
    let created = now
        .replace_nanosecond(0)
        .unwrap_or(now)
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(std::io::Error::other)?;
    let base = slug(&title);
    for attempt in 0..1000 {
        let name = if attempt == 0 {
            format!("{base}.md")
        } else {
            format!("{base}-{}.md", attempt + 1)
        };
        let path = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(new_note_text(&title, &created).as_bytes())?;
                return Ok(path);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::other("no free note file name"))
}

/// The note as handed to an agent: its title, then its body.
pub(crate) fn note_for_agent(note: &NoteEntry) -> String {
    let body = note.body.trim();
    if heading_title(body).as_deref() == Some(note.title.as_str()) {
        body.to_owned()
    } else {
        format!("# {}\n\n{body}", note.title)
    }
}

/// `path` quoted for a POSIX shell.
pub(crate) fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// The shell command that opens `note` in `editor`. `lumbridge-notes` (by
/// name or by path, e.g. `~/.cargo/bin/lumbridge-notes`) also gets `--dir <dir>` so its picker and
/// search cover the same notebook, unless the setting already passes one.
pub(crate) fn editor_command(editor: &str, dir: &Path, note: &Path) -> String {
    if is_notes_editor(editor) {
        format!("{editor} --dir {} {}", shell_quote(dir), shell_quote(note))
    } else {
        format!("{editor} {}", shell_quote(note))
    }
}

/// Whether the editor setting runs lumbridge-notes without
/// its own `--dir`.
fn is_notes_editor(editor: &str) -> bool {
    let mut words = editor.split_whitespace();
    let program = words.next().map(Path::new).and_then(Path::file_name);
    let named = program.is_some_and(|name| {
        let name = name.to_string_lossy();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        crate::config::NOTES_EDITORS.contains(&name)
    });
    named && !words.any(|word| word == "--dir" || word.starts_with("--dir="))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_command_passes_the_notebook_dir_to_the_notes_editor() {
        let dir = Path::new("/srv/my notes");
        let note = Path::new("/srv/my notes/it's.md");
        assert_eq!(
            editor_command("lumbridge-notes", dir, note),
            r"lumbridge-notes --dir '/srv/my notes' '/srv/my notes/it'\''s.md'"
        );
        assert_eq!(
            editor_command(
                "/opt/bin/lumbridge-notes.exe",
                Path::new("/n"),
                Path::new("/n/a.md")
            ),
            "/opt/bin/lumbridge-notes.exe --dir '/n' '/n/a.md'"
        );
        assert_eq!(
            editor_command(
                "lumbridge-notes --dir=/x",
                Path::new("/n"),
                Path::new("/n/a.md")
            ),
            "lumbridge-notes --dir=/x '/n/a.md'"
        );
        assert_eq!(
            editor_command("hx", dir, note),
            r"hx '/srv/my notes/it'\''s.md'"
        );
        // By path too, and not twice when the setting already has one.
        assert_eq!(
            editor_command("/usr/local/bin/lumbridge-notes", dir, note),
            r"/usr/local/bin/lumbridge-notes --dir '/srv/my notes' '/srv/my notes/it'\''s.md'"
        );
        assert_eq!(
            editor_command(
                "~/.cargo/bin/lumbridge-notes",
                Path::new("/n"),
                Path::new("/n/a.md")
            ),
            "~/.cargo/bin/lumbridge-notes --dir '/n' '/n/a.md'"
        );
        assert_eq!(
            editor_command(
                "lumbridge-notes --dir /x",
                Path::new("/n"),
                Path::new("/n/a.md")
            ),
            "lumbridge-notes --dir /x '/n/a.md'"
        );
        assert_eq!(
            editor_command("not-lumbridge-notes", Path::new("/n"), Path::new("/n/a.md")),
            "not-lumbridge-notes '/n/a.md'"
        );
        assert_eq!(
            editor_command("lumbridge-notes-dev", Path::new("/n"), Path::new("/n/a.md")),
            "lumbridge-notes-dev '/n/a.md'"
        );
    }

    #[test]
    fn reads_toml_yaml_and_bare_notes() {
        let (front, body) =
            parse_note("+++\ntitle = \"Disk\"\ntags = [\"fleet\"]\nextra = 1\n+++\nbody\n");
        assert_eq!(front.title.as_deref(), Some("Disk"));
        assert_eq!(front.tags, vec!["fleet"]);
        assert_eq!(body, "body\n");

        let (front, body) = parse_note("---\ntitle: \"Plan\"\ntags: [a, 'b']\n---\n# x\n");
        assert_eq!(front.title.as_deref(), Some("Plan"));
        assert_eq!(front.tags, vec!["a", "b"]);
        assert_eq!(body, "# x\n");

        let (front, body) = parse_note("# Just text\n");
        assert_eq!(front, FrontMatter::default());
        assert_eq!(body, "# Just text\n");

        // An unclosed fence is body text, not front matter.
        let (_, body) = parse_note("+++\nnot closed\n");
        assert_eq!(body, "+++\nnot closed\n");
    }

    #[test]
    fn loads_folder_newest_first_and_creates_notes() {
        let dir = std::env::temp_dir().join(format!(
            "lumbridge-notebook-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let first = create_note(&dir, "NAS: port 8080 down").unwrap();
        assert!(first.ends_with("nas-port-8080-down.md"));
        let second = create_note(&dir, "NAS: port 8080 down").unwrap();
        assert!(second.ends_with("nas-port-8080-down-2.md"));
        assert!(dir.join(GUIDE_FILE).exists());
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/plain.md"), "# Heading title\n\ntext").unwrap();
        std::fs::write(dir.join(".hidden.md"), "# hidden").unwrap();
        std::fs::write(dir.join("other.txt"), "# not a note").unwrap();
        // lumbridge-notes recovery and temp files are not notes.
        let scratches = [
            "plain.md.lumbridge-notes-recovery",
            ".plain.md.lumbridge-notes-1-0.tmp",
        ];
        for scratch in scratches {
            std::fs::write(dir.join("sub").join(scratch), "# scratch").unwrap();
        }

        let notes = load_notes(&dir);
        let titles: Vec<&str> = notes.iter().map(|note| note.title.as_str()).collect();
        assert_eq!(notes.len(), 3, "{titles:?}");
        assert!(titles.contains(&"Heading title"));
        assert!(notes.iter().any(|note| note.relative == "sub/plain.md"));
        let created = notes
            .iter()
            .find(|note| note.path == first)
            .expect("created note");
        assert_eq!(created.title, "NAS: port 8080 down");
        assert_eq!(note_for_agent(created), "# NAS: port 8080 down");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_notes(&dir).is_empty());
    }

    #[test]
    fn slugs_and_quotes() {
        assert_eq!(slug("  Hello, World!  "), "hello-world");
        assert_eq!(slug("!!!"), "note");
        assert_eq!(shell_quote(Path::new("/a/it's.md")), r"'/a/it'\''s.md'");
    }
}

/// The golden format suite against the drawer's own copies of the note
/// format (`parse_note`, `heading_title`, `slug`, `new_note_text` and the
/// title rule in `read_note`).
///
/// The files are the ones `crates/lumbridge-notes/tests/golden.rs` checks
/// against `lumbridge_notes::store`, synced from Muxin by
/// `scripts/sync-notes-golden.sh`. The drawer reads only part of the format:
/// no `created`, no `summary`, no retitle or templates, so it is held to the
/// columns it has.
#[cfg(test)]
mod golden {
    use super::*;
    use std::collections::BTreeSet;

    fn golden() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/lumbridge-notes/tests/golden")
    }

    fn read(path: &Path) -> String {
        let bytes = std::fs::read(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        String::from_utf8(bytes).unwrap_or_else(|_| panic!("{} is not UTF-8", path.display()))
    }

    /// A table cell: `\\`, `\t`, `\n`, `\r`, and `\u{..}` for other controls.
    fn unescape(cell: &str) -> String {
        let mut out = String::with_capacity(cell.len());
        let mut chars = cell.chars();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('u') => {
                    assert_eq!(chars.next(), Some('{'), "bad escape in {cell:?}");
                    let hex: String = chars.by_ref().take_while(|&ch| ch != '}').collect();
                    let code = u32::from_str_radix(&hex, 16).expect("hex escape");
                    out.push(char::from_u32(code).expect("a char"));
                }
                other => panic!("bad escape \\{other:?} in {cell:?}"),
            }
        }
        out
    }

    /// Checks every row of `golden/<name>` (inputs, then the expected last
    /// cell) and fails with each row the drawer gets wrong.
    fn check_table(name: &str, drawer: impl Fn(&[String]) -> String) {
        let text = read(&golden().join(name));
        let mut lines = text.lines();
        assert!(lines.next().is_some_and(|header| header.starts_with("# ")));
        let mut rows = 0;
        let mut wrong = Vec::new();
        for (index, line) in lines.enumerate() {
            let cells: Vec<String> = line.split('\t').map(unescape).collect();
            let (expected, inputs) = cells.split_last().unwrap();
            let got = drawer(inputs);
            if &got != expected {
                wrong.push(format!(
                    "{name}:{}: {inputs:?}\n  blessed {expected:?}\n  drawer  {got:?}",
                    index + 2
                ));
            }
            rows += 1;
        }
        assert!(rows > 0, "{name} has rows");
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn slugs_match() {
        check_table("slugs.tsv", |cells| slug(&cells[0]));
    }

    #[test]
    fn new_notes_match() {
        check_table("new_note.tsv", |cells| new_note_text(&cells[0], &cells[1]));
    }

    /// Notes the drawer reads differently from `lumbridge_notes::store`, with
    /// exactly what the drawer makes of them. The drawer's copy predates
    /// three store fixes: a `---` block must look like YAML to be front
    /// matter (else it is a thematic break), a `tags` list with non-strings
    /// keeps its strings, and an empty `title` falls through to the heading.
    ///
    /// Pinned exactly, so any other difference fails, and so does a fix to
    /// the drawer until its row here goes. The rows go when the drawer moves
    /// onto the shared notes-core crate.
    const KNOWN_DRIFT: &[(&str, &str)] = &[
        (
            "thematic-break.md",
            r#"{"title":"Not front matter","tags":[],"body_start":40,"heading_title":"Not front matter"}"#,
        ),
        (
            "toml-tags-mixed.md",
            r#"{"title":"toml-tags-mixed","tags":[],"body_start":66,"heading_title":null}"#,
        ),
        (
            "toml-title-empty.md",
            r#"{"title":"","tags":[],"body_start":29,"heading_title":"Heading wins"}"#,
        ),
    ];

    #[test]
    fn notes_read_as_blessed() {
        let expected: serde_json::Value =
            serde_json::from_str(&read(&golden().join("expected.json"))).unwrap();
        let expected = expected.as_object().expect("expected.json is an object");
        assert!(expected.len() >= 30, "{} notes", expected.len());
        for (name, _) in KNOWN_DRIFT {
            assert!(expected.contains_key(*name), "KNOWN_DRIFT names {name}");
        }
        let root = golden();
        let mut wrong = Vec::new();
        for (name, want) in expected {
            let path = root.join("notes").join(name);
            let text = read(&path);
            let (front, body) = parse_note(&text);
            let entry = read_note(&root, &path).expect("a readable note");
            let drawer = serde_json::json!({
                "title": entry.title,
                "tags": front.tags,
                "body_start": text.len() - body.len(),
                "heading_title": heading_title(body).map(|title| clean(&title)),
            });
            let blessed = serde_json::json!({
                "title": want["title"],
                "tags": want["tags"],
                "body_start": want["body_start"],
                "heading_title": want["heading_title"],
            });
            let known = KNOWN_DRIFT.iter().find(|(drift, _)| drift == name);
            match known {
                None if drawer != blessed => {
                    wrong.push(format!("{name}:\n  blessed {blessed}\n  drawer  {drawer}"));
                }
                Some(_) if drawer == blessed => {
                    wrong.push(format!(
                        "{name}: the drawer reads it as blessed now; drop it from KNOWN_DRIFT"
                    ));
                }
                Some((_, pinned)) => {
                    let pinned: serde_json::Value = serde_json::from_str(pinned).unwrap();
                    if drawer != pinned {
                        wrong.push(format!(
                            "{name}:\n  blessed {blessed}\n  pinned  {pinned}\n  drawer  {drawer}"
                        ));
                    }
                }
                None => {}
            }
        }
        assert!(
            wrong.is_empty(),
            "the drawer reads {} notes differently:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    /// The drawer reads the same files the crate's suite checks: every file
    /// matches `MANIFEST.sha256`, so equal manifests mean equal fixtures.
    #[test]
    fn manifest_matches_the_files() {
        use sha2::{Digest, Sha256};
        let root = golden();
        let manifest = read(&root.join("MANIFEST.sha256"));
        let mut listed = BTreeSet::new();
        for line in manifest.lines() {
            let (sum, file) = line
                .split_once("  ")
                .unwrap_or_else(|| panic!("bad manifest line {line:?}"));
            let bytes =
                std::fs::read(root.join(file)).unwrap_or_else(|err| panic!("{file}: {err}"));
            let ours: String = Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(ours, sum, "{file} changed since the manifest was written");
            listed.insert(file.to_owned());
        }
        for required in ["expected.json", "slugs.tsv", "new_note.tsv"] {
            assert!(listed.contains(required), "the manifest lists {required}");
        }
        let notes = std::fs::read_dir(root.join("notes")).unwrap();
        for entry in notes {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            if name.ends_with(".md") {
                assert!(listed.contains(&format!("notes/{name}")), "notes/{name}");
            }
        }
    }
}
