//! Machine feeder: an external command that supplies per-machine sidebar values.
//!
//! `[ui.sidebar.machines] feeder` names an argv command. The client runs it on a
//! background thread every `feeder_interval_secs` and expects one JSON document
//! on stdout:
//!
//! ```json
//! {"machines": {"desk": {"tokens": {"load": "0.2", "disk": "78"},
//!                        "services": [{"name": "ci-runner", "state": "up"}]}}}
//! ```
//!
//! Keys under `machines` are machine labels as the sidebar shows them (`Local`
//! for this machine). Values feed `$name` tokens and the `services` token in
//! machine rows. Rendering never waits on the feeder; it reads the last result.
//!
//! Two optional fields feed the machine detail drawer: `series` maps a metric
//! name to recent samples, oldest first (drawn as a sparkline), and `alerts`
//! lists active alerts as `{"name", "severity", "summary"}`.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tracing::warn;

use super::events::ClientLoopEvent;

const MAX_FEEDER_OUTPUT_BYTES: usize = 1 << 20;
const MAX_FEEDER_RUNTIME: Duration = Duration::from_secs(10);
const MAX_TOKEN_VALUE_CHARS: usize = 64;
const MAX_SERVICES_PER_MACHINE: usize = 32;
const MAX_SERIES_PER_MACHINE: usize = 8;
const MAX_SERIES_POINTS: usize = 240;
const MAX_ALERTS_PER_MACHINE: usize = 32;
const MAX_ALERT_TEXT_CHARS: usize = 160;
/// After this many intervals without a good run, drop the last values rather
/// than keep showing numbers nobody is measuring any more.
const STALE_AFTER_INTERVALS: u32 = 3;
const SETTINGS_POLL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ServiceState {
    Up,
    Down,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct ServiceDot {
    #[serde(default)]
    pub(crate) name: String,
    pub(crate) state: ServiceState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct FeedAlert {
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) severity: String,
    #[serde(default)]
    pub(crate) summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub(crate) struct MachineFeedEntry {
    #[serde(default)]
    pub(crate) tokens: HashMap<String, String>,
    #[serde(default)]
    pub(crate) services: Vec<ServiceDot>,
    #[serde(default)]
    pub(crate) series: HashMap<String, Vec<f64>>,
    #[serde(default)]
    pub(crate) alerts: Vec<FeedAlert>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub(crate) struct MachineFeed {
    #[serde(default)]
    pub(crate) machines: HashMap<String, MachineFeedEntry>,
}

impl MachineFeed {
    pub(crate) fn entry(&self, label: &str) -> Option<&MachineFeedEntry> {
        self.machines.get(label)
    }
}

/// Feeder output is untrusted text drawn into the terminal.
fn clean_text(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(max_chars)
        .collect()
}

pub(crate) fn parse_feed(output: &[u8]) -> Result<MachineFeed, String> {
    let mut feed: MachineFeed =
        serde_json::from_slice(output).map_err(|error| format!("invalid feeder JSON: {error}"))?;
    for entry in feed.machines.values_mut() {
        for value in entry.tokens.values_mut() {
            *value = clean_text(value, MAX_TOKEN_VALUE_CHARS);
        }
        entry.services.truncate(MAX_SERVICES_PER_MACHINE);
        for service in &mut entry.services {
            service.name = clean_text(&service.name, MAX_TOKEN_VALUE_CHARS);
        }
        if entry.series.len() > MAX_SERIES_PER_MACHINE {
            let mut names = entry.series.keys().cloned().collect::<Vec<_>>();
            names.sort();
            for name in names.split_off(MAX_SERIES_PER_MACHINE) {
                entry.series.remove(&name);
            }
        }
        for points in entry.series.values_mut() {
            points.retain(|value| value.is_finite());
            let excess = points.len().saturating_sub(MAX_SERIES_POINTS);
            points.drain(..excess);
        }
        entry.alerts.truncate(MAX_ALERTS_PER_MACHINE);
        for alert in &mut entry.alerts {
            alert.name = clean_text(&alert.name, MAX_TOKEN_VALUE_CHARS);
            alert.severity = clean_text(&alert.severity, 16);
            alert.summary = clean_text(&alert.summary, MAX_ALERT_TEXT_CHARS);
        }
    }
    Ok(feed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FeederSettings {
    command: Vec<String>,
    interval: Duration,
}

static SETTINGS: Mutex<FeederSettings> = Mutex::new(FeederSettings {
    command: Vec::new(),
    interval: Duration::from_secs(15),
});

/// Called whenever the client (re)loads `[ui.sidebar.machines]`.
pub(crate) fn configure(command: &[String], interval_secs: u64) {
    let mut settings = SETTINGS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    settings.command = command.to_vec();
    settings.interval = Duration::from_secs(interval_secs.max(1));
}

fn current_settings() -> FeederSettings {
    SETTINGS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn run_once(command: &[String]) -> Result<MachineFeed, String> {
    let (program, args) = command.split_first().ok_or("feeder command is empty")?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not start feeder `{program}`: {error}"))?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("feeder stdout was not captured".into());
    };
    // Read on a side thread so a chatty feeder cannot block on a full pipe
    // while we wait for it to exit.
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        let _ = stdout
            .take(MAX_FEEDER_OUTPUT_BYTES as u64 + 1)
            .read_to_end(&mut output);
        output
    });
    let deadline = Instant::now() + MAX_FEEDER_RUNTIME;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "feeder `{program}` did not finish within {}s",
                    MAX_FEEDER_RUNTIME.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(format!("could not wait for feeder: {error}")),
        }
    };
    let output = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(format!("feeder `{program}` exited with {status}"));
    }
    if output.len() > MAX_FEEDER_OUTPUT_BYTES {
        return Err("feeder output exceeded 1 MiB".into());
    }
    parse_feed(&output)
}

