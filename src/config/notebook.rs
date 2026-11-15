//! Notebook drawer settings (`[ui.notebook]`).

use std::ffi::OsStr;
use std::path::Path;

use serde::Deserialize;

/// The terminal note editor shipped alongside lumbridge-tui (Lumbridge Notes,
/// the `crates/lumbridge-notes` submodule). The drawer prefers it when it is
/// on `PATH` and hands it the notebook folder with `--dir`.
pub const NOTES_EDITOR: &str = "lumbridge-notes";
/// The note editor's binary names, found on `PATH` and given `--dir`.
pub const NOTES_EDITORS: [&str; 1] = [NOTES_EDITOR];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct NotebookConfig {
    /// Folder of Markdown notes. `~` expands to the home folder.
    pub dir: String,
    /// Command that edits a note (its path is appended). Empty:
    /// `lumbridge-notes` when it is on `PATH` (it
    /// is also given `--dir <dir>`), then `$VISUAL`, then `$EDITOR`, then
    /// `nano`.
    pub editor: String,
}

impl Default for NotebookConfig {
    fn default() -> Self {
        Self {
            dir: "~/notes".into(),
            editor: String::new(),
        }
    }
}

impl NotebookConfig {
    pub fn resolved_dir(&self) -> std::path::PathBuf {
        crate::worktree::expand_tilde_path(self.dir.trim())
    }

    /// The editor that opens a note: the configured `editor`, else
    /// `lumbridge-notes` when on `PATH`, else `$VISUAL`,
    /// `$EDITOR`, `nano`.
    /// Reads the environment, so call it when a note is opened, not per frame.
    pub fn resolved_editor(&self) -> String {
        resolve_editor(
            &self.editor,
            std::env::var_os("PATH").as_deref(),
            std::env::var("VISUAL").ok(),
            std::env::var("EDITOR").ok(),
        )
    }
}

/// Pure editor choice behind [`NotebookConfig::resolved_editor`], first match
/// wins: `configured`, `lumbridge-notes` found on
/// `path_var`, `visual`, `editor`, `nano`. Blank candidates and ones with
/// control characters are skipped.
fn resolve_editor(
    configured: &str,
    path_var: Option<&OsStr>,
    visual: Option<String>,
    editor: Option<String>,
) -> String {
    let usable = |candidate: &str| {
        let candidate = candidate.trim();
        (!candidate.is_empty() && !candidate.chars().any(char::is_control))
            .then(|| candidate.to_owned())
    };
    if let Some(configured) = usable(configured) {
        return configured;
    }
    if let Some(path_var) = path_var {
        if let Some(found) = NOTES_EDITORS
            .into_iter()
            .find(|name| executable_on_path(name, path_var))
        {
            return found.to_owned();
        }
    }
    [visual, editor]
        .into_iter()
        .flatten()
        .find_map(|candidate| usable(&candidate))
        .unwrap_or_else(|| "nano".to_owned())
}

/// Whether `name` is an executable file in one of the `PATH`-style
/// directories in `path_var`. Only stats one file per directory, so call it
/// when something opens, never per frame.
pub fn executable_on_path(name: &str, path_var: &OsStr) -> bool {
    std::env::split_paths(path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .any(|dir| is_executable(&dir.join(name)) || windows_exe(&dir, name))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

#[cfg(windows)]
fn windows_exe(dir: &Path, name: &str) -> bool {
    is_executable(&dir.join(format!("{name}.exe")))
}

#[cfg(not(windows))]
fn windows_exe(_dir: &Path, _name: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_notebook_settings() {
        let config: crate::config::Config =
            toml::from_str("[ui.notebook]\ndir = \"/srv/notes\"\neditor = \"hx\"\n").unwrap();
        assert_eq!(config.ui.notebook.dir, "/srv/notes");
        assert_eq!(config.ui.notebook.resolved_editor(), "hx");
        assert_eq!(crate::config::Config::default().ui.notebook.dir, "~/notes");
    }

    /// A temp `PATH` dir holding an executable for each name in `install`.
    fn path_dir(tag: &str, install: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lumbridge-notebook-path-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in install {
            let bin = if cfg!(windows) {
                dir.join(format!("{name}.exe"))
            } else {
                dir.join(name)
            };
            std::fs::write(&bin, "#!/bin/sh\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        dir
    }

    fn join(dirs: &[&Path]) -> std::ffi::OsString {
        std::env::join_paths(dirs).unwrap()
    }

    #[test]
    fn editor_order_prefers_config_then_notes_editor_then_env() {
        let with = path_dir("with", &[NOTES_EDITOR]);
        let without = path_dir("without", &[]);
        let found = join(&[&without, &with]);
        let missing = join(&[&without]);
        let env = || (Some("vis".to_owned()), Some("ed".to_owned()));

        let (visual, editor) = env();
        assert_eq!(resolve_editor(" hx ", Some(&found), visual, editor), "hx");
        let (visual, editor) = env();
        assert_eq!(
            resolve_editor("", Some(&found), visual, editor),
            NOTES_EDITOR
        );
        let (visual, editor) = env();
        assert_eq!(resolve_editor("", Some(&missing), visual, editor), "vis");
        assert_eq!(resolve_editor("", None, None, Some("ed".into())), "ed");
        assert_eq!(
            resolve_editor("\t", Some(&missing), Some("  ".into()), None),
            "nano"
        );
        assert_eq!(
            resolve_editor("", Some(&missing), Some("a\nb".into()), None),
            "nano"
        );

        let _ = std::fs::remove_dir_all(with);
        let _ = std::fs::remove_dir_all(without);
    }

    #[test]
    fn path_lookup_needs_an_executable_file() {
        let empty = path_dir("empty", &[]);
        assert!(!executable_on_path(NOTES_EDITOR, &join(&[&empty])));
        assert!(!executable_on_path(NOTES_EDITOR, OsStr::new("")));

        // A directory with the right name is not an executable.
        std::fs::create_dir_all(empty.join(NOTES_EDITOR)).unwrap();
        assert!(!executable_on_path(NOTES_EDITOR, &join(&[&empty])));

        #[cfg(unix)]
        {
            let plain = path_dir("plain", &[]);
            std::fs::write(plain.join(NOTES_EDITOR), "not executable").unwrap();
            assert!(!executable_on_path(NOTES_EDITOR, &join(&[&plain])));
            let _ = std::fs::remove_dir_all(plain);
        }

        let installed = path_dir("installed", &[NOTES_EDITOR]);
        assert!(executable_on_path(
            NOTES_EDITOR,
            &join(&[&empty, &installed])
        ));

        let _ = std::fs::remove_dir_all(empty);
        let _ = std::fs::remove_dir_all(installed);
    }
}
