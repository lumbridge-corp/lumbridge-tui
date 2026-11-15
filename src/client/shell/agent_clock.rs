//! Client-side record of when each agent entered its current state.
//!
//! The server reports a state-change sequence number but no timestamps, so the
//! client notes the moment it sees each change. An agent first seen mid-state
//! (after attach, or a new machine) has an unknown start; its time shows with a
//! `+` suffix, meaning "at least this long".

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::api::schema::AgentStatus;
use crate::client::endpoint::ClientEndpointId;
use crate::protocol::ClientShellSnapshot;

#[derive(Debug, Clone, Copy)]
struct Entry {
    status: AgentStatus,
    seq: u64,
    since: Instant,
    /// The change was seen happen, so `since` is exact.
    exact: bool,
}

#[derive(Debug, Default)]
pub(crate) struct AgentClock {
    entries: HashMap<(ClientEndpointId, String), Entry>,
}

impl AgentClock {
    /// Records state changes in one endpoint's snapshot and forgets its agents
    /// that are gone.
    pub(super) fn observe(
        &mut self,
        endpoint_id: &ClientEndpointId,
        snapshot: &ClientShellSnapshot,
        now: Instant,
    ) {
        for agent in &snapshot.agents {
            let key = (endpoint_id.clone(), agent.pane_id.clone());
            match self.entries.get_mut(&key) {
                Some(entry)
                    if entry.status == agent.agent_status
                        && entry.seq == agent.state_change_seq => {}
                Some(entry) => {
                    if entry.status != agent.agent_status {
                        entry.since = now;
                        entry.exact = true;
                    }
                    entry.status = agent.agent_status;
                    entry.seq = agent.state_change_seq;
                }
                None => {
                    self.entries.insert(
                        key,
                        Entry {
                            status: agent.agent_status,
                            seq: agent.state_change_seq,
                            since: now,
                            exact: false,
                        },
                    );
                }
            }
        }
        self.entries.retain(|(endpoint, pane_id), _| {
            endpoint != endpoint_id
                || snapshot
                    .agents
                    .iter()
                    .any(|agent| &agent.pane_id == pane_id)
        });
    }

    /// Time in the current state, formatted for the sidebar (`3m`, `2h+`).
    pub(super) fn elapsed(
        &self,
        endpoint_id: &ClientEndpointId,
        pane_id: &str,
        now: Instant,
    ) -> Option<String> {
        let entry = self
            .entries
            .get(&(endpoint_id.clone(), pane_id.to_owned()))?;
        let elapsed = now.saturating_duration_since(entry.since);
        let mut text = format_elapsed(elapsed);
        if !entry.exact {
            // "<1m+" says nothing; wait until the lower bound means something.
            if elapsed < Duration::from_secs(60) {
                return None;
            }
            text.push('+');
        }
        Some(text)
    }
}

pub(super) fn format_elapsed(duration: Duration) -> String {
    let secs = duration.as_secs();
    match secs {
        0..60 => "<1m".to_owned(),
        60..3_600 => format!("{}m", secs / 60),
        3_600..86_400 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_coarse_durations() {
        assert_eq!(format_elapsed(Duration::from_secs(5)), "<1m");
        assert_eq!(format_elapsed(Duration::from_secs(185)), "3m");
        assert_eq!(format_elapsed(Duration::from_secs(7_300)), "2h");
        assert_eq!(format_elapsed(Duration::from_secs(200_000)), "2d");
    }
}
