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

pub(super) fn load_pr_rows(socket_path: &Path) -> Result<Vec<PrRow>> {
    let result =
        socket_call(socket_path, "agent.list", json!({})).context("failed to list Herdr agents")?;
    let agents = result
        .get("agents")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut rows = Vec::new();
    let mut seen_urls = HashSet::new();
    for agent in agents {
        let pane_id = string_at(&agent, &["pane_id"]).unwrap_or_default();
        let workspace_id = string_at(&agent, &["workspace_id"]).unwrap_or_default();
        let agent_name = string_at(&agent, &["agent"])
            .or_else(|| string_at(&agent, &["display_agent"]))
            .unwrap_or_else(|| "agent".to_string());
        let agent_status =
            string_at(&agent, &["agent_status"]).unwrap_or_else(|| "unknown".to_string());
        let Some(cwd) = string_at(&agent, &["foreground_cwd"])
            .or_else(|| string_at(&agent, &["cwd"]))
            .map(PathBuf::from)
        else {
            continue;
        };
        if pane_id.is_empty() || workspace_id.is_empty() || !cwd.is_dir() {
            continue;
        }

        let branch = git_branch(&cwd).unwrap_or_else(|| "detached".to_string());
        let url =
            pr_url_from_pane(socket_path, &pane_id).or_else(|| pr_url_for_branch(&cwd, &branch));
        let Some(url) = url else {
            continue;
        };
        if !seen_urls.insert(url.clone()) {
            continue;
        }

        if let Some(row) = pr_row_from_gh(
            &cwd,
            &url,
            &agent_name,
            &agent_status,
            &branch,
            &workspace_id,
            &pane_id,
        ) {
            rows.push(row);
        }
    }

    rows.sort_by(|a, b| {
        pr_state_rank(a.state)
            .cmp(&pr_state_rank(b.state))
            .then_with(|| a.number.cmp(&b.number))
    });
    Ok(rows)
}

fn pr_url_from_pane(socket_path: &Path, pane_id: &str) -> Option<String> {
    let result = socket_call(
        socket_path,
        "pane.read",
        json!({
            "pane_id": pane_id,
            "source": "recent",
            "lines": 4000,
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
