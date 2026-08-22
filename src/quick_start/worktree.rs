use super::{QuickStartForm, quick_start_name};
use crate::herdr::{CreatedWorktree, HerdrClient};
use crate::ids::WorkspaceId;
use crate::util::{non_empty_env, slugify};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FlatWorktreeRecord {
    workspace_id: WorkspaceId,
    source_cwd: PathBuf,
    path: PathBuf,
    branch: String,
    label: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FlatWorktreeRegistry {
    worktrees: Vec<FlatWorktreeRecord>,
}

pub(super) fn create_worktree(
    client: &HerdrClient,
    cwd: &Path,
    branch: &str,
    base: Option<&str>,
    prompt: &str,
    grouped: bool,
) -> Result<CreatedWorktree> {
    let label = workspace_label(branch, prompt);
    let created = client
        .create_worktree(cwd, branch, base, &label, grouped)
        .context("failed to create quick-start worktree")?;

    if grouped {
        return Ok(created);
    }
    let path = created.path;

    // Herdr has no "create a worktree but show it as a top-level workspace"
    // method. Let Herdr choose and create the checkout under its configured
    // worktree directory, close only the grouped workspace state, then reopen
    // the same checkout as an ordinary workspace. workspace.close does not run
    // `git worktree remove`, so the checkout remains intact.
    client
        .close_workspace(&created.workspace_id)
        .with_context(|| {
            format!(
                "failed to close temporary grouped workspace; worktree remains at {}",
                path.display()
            )
        })?;

    let workspace = match client.create_workspace(&path, &label) {
        Ok(workspace) => workspace,
        Err(error) => {
            let rollback = git_worktree_remove(cwd, &path);
            return match rollback {
                Ok(()) => Err(error).context(
                    "failed to reopen worktree as a top-level workspace; removed the unused checkout",
                ),
                Err(rollback_error) => Err(error).with_context(|| {
                    format!(
                        "failed to reopen worktree as a top-level workspace; rollback also failed ({rollback_error:#}); checkout remains at {}",
                        path.display()
                    )
                }),
            };
        }
    };

    remember_flat_worktree(FlatWorktreeRecord {
        workspace_id: workspace.workspace_id.clone(),
        source_cwd: cwd.to_path_buf(),
        path: path.clone(),
        branch: branch.to_string(),
        label,
    })
    .with_context(|| {
        format!(
            "top-level worktree was created at {}, but Scatterer could not record it for safe removal",
            path.display()
        )
    })?;

    Ok(CreatedWorktree {
        workspace_id: workspace.workspace_id,
        initial_tab_id: Some(workspace.initial_tab_id),
        path,
    })
}

pub(super) fn remove_current_flat_worktree() -> Result<()> {
    let client = HerdrClient::from_env()?;
    let source = client.invocation_source()?;
    let mut registry = load_flat_worktree_registry()?;
    let Some(index) = registry
        .worktrees
        .iter()
        .position(|record| source.cwd == record.path || source.cwd.starts_with(&record.path))
    else {
        bail!(
            "{} is not inside a top-level worktree recorded by Scatterer",
            source.cwd.display()
        );
    };
    let record = registry.worktrees[index].clone();

    ensure_worktree_clean(&record.path)?;
    client.close_workspace(&record.workspace_id)?;

    if let Err(remove_error) = git_worktree_remove(&record.source_cwd, &record.path) {
        let restored = client
            .create_workspace(&record.path, &record.label)
            .context("failed to restore the workspace after Git refused worktree removal")?;
        registry.worktrees[index].workspace_id = restored.workspace_id;
        save_flat_worktree_registry(&registry)?;
        return Err(remove_error)
            .context("Git refused worktree removal; Scatterer restored the top-level workspace");
    }

    registry.worktrees.remove(index);
    save_flat_worktree_registry(&registry)?;
    println!(
        "scatterer: removed top-level worktree {}",
        record.path.display()
    );
    println!("scatterer: branch {} was not deleted", record.branch);
    Ok(())
}

fn ensure_worktree_clean(path: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(path)
        .args(["status", "--porcelain"])
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to inspect worktree {}", path.display()))?;
    if !output.status.success() {
        bail!("git status failed for {}", path.display());
    }
    if !output.stdout.is_empty() {
        bail!(
            "refusing to remove dirty top-level worktree {}; commit, stash, or clean it first",
            path.display()
        );
    }
    Ok(())
}

fn git_worktree_remove(source_cwd: &Path, path: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(source_cwd)
        .args(["worktree", "remove"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to run git worktree remove {}", path.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    bail!(
        "git worktree remove {} failed with {}{}",
        path.display(),
        output.status,
        if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        }
    )
}

fn flat_worktree_registry_path() -> Result<PathBuf> {
    let state_dir = non_empty_env("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HERDR_PLUGIN_STATE_DIR is unavailable"))?;
    Ok(state_dir.join("flat-worktrees.json"))
}

