use crate::git::git_branch;
use crate::herdr::{HerdrClient, Method, plugin_id};
use crate::ids::WorkspaceId;
use crate::util::string_at;
use anyhow::{Context, Result};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrState {
    Open,
    Draft,
    Merged,
    Closed,
}

impl PrState {
    fn icon(self) -> &'static str {
        match self {
            Self::Open => "\u{f407}",
            Self::Draft => "\u{f4dd}",
            Self::Merged => "\u{f419}",
            Self::Closed => "\u{f4dc}",
        }
    }

    fn metadata_key(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Draft => "draft",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }

    fn from_gh(value: &Value) -> Self {
        let state = string_at(value, &["state"]).unwrap_or_default();
        if state == "MERGED" || string_at(value, &["mergedAt"]).is_some() {
            Self::Merged
        } else if state == "CLOSED" {
            Self::Closed
        } else if value.get("isDraft").and_then(Value::as_bool) == Some(true) {
            Self::Draft
        } else {
            Self::Open
        }
    }
}

struct DiffStats {
    additions: u64,
    deletions: u64,
}

struct PrBadge {
    number: u64,
    state: PrState,
    diff: Option<DiffStats>,
}

impl PrBadge {
    fn from_gh(value: &Value) -> Option<Self> {
        let number = value.get("number")?.as_u64().filter(|number| *number > 0)?;
        let state = PrState::from_gh(value);
        let diff = match state {
            PrState::Open | PrState::Draft => value
                .get("additions")
                .and_then(Value::as_u64)
                .zip(value.get("deletions").and_then(Value::as_u64))
                .map(|(additions, deletions)| DiffStats {
                    additions,
                    deletions,
                }),
            PrState::Merged | PrState::Closed => None,
        };
        Some(Self {
            number,
            state,
            diff,
        })
    }

