use super::{QuickStartForm, quick_start_name};
use crate::herdr::socket_call;
use crate::util::{first_string, slugify};
use anyhow::{Context, Result, anyhow};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub(super) struct CreatedWorktree {
    pub(super) workspace_id: String,
    pub(super) initial_tab_id: Option<String>,
    pub(super) path: PathBuf,
}

pub(super) fn create_worktree(
    socket_path: &Path,
    cwd: &Path,
    branch: &str,
    prompt: &str,
) -> Result<CreatedWorktree> {
    let cwd_string = cwd.to_string_lossy().to_string();
    let label = quick_start_name(prompt);
    let result = socket_call(
        socket_path,
        "worktree.create",
        json!({
            "cwd": cwd_string,
            "branch": branch,
            "label": label,
            "focus": true,
        }),
    )
    .context("failed to create quick-start worktree")?;

    let workspace_id = first_string(
        &result,
        &[
            &["workspace", "workspace_id"],
            &["workspace", "id"],
            &["workspace_id"],
        ],
    )
    .ok_or_else(|| anyhow!("worktree.create response did not include a workspace id: {result}"))?;

    let initial_tab_id = first_string(
        &result,
        &[
            &["tab", "tab_id"],
            &["tab", "id"],
            &["root_pane", "tab_id"],
            &["pane", "tab_id"],
            &["tab_id"],
        ],
    );

    let path = first_string(
        &result,
        &[&["worktree", "path"], &["workspace", "cwd"], &["path"]],
    )
    .map(PathBuf::from)
    .unwrap_or_else(|| cwd.to_path_buf());

    Ok(CreatedWorktree {
        workspace_id,
        initial_tab_id,
        path,
    })
}

pub(super) fn branch_for_form(form: &QuickStartForm) -> String {
    let branch = form.branch.trim();
    if !branch.is_empty() {
        return branch.to_string();
    }

    let slug = slugify(&form.prompt, 48);
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("quick/{slug}-{seconds}")
}
