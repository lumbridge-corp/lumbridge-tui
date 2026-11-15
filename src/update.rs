//! No updater: Lumbridge TUI is built from source.
//!
//! There is no self-update, background version check or release-notes
//! fetch; nothing here opens a network connection. What there is:
//!
//! - [`Version`], used by plugin `min_lumbridge_version` checks and the
//!   local release notes.
//! - `lumbridge-tui update`, which only explains how to rebuild.
//! - Package-manager install detection, used by remote attach to decide
//!   whether the running binary can be copied to a remote host.
//! - A local test hook ([`FAKE_UPDATE_VERSION_ENV`]) that shows the
//!   "update ready" notice without any network access.

use std::env;
use std::path::{Path, PathBuf};

/// Set to a version to exercise the update notice locally (no network).
pub(crate) const FAKE_UPDATE_VERSION_ENV: &str = "LUMBRIDGE_TUI_FAKE_UPDATE_VERSION";
const REBUILD_COMMAND: &str = "cargo build --release";
const MISE_INSTALLS_DIR_ENV: &str = "MISE_INSTALLS_DIR";
const NO_UPDATER: &str = "lumbridge-tui has no updater: it is built from source. Pull the repository and run `cargo build --release`, then restart the server.";

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

/// Parsed semver version for comparison.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        Some(Self {
            major: parts[0].parse().ok()?,
            minor: parts[1].parse().ok()?,
            patch: parts[2].parse().ok()?,
        })
    }

    pub fn current() -> Self {
        Self::parse(crate::build_info::BASE_VERSION).expect("invalid CARGO_PKG_VERSION")
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ---------------------------------------------------------------------------
// The `update` command
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SelfUpdateOptions;

pub(crate) fn parse_self_update_args(args: &[String]) -> Result<SelfUpdateOptions, String> {
    match args {
        [] => Ok(SelfUpdateOptions),
        [arg] if arg == "--handoff" => Ok(SelfUpdateOptions),
        [arg, ..] if arg == "--help" || arg == "-h" => {
            Err("usage: lumbridge-tui update (prints how to rebuild)".to_string())
        }
        [arg, ..] => Err(format!("unknown update option: {arg}")),
    }
}

/// `lumbridge-tui update`: there is nothing to download; say how to rebuild.
pub fn self_update(_options: SelfUpdateOptions) -> Result<Version, String> {
    Err(NO_UPDATER.to_string())
}

/// Background update check. A no-op that never opens a connection; only the
/// local [`FAKE_UPDATE_VERSION_ENV`] hook shows the notice.
pub fn auto_update(events: tokio::sync::mpsc::Sender<crate::events::AppEvent>) {
    let Ok(version) = env::var(FAKE_UPDATE_VERSION_ENV) else {
        return;
    };
    let version = version.trim();
    if version.is_empty() {
        return;
    }
    let _ = events.blocking_send(crate::events::AppEvent::UpdateReady {
        version: version.to_string(),
        install_command: update_install_command().to_string(),
    });
}

/// Windows remote installs copy this installer next to a local package.
pub(crate) const WINDOWS_INSTALLER: &str = include_str!("../distribution/install.ps1");

// ---------------------------------------------------------------------------
// Installation manager detection
// ---------------------------------------------------------------------------

pub(crate) fn update_install_command() -> &'static str {
    REBUILD_COMMAND
}

pub(crate) fn update_install_instruction(install_command: &str) -> String {
    format!("detach, run `{install_command}`, then run Lumbridge TUI again to reconnect")
}

#[cfg(unix)]
pub(crate) fn is_package_manager_managed_exe_path(path: &Path) -> bool {
    is_homebrew_managed_exe_path_following_links(path)
        || is_mise_managed_exe_path_following_links(path)
        || is_nix_store_exe_path_following_links(path)
}

#[cfg(not(unix))]
pub(crate) fn is_package_manager_managed_exe_path(_path: &Path) -> bool {
    false
}

fn is_homebrew_managed_exe_path_following_links(path: &Path) -> bool {
    if is_homebrew_managed_exe_path(path) {
        return true;
    }

    path.canonicalize()
        .is_ok_and(|path| is_homebrew_managed_exe_path(&path))
}

fn is_nix_store_exe_path_following_links(path: &Path) -> bool {
    if is_nix_store_exe_path(path) {
        return true;
    }

    path.canonicalize()
        .is_ok_and(|path| is_nix_store_exe_path(&path))
}