    fn for_checkout(cwd: &Path) -> Option<Self> {
        let branch = git_branch(cwd)?;
        let listed = Command::new("gh")
            .current_dir(cwd)
            .args([
                "pr",
                "list",
                "--head",
                &branch,
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
        if !listed.status.success() {
            return None;
        }
        let url = std::str::from_utf8(&listed.stdout).ok()?.trim();
        if url.is_empty() {
            return None;
        }
        let output = Command::new("gh")
            .current_dir(cwd)
            .args([
                "pr",
                "view",
                url,
                "--json",
                "number,state,isDraft,mergedAt,additions,deletions",
            ])
            .stdin(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let value: Value = serde_json::from_slice(&output.stdout).ok()?;
        Self::from_gh(&value)
    }

    fn text(&self) -> String {
        format!(
            "#{} {} {}",
            self.number,
            self.state.icon(),
            self.state.metadata_key()
        )
    }
}

fn repo_name(cwd: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Path::new(std::str::from_utf8(&output.stdout).ok()?.trim())
        .parent()?
        .file_name()?
        .to_str()
        .map(str::to_owned)
}

fn tokens_for(label: &str, repo: Option<&str>, pr: Option<&PrBadge>) -> Map<String, Value> {
    let mut tokens = Map::new();
    tokens.insert(
        "repo".to_string(),
        json!(repo.filter(|repo| *repo != label)),
    );
    for state in [
        PrState::Open,
        PrState::Draft,
        PrState::Merged,
        PrState::Closed,
    ] {
        tokens.insert(
            format!("pr_{}", state.metadata_key()),
            json!(pr.filter(|pr| pr.state == state).map(PrBadge::text)),
        );
    }
    let diff = pr.and_then(|pr| pr.diff.as_ref());
    tokens.insert(
        "pr_additions".to_string(),
        json!(diff.map(|diff| format!("+{}", diff.additions))),
    );
    tokens.insert(
        "pr_deletions".to_string(),
        json!(diff.map(|diff| format!("-{}", diff.deletions))),
    );
    tokens
}

pub(crate) fn refresh_spaces() -> Result<()> {
    refresh(None)
}

pub(crate) fn refresh_space() -> Result<()> {
    let workspace_id: WorkspaceId = std::env::var("HERDR_WORKSPACE_ID")
        .context("workspace event did not include HERDR_WORKSPACE_ID")?
        .into();
    refresh(Some(&workspace_id))
}

fn refresh(target: Option<&WorkspaceId>) -> Result<()> {
    let client = HerdrClient::from_env()?;
    let workspaces = client.list_workspaces()?;
    let panes = client.list_panes(None)?;
    let source = format!("plugin:{}", plugin_id());

    for workspace in &workspaces {
        let Some(id) = string_at(workspace, &["workspace_id"]).map(WorkspaceId::from) else {
            continue;
        };
        if target.is_some_and(|target| target != &id) {
            continue;
        }
        let label = string_at(workspace, &["label"]).unwrap_or_default();
        let cwd = string_at(workspace, &["worktree", "checkout_path"])
            .map(PathBuf::from)
            .or_else(|| {
                panes
                    .iter()
                    .find(|pane| {
                        pane.get("workspace_id").and_then(Value::as_str) == Some(id.as_str())
                            && pane.get("focused").and_then(Value::as_bool) == Some(true)
                    })
                    .or_else(|| {
                        panes.iter().find(|pane| {
                            pane.get("workspace_id").and_then(Value::as_str) == Some(id.as_str())
                        })
                    })
                    .and_then(|pane| string_at(pane, &["cwd"]))
                    .map(PathBuf::from)
            });
        let repo = string_at(workspace, &["worktree", "repo_name"])
            .or_else(|| cwd.as_deref().and_then(repo_name));
        let pr = cwd.as_deref().and_then(PrBadge::for_checkout);
        client
            .call(
                Method::WorkspaceReportMetadata,
                json!({
                    "workspace_id": id.as_str(),
                    "source": source.as_str(),
                    "tokens": tokens_for(&label, repo.as_deref(), pr.as_ref()),
                }),
            )
            .with_context(|| format!("failed to report workspace metadata for {id}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_tokens_replace_old_pr_state_and_omit_redundant_repo_name() {
        let pr = PrBadge::from_gh(&json!({
            "number": 184,
            "state": "MERGED",
            "mergedAt": "2026-09-28T12:13:38Z",
            "additions": 1043,
            "deletions": 102,
        }))
        .unwrap();
        let tokens = tokens_for("pi coordination", Some("dotfiles"), Some(&pr));
        assert_eq!(tokens["repo"], "dotfiles");
        assert_eq!(tokens["pr_merged"], "#184 \u{f419} merged");
        for key in [
            "pr_open",
            "pr_draft",
            "pr_closed",
            "pr_additions",
            "pr_deletions",
        ] {
            assert!(tokens[key].is_null(), "{key} should be cleared");
        }
        let cleared = tokens_for("dotfiles", Some("dotfiles"), None);
        assert!(cleared.values().all(Value::is_null));
    }

    #[test]
    fn loc_tokens_only_appear_for_open_and_draft_prs() {
        for (state, is_draft, badge_key) in [("OPEN", false, "pr_open"), ("OPEN", true, "pr_draft")]
        {
            let pr = PrBadge::from_gh(&json!({
                "number": 137,
                "state": state,
                "isDraft": is_draft,
                "additions": 798,
                "deletions": 74,
            }))
            .unwrap();
            let tokens = tokens_for("dotfiles", None, Some(&pr));
            assert!(tokens[badge_key].as_str().unwrap().starts_with("#137"));
            assert_eq!(tokens["pr_additions"], "+798");
            assert_eq!(tokens["pr_deletions"], "-74");
        }

        let closed_draft = PrBadge::from_gh(&json!({
            "number": 137,
            "state": "CLOSED",
            "isDraft": true,
            "additions": 798,
            "deletions": 74,
        }))
        .unwrap();
        let tokens = tokens_for("dotfiles", None, Some(&closed_draft));
        assert_eq!(tokens["pr_closed"], "#137 \u{f4dc} closed");
        assert!(tokens["pr_additions"].is_null());
        assert!(tokens["pr_deletions"].is_null());
    }

    #[test]
    fn missing_loc_does_not_hide_open_pr() {
        let pr = PrBadge::from_gh(&json!({ "number": 137, "state": "OPEN" })).unwrap();
        let tokens = tokens_for("dotfiles", None, Some(&pr));
        assert_eq!(tokens["pr_open"], "#137 \u{f407} open");
        assert!(tokens["pr_additions"].is_null());
        assert!(tokens["pr_deletions"].is_null());
    }
}