fn load_flat_worktree_registry() -> Result<FlatWorktreeRegistry> {
    let path = flat_worktree_registry_path()?;
    if !path.exists() {
        return Ok(FlatWorktreeRegistry::default());
    }
    let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))
}

fn remember_flat_worktree(record: FlatWorktreeRecord) -> Result<()> {
    let mut registry = load_flat_worktree_registry()?;
    registry.worktrees.retain(|existing| {
        existing.path != record.path && existing.workspace_id != record.workspace_id
    });
    registry.worktrees.push(record);
    save_flat_worktree_registry(&registry)
}

fn save_flat_worktree_registry(registry: &FlatWorktreeRegistry) -> Result<()> {
    let path = flat_worktree_registry_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("flat-worktree registry path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(registry)?;
    fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, &path).with_context(|| format!("failed to replace {}", path.display()))
}

pub(super) fn branch_for_form(form: &QuickStartForm) -> String {
    let branch = form.branch.trim();
    if !branch.is_empty() {
        return branch.to_string();
    }

    default_branch_for_prompt(&form.prompt)
}

pub(super) fn requested_workspace_branch(form: &QuickStartForm) -> Option<String> {
    let branch = form.branch.trim();
    (!branch.is_empty()).then_some(branch.to_string())
}

pub(super) fn base_for_form(form: &QuickStartForm) -> Option<String> {
    let base = form.base.trim();
    (!base.is_empty()).then_some(base.to_string())
}

pub(super) fn default_branch_for_prompt(prompt: &str) -> String {
    let slug = slugify(prompt, 48);
    format!("daniel/{slug}")
}

fn workspace_label(branch: &str, prompt: &str) -> String {
    let branch = branch.trim();
    if branch.is_empty() {
        quick_start_name(prompt)
    } else {
        branch.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quick_start::{Harness, QuickStartTarget};

    #[test]
    fn default_branch_uses_daniel_prompt_slug() {
        assert_eq!(
            default_branch_for_prompt("Start the Linear issue\nwith tests"),
            "daniel/start-the-linear-issue-with-tests"
        );
    }

    #[test]
    fn explicit_branch_wins() {
        let form = QuickStartForm {
            prompt: "anything".to_string(),
            branch: "feature/custom".to_string(),
            base: String::new(),
            target: QuickStartTarget::Worktree,
            harness: Harness::Pi,
            model: None,
        };
        assert_eq!(branch_for_form(&form), "feature/custom");
    }

    #[test]
    fn base_for_form_trims_input() {
        let form = QuickStartForm {
            prompt: "anything".to_string(),
            branch: "feature/custom".to_string(),
            base: "  parent/base  ".to_string(),
            target: QuickStartTarget::Worktree,
            harness: Harness::Pi,
            model: None,
        };
        assert_eq!(base_for_form(&form).as_deref(), Some("parent/base"));
    }
}
