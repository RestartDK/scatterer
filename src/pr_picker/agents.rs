use super::github::{pr_row_from_gh, pr_url_for_branch};
use super::{PrRow, pr_state_rank};
use crate::git::git_branch;
use crate::herdr::socket_call;
use crate::util::string_at;
use anyhow::{Context, Result};
use regex::Regex;
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::thread;

#[derive(Debug)]
struct PrCandidate {
    cwd: PathBuf,
    url: String,
    agent_name: String,
    agent_status: String,
    branch: String,
    workspace_id: String,
    pane_id: String,
}

pub(super) fn load_pr_rows(socket_path: &Path) -> Result<Vec<PrRow>> {
    let result =
        socket_call(socket_path, "agent.list", json!({})).context("failed to list Herdr agents")?;
    let agents = result
        .get("agents")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    clear_pr_metadata(socket_path, &agents);

    // GitHub lookups can be slow when several agents are active. Resolve each
    // agent concurrently, then deduplicate before fetching full PR details.
    let mut candidates = thread::scope(|scope| {
        let handles = agents
            .into_iter()
            .map(|agent| {
                let socket_path = socket_path.to_path_buf();
                scope.spawn(move || candidate_for_agent(&socket_path, &agent))
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
            .map(|candidate| {
                scope.spawn(move || {
                    pr_row_from_gh(
                        &candidate.cwd,
                        &candidate.url,
                        &candidate.agent_name,
                        &candidate.agent_status,
                        &candidate.branch,
                        &candidate.workspace_id,
                        &candidate.pane_id,
                    )
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok().flatten())
            .collect::<Vec<_>>()
    });

    rows.sort_by(|a, b| {
        pr_state_rank(a.state)
            .cmp(&pr_state_rank(b.state))
            .then_with(|| a.number.cmp(&b.number))
    });
    report_pr_metadata(socket_path, &rows);
    Ok(rows)
}

fn candidate_for_agent(socket_path: &Path, agent: &serde_json::Value) -> Option<PrCandidate> {
    let pane_id = string_at(agent, &["pane_id"])?;
    let workspace_id = string_at(agent, &["workspace_id"])?;
    let agent_name = string_at(agent, &["display_agent"])
        .or_else(|| string_at(agent, &["agent"]))
        .unwrap_or_else(|| "agent".to_string());
    let agent_status = string_at(agent, &["agent_status"]).unwrap_or_else(|| "unknown".to_string());
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
        pr_url_for_branch(&cwd, &branch).or_else(|| pr_url_from_pane(socket_path, &pane_id))?;

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

fn metadata_source() -> String {
    let plugin_id = crate::util::non_empty_env("HERDR_PLUGIN_ID")
        .unwrap_or_else(|| "daniel.scatterer".to_string());
    format!("plugin:{plugin_id}")
}

fn clear_pr_metadata(socket_path: &Path, agents: &[serde_json::Value]) {
    let source = metadata_source();
    for pane_id in agents
        .iter()
        .filter_map(|agent| string_at(agent, &["pane_id"]))
    {
        let _ = socket_call(
            socket_path,
            "pane.report_metadata",
            json!({
                "pane_id": pane_id,
                "source": source,
                "tokens": {
                    "pr_url": null,
                    "pr_number": null,
                    "pr_state": null,
                },
            }),
        );
    }
}

fn report_pr_metadata(socket_path: &Path, rows: &[PrRow]) {
    let source = metadata_source();
    for row in rows {
        let state = match row.state {
            super::PrState::Open => "open",
            super::PrState::Draft => "draft",
            super::PrState::Merged => "merged",
            super::PrState::Closed => "closed",
        };
        let _ = socket_call(
            socket_path,
            "pane.report_metadata",
            json!({
                "pane_id": row.pane_id,
                "source": source,
                "tokens": {
                    "pr_url": row.url,
                    "pr_number": format!("#{}", row.number),
                    "pr_state": state,
                },
            }),
        );
    }
}

fn pr_url_from_pane(socket_path: &Path, pane_id: &str) -> Option<String> {
    let result = socket_call(
        socket_path,
        "pane.read",
        json!({
            "pane_id": pane_id,
            "source": "recent-unwrapped",
            "lines": 800,
        }),
    )
    .ok()?;
    let text = string_at(&result, &["read", "text"])?;
    find_latest_pr_url(&text)
}

fn find_latest_pr_url(text: &str) -> Option<String> {
    let re = Regex::new(r"https://github\.com/[^/\s]+/[^/\s]+/pull/[0-9]+").ok()?;
    re.find_iter(text).last().map(|m| m.as_str().to_string())
}