pub(super) fn watch(
    event_tx: tokio::sync::mpsc::Sender<ClientLoopEvent>,
    should_quit: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let mut settings: Option<FeederSettings> = None;
        let mut published = MachineFeed::default();
        let mut next_run = Instant::now();
        let mut last_success: Option<Instant> = None;
        while !should_quit.load(Ordering::Acquire) {
            let now = Instant::now();
            let latest = current_settings();
            if settings.as_ref() != Some(&latest) {
                settings = Some(latest.clone());
                next_run = now;
                last_success = None;
            }
            let mut next = None;
            if latest.command.is_empty() {
                next = Some(MachineFeed::default());
            } else if now >= next_run {
                next_run = now + latest.interval;
                match run_once(&latest.command) {
                    Ok(feed) => {
                        last_success = Some(now);
                        next = Some(feed);
                    }
                    Err(error) => {
                        warn!(%error, "machine feeder run failed");
                        let stale = last_success.is_none_or(|success| {
                            now.duration_since(success) > latest.interval * STALE_AFTER_INTERVALS
                        });
                        if stale {
                            next = Some(MachineFeed::default());
                        }
                    }
                }
            }
            if let Some(feed) = next.filter(|feed| *feed != published) {
                published = feed.clone();
                if event_tx
                    .blocking_send(ClientLoopEvent::MachineFeed(feed))
                    .is_err()
                {
                    break;
                }
            }
            std::thread::sleep(SETTINGS_POLL);
        }
    });
}

/// Sends a `ClockTick` every 30 seconds until the client quits.
pub(super) fn tick(
    event_tx: tokio::sync::mpsc::Sender<ClientLoopEvent>,
    should_quit: Arc<AtomicBool>,
) {
    const TICK: Duration = Duration::from_secs(30);
    std::thread::spawn(move || {
        let mut next = Instant::now() + TICK;
        while !should_quit.load(Ordering::Acquire) {
            std::thread::sleep(SETTINGS_POLL);
            if Instant::now() < next {
                continue;
            }
            next = Instant::now() + TICK;
            if event_tx.blocking_send(ClientLoopEvent::ClockTick).is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tokens_and_services_and_tolerates_missing_fields() {
        let feed = parse_feed(
            br#"{"machines": {
                "desk": {"tokens": {"load": "0.2"},
                          "services": [{"name": "a", "state": "up"},
                                       {"name": "b", "state": "down"},
                                       {"name": "c", "state": "degraded"}]},
                "gpu": {}
            }}"#,
        )
        .unwrap();
        let desk = feed.entry("desk").unwrap();
        assert_eq!(desk.tokens.get("load").map(String::as_str), Some("0.2"));
        assert_eq!(
            desk.services
                .iter()
                .map(|service| service.state)
                .collect::<Vec<_>>(),
            vec![ServiceState::Up, ServiceState::Down, ServiceState::Unknown]
        );
        assert_eq!(feed.entry("gpu"), Some(&MachineFeedEntry::default()));
        assert!(feed.entry("web").is_none());
    }

    #[test]
    fn strips_control_characters_and_caps_value_length() {
        let long = "x".repeat(200);
        let feed = parse_feed(
            format!(
                r#"{{"machines": {{"m": {{"tokens": {{"a": "o\u001b[2Jk", "b": "{long}"}}}}}}}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let tokens = &feed.entry("m").unwrap().tokens;
        assert_eq!(tokens["a"], "o[2Jk");
        assert_eq!(tokens["b"].chars().count(), MAX_TOKEN_VALUE_CHARS);
    }

    #[test]
    fn parses_series_and_alerts_and_bounds_them() {
        let points = (0..300)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let feed = parse_feed(
            format!(
                r#"{{"machines": {{"m": {{
                    "series": {{"load": [{points}]}},
                    "alerts": [{{"name": "Disk\u001b", "severity": "critical", "summary": "90% full"}}]
                }}}}}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let entry = feed.entry("m").unwrap();
        let load = &entry.series["load"];
        assert_eq!(load.len(), MAX_SERIES_POINTS);
        assert_eq!(load.last(), Some(&299.0), "keeps the newest samples");
        assert_eq!(entry.alerts[0].name, "Disk");
        assert_eq!(entry.alerts[0].summary, "90% full");
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_feed(b"not json").is_err());
    }

    #[test]
    fn runs_a_real_command_and_reports_failures() {
        let ok = run_once(&[
            "sh".into(),
            "-c".into(),
            r#"printf '{"machines":{"Local":{"tokens":{"x":"1"}}}}'"#.into(),
        ])
        .unwrap();
        assert_eq!(ok.entry("Local").unwrap().tokens["x"], "1");
        assert!(run_once(&["sh".into(), "-c".into(), "exit 3".into()]).is_err());
        assert!(run_once(&["/nonexistent/lumbridge-feeder".into()]).is_err());
    }
}
