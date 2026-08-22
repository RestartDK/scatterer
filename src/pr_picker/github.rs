use super::agents::PrCandidate;
use super::{CheckState, PrRow, PrState, ReviewDecision};
use crate::util::{command_exists, copy_to_terminal_clipboard, is_ssh_session, string_at};
use anyhow::{Context, Result};
use serde_json::Value;
use std::env;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OpenPrOutcome {
    Opened,
    CopiedToTerminalClipboard,
}

pub(super) fn open_pr_in_browser(url: &str) -> Result<OpenPrOutcome> {
    if is_ssh_session() {
        copy_to_terminal_clipboard(url)
            .context("failed to copy URL to local terminal clipboard")?;
        return Ok(OpenPrOutcome::CopiedToTerminalClipboard);
    }

    if cfg!(target_os = "macos") {
        spawn_open_command("open", &[url]).context("failed to open URL with open")?;
        return Ok(OpenPrOutcome::Opened);
    }

    if graphical_session_available() {
        if command_exists("xdg-open") {
            spawn_open_command("xdg-open", &[url]).context("failed to open URL with xdg-open")?;
            return Ok(OpenPrOutcome::Opened);
        }

        if command_exists("gio") {
            spawn_open_command("gio", &["open", url]).context("failed to open URL with gio")?;
            return Ok(OpenPrOutcome::Opened);
        }

        if command_exists("gh") {
            spawn_open_command("gh", &["pr", "view", url, "--web"])
                .context("failed to open URL with gh")?;
            return Ok(OpenPrOutcome::Opened);
        }
    }

    copy_to_terminal_clipboard(url).context("failed to copy URL to terminal clipboard")?;
    Ok(OpenPrOutcome::CopiedToTerminalClipboard)
}

fn spawn_open_command(program: &str, args: &[&str]) -> Result<()> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start {program}"))?;
    Ok(())
}

fn graphical_session_available() -> bool {
    env::var_os("DISPLAY").is_some() || env::var_os("WAYLAND_DISPLAY").is_some()
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

impl PrCandidate {
    /// Fetch full PR details from `gh` and build the display row.
    pub(super) fn fetch_pr_row(&self) -> Option<PrRow> {
        let output = Command::new("gh")
            .current_dir(&self.cwd)
            .args([
                "pr",
                "view",
                self.url.as_str(),
                "--json",
                "number,title,state,isDraft,mergedAt,url,reviewDecision,statusCheckRollup,comments,headRefName,baseRefName,additions,deletions,changedFiles",
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
        let state = PrState::from_gh(&value);
        let checks = CheckState::from_gh(value.get("statusCheckRollup"));
        let comments = value
            .get("comments")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default();
        let additions = value
            .get("additions")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let deletions = value
            .get("deletions")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let changed_files = value
            .get("changedFiles")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let review = string_at(&value, &["reviewDecision"])
            .and_then(|value| ReviewDecision::from_gh(&value));
        let head = string_at(&value, &["headRefName"]).unwrap_or_else(|| self.branch.clone());

        Some(PrRow {
            url: string_at(&value, &["url"]).unwrap_or_else(|| self.url.clone()),
            number,
            title,
            state,
            checks,
            comments,
            additions,
            deletions,
            changed_files,
            review,
            agent: self.agent_name.clone(),
            agent_status: self.agent_status,
            branch: head,
            workspace_id: self.workspace_id.clone(),
            pane_id: self.pane_id.clone(),
        })
    }
}

impl PrState {
    /// Classify a `gh pr view` payload.
    fn from_gh(value: &Value) -> Self {
        let state = string_at(value, &["state"]).unwrap_or_default();
        let is_draft = value
            .get("isDraft")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let merged_at = string_at(value, &["mergedAt"]);
        if state == "MERGED" || merged_at.is_some() {
            Self::Merged
        } else if is_draft {
            Self::Draft
        } else if state == "CLOSED" {
            Self::Closed
        } else {
            Self::Open
        }
    }
}

impl CheckState {
    /// Aggregate a `statusCheckRollup` array: any failure wins, then pending.
    fn from_gh(value: Option<&Value>) -> Self {
        let Some(checks) = value.and_then(Value::as_array) else {
            return Self::None;
        };
        if checks.is_empty() {
            return Self::None;
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
                return Self::Fail;
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
        if pending { Self::Pending } else { Self::Pass }
    }
}
