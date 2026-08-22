use super::github::pr_url_for_branch;
use super::{PrRow, PrState};
use crate::git::git_branch;
use crate::herdr::{AgentStatus, HerdrClient, Method, plugin_id};
use crate::ids::{PaneId, WorkspaceId};
use crate::util::string_at;
use anyhow::Result;
use regex::Regex;
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;
use std::thread;

#[derive(Debug)]
pub(super) struct PrCandidate {
    pub(super) cwd: PathBuf,
    pub(super) url: String,
    pub(super) agent_name: String,
    pub(super) agent_status: AgentStatus,
    pub(super) branch: String,
    pub(super) workspace_id: WorkspaceId,
    pub(super) pane_id: PaneId,
}

impl PrRow {
    /// Resolve every active agent to its PR, fetch details, and sort for display.
    pub(super) fn load_all(client: &HerdrClient) -> Result<Vec<PrRow>> {
        let agents = client.list_agents()?;
        clear_pr_metadata(client, &agents);

        // GitHub lookups can be slow when several agents are active. Resolve each
        // agent concurrently, then deduplicate before fetching full PR details.
        let mut candidates = thread::scope(|scope| {
            let handles = agents
                .into_iter()
                .map(|agent| {
                    let client = client.clone();
                    scope.spawn(move || PrCandidate::from_agent(&client, &agent))
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok().flatten())
                .collect::<Vec<_>>()
        });

        let mut seen_urls = HashSet::new();
        candidates.retain(|candidate| seen_urls.insert(candidate.url.clone()));

        let mut rows = thread::scope(|scope| {
            let handles = candidates
                .into_iter()
                .map(|candidate| scope.spawn(move || candidate.fetch_pr_row()))
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok().flatten())
                .collect::<Vec<_>>()
        });

        // `PrState` derives `Ord` in display order, so sorting needs no rank table.
        rows.sort_by(|a, b| a.state.cmp(&b.state).then_with(|| a.number.cmp(&b.number)));
        report_pr_metadata(client, &rows);
        Ok(rows)
    }
}

impl PrCandidate {
    /// Classify one `agent.list` entry, resolving its branch and PR URL.
    fn from_agent(client: &HerdrClient, agent: &serde_json::Value) -> Option<Self> {
        let pane_id = string_at(agent, &["pane_id"]).map(PaneId::from)?;
        let workspace_id = string_at(agent, &["workspace_id"]).map(WorkspaceId::from)?;
        let agent_name = string_at(agent, &["display_agent"])
            .or_else(|| string_at(agent, &["agent"]))
            .unwrap_or_else(|| "agent".to_string());
        let agent_status = string_at(agent, &["agent_status"])
            .as_deref()
            .map_or(AgentStatus::Unknown, AgentStatus::from);
        let cwd = string_at(agent, &["foreground_cwd"])
            .or_else(|| string_at(agent, &["cwd"]))
            .map(PathBuf::from)?;
        if !cwd.is_dir() {
            return None;
        }

        let branch = git_branch(&cwd).unwrap_or_else(|| "detached".to_string());
        // The current branch is authoritative. Pane history is only a fallback,
        // because old transcript URLs can refer to unrelated PRs.
        let url =
            pr_url_for_branch(&cwd, &branch).or_else(|| pr_url_from_pane(client, &pane_id))?;

        Some(PrCandidate {
            cwd,
            url,
            agent_name,
            agent_status,
            branch,
            workspace_id,
            pane_id,
        })
    }
}

fn metadata_source() -> String {
    format!("plugin:{}", plugin_id())
}

fn clear_pr_metadata(client: &HerdrClient, agents: &[serde_json::Value]) {
    let source = metadata_source();
    for pane_id in agents
        .iter()
        .filter_map(|agent| string_at(agent, &["pane_id"]))
    {
        let _ = client.call(
            Method::PaneReportMetadata,
            json!({
                "pane_id": pane_id,
                "source": source,
                "tokens": {
                    "pr_url": null,
                    "pr_number": null,
                    "pr_state": null,
                    "pr_open": null,
                    "pr_draft": null,
                    "pr_merged": null,
                    "pr_closed": null,
                },
            }),
        );
    }
}

fn report_pr_metadata(client: &HerdrClient, rows: &[PrRow]) {
    let source = metadata_source();
    for row in rows {
        let state = row.state.metadata_key();
        let badge = format!("#{} {} {state}", row.number, row.state.icon());
        let _ = client.call(
            Method::PaneReportMetadata,
            json!({
                "pane_id": row.pane_id,
                "source": source,
                "tokens": {
                    "pr_url": row.url,
                    "pr_number": format!("#{}", row.number),
                    "pr_state": state,
                    "pr_open": (row.state == PrState::Open).then(|| badge.clone()),
                    "pr_draft": (row.state == PrState::Draft).then(|| badge.clone()),
                    "pr_merged": (row.state == PrState::Merged).then(|| badge.clone()),
                    "pr_closed": (row.state == PrState::Closed).then(|| badge.clone()),
                },
            }),
        );
    }
}

fn pr_url_from_pane(client: &HerdrClient, pane_id: &PaneId) -> Option<String> {
    let text = client.read_pane_recent_text(pane_id, 800).ok()?;
    find_latest_pr_url(&text)
}

fn find_latest_pr_url(text: &str) -> Option<String> {
    let re = Regex::new(r"https://github\.com/[^/\s]+/[^/\s]+/pull/[0-9]+").ok()?;
    re.find_iter(text).last().map(|m| m.as_str().to_string())
}