fn is_mise_managed_exe_path_following_links(path: &Path) -> bool {
    if is_mise_managed_exe_path(path) {
        return true;
    }

    path.canonicalize()
        .is_ok_and(|path| is_mise_managed_exe_path(&path))
}

fn is_nix_store_exe_path(path: &Path) -> bool {
    path.starts_with("/nix/store")
}

fn is_mise_managed_exe_path(path: &Path) -> bool {
    mise_install_root(path).is_some()
}

fn mise_install_root(path: &Path) -> Option<PathBuf> {
    if let Some(root) = mise_install_root_under_configured_installs_dir(path) {
        return Some(root);
    }

    mise_install_root_under_named_installs_dir(path)
}

fn mise_install_root_under_configured_installs_dir(path: &Path) -> Option<PathBuf> {
    let installs_dir = env::var_os(MISE_INSTALLS_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())?;
    let version_dir = mise_tool_version_dir(path)?;
    let tool_dir = version_dir.parent()?;
    paths_match(tool_dir.parent()?, &installs_dir).then_some(version_dir.to_path_buf())
}

fn mise_install_root_under_named_installs_dir(path: &Path) -> Option<PathBuf> {
    let version_dir = mise_tool_version_dir(path)?;
    let tool_dir = version_dir.parent()?;
    let installs_dir = tool_dir.parent()?;
    if installs_dir.file_name()? != "installs" {
        return None;
    }
    Some(version_dir.to_path_buf())
}

fn mise_tool_version_dir(path: &Path) -> Option<&Path> {
    if path.file_name()? != "lumbridge-tui" {
        return None;
    }
    let bin_dir = path.parent()?;
    if bin_dir.file_name()? != "bin" {
        return None;
    }
    let version_dir = bin_dir.parent()?;
    let tool_dir = version_dir.parent()?;
    if tool_dir.file_name()? != "lumbridge-tui" {
        return None;
    }
    Some(version_dir)
}

fn paths_match(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }

    let Ok(left) = left.canonicalize() else {
        return false;
    };
    let Ok(right) = right.canonicalize() else {
        return false;
    };
    left == right
}

fn is_homebrew_managed_exe_path(path: &Path) -> bool {
    homebrew_cellar_keg_root(path).is_some()
}

fn homebrew_cellar_keg_root(path: &Path) -> Option<PathBuf> {
    if path.file_name()? != "lumbridge-tui" {
        return None;
    }
    let bin_dir = path.parent()?;
    if bin_dir.file_name()? != "bin" {
        return None;
    }
    let version_dir = bin_dir.parent()?;
    let formula_dir = version_dir.parent()?;
    if formula_dir.file_name()? != "lumbridge-tui" {
        return None;
    }
    let cellar_dir = formula_dir.parent()?;
    if cellar_dir.file_name()? != "Cellar" {
        return None;
    }
    Some(version_dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(
            Version::parse("v1.2.3"),
            Some(Version {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        assert!(Version::parse("1.2").is_none());
        assert!(Version::parse("1.2.x").is_none());
        assert!(Version::parse("0.10.0") > Version::parse("0.9.5"));
        assert_eq!(Version::parse("0.9.5").unwrap().to_string(), "0.9.5");
        let _ = Version::current();
    }

    #[test]
    fn update_never_downloads_and_explains_how_to_rebuild() {
        let err = self_update(SelfUpdateOptions).unwrap_err();
        assert!(err.contains("built from source"), "{err}");
        assert!(parse_self_update_args(&[]).is_ok());
        assert!(parse_self_update_args(&["--bogus".into()]).is_err());
        assert_eq!(
            update_install_instruction(update_install_command()),
            "detach, run `cargo build --release`, then run Lumbridge TUI again to reconnect"
        );
    }

    #[test]
    fn package_manager_installs_are_recognised_by_path() {
        assert!(is_homebrew_managed_exe_path(Path::new(
            "/opt/homebrew/Cellar/lumbridge-tui/0.9.5/bin/lumbridge-tui"
        )));
        assert!(is_nix_store_exe_path(Path::new(
            "/nix/store/abc-lumbridge-tui/bin/lumbridge-tui"
        )));
        assert!(is_mise_managed_exe_path(Path::new(
            "/home/u/.local/share/mise/installs/lumbridge-tui/0.9.5/bin/lumbridge-tui"
        )));
        assert!(!is_package_manager_managed_exe_path(Path::new(
            "/home/u/src/lumbridge-tui/target/release/lumbridge-tui"
        )));
    }
}
