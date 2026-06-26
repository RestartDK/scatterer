use super::{CheckState, PrRow, PrState};
use crate::util::string_at;
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) fn open_pr_in_browser(url: &str) {
    let _ = Command::new("gh")
        .args(["pr", "view", url, "--web"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

pub(super) fn pr_url_for_branch(cwd: &Path, branch: &str) -> Option<String> {
    if branch.is_empty() || branch == "detached" {
        return None;
    }
    let output = Command::new("gh")
        .current_dir(cwd)
        .args([
            "pr",
            "list",
            "--head",
            branch,
            "--state",
            "all",
            "--json",
            "url",
            "--jq",
            ".[0].url // empty",
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

pub(super) fn pr_row_from_gh(
    cwd: &Path,
    url: &str,
    agent: &str,
    agent_status: &str,
    branch: &str,
    workspace_id: &str,
    pane_id: &str,
) -> Option<PrRow> {
    let output = Command::new("gh")
        .current_dir(cwd)
        .args([
            "pr",
            "view",
            url,
            "--json",
            "number,title,state,isDraft,mergedAt,url,reviewDecision,statusCheckRollup,comments,headRefName,baseRefName",
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: Value = serde_json::from_slice(&output.stdout).ok()?;
    let number = value
        .get("number")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let title = string_at(&value, &["title"]).unwrap_or_else(|| "Untitled PR".to_string());
    let state = parse_pr_state(&value);
    let checks = parse_check_state(value.get("statusCheckRollup"));
    let comments = value
        .get("comments")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    let review = string_at(&value, &["reviewDecision"]).filter(|value| value != "");
    let head = string_at(&value, &["headRefName"]).unwrap_or_else(|| branch.to_string());

    Some(PrRow {
        url: string_at(&value, &["url"]).unwrap_or_else(|| url.to_string()),
        number,
        title,
        state,
        checks,
        comments,
        review,
        agent: agent.to_string(),
        agent_status: agent_status.to_string(),
        branch: head,
        workspace_id: workspace_id.to_string(),
        pane_id: pane_id.to_string(),
    })
}

fn parse_pr_state(value: &Value) -> PrState {
    let state = string_at(value, &["state"]).unwrap_or_default();
    let is_draft = value
        .get("isDraft")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let merged_at = string_at(value, &["mergedAt"]);
    if state == "MERGED" || merged_at.is_some() {
        PrState::Merged
    } else if is_draft {
        PrState::Draft
    } else if state == "CLOSED" {
        PrState::Closed
    } else {
        PrState::Open
    }
}

fn parse_check_state(value: Option<&Value>) -> CheckState {
    let Some(checks) = value.and_then(Value::as_array) else {
        return CheckState::None;
    };
    if checks.is_empty() {
        return CheckState::None;
    }

    let mut pending = false;
    for check in checks {
        let conclusion = string_at(check, &["conclusion"]).unwrap_or_default();
        let state = string_at(check, &["state"]).unwrap_or_default();
        if matches!(
            conclusion.as_str(),
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED"
        ) || matches!(
            state.as_str(),
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED"
        ) {
            return CheckState::Fail;
        }
        if conclusion.is_empty()
            || matches!(
                state.as_str(),
                "PENDING" | "IN_PROGRESS" | "QUEUED" | "REQUESTED" | "WAITING" | "EXPECTED"
            )
        {
            pending = true;
        }
    }
    if pending {
        CheckState::Pending
    } else {
        CheckState::Pass
    }
}
