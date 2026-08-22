mod pi;
mod tui;
mod worktree;

use crate::config::load_project_config;
use crate::git::{git_branch, remember_parent_branch, switch_or_create_branch};
use crate::herdr::{Entrypoint, HerdrClient, Placement};
use crate::layout::{apply_scatterer_layout, create_workspace};
use crate::util::slugify;
use crate::worktree_setup::run_worktree_setup;
use anyhow::{Context, Result};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Harness {
    Pi,
}

impl Harness {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Harness::Pi => "pi",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickStartTarget {
    Workspace,
    Worktree,
    FlatWorktree,
}

impl QuickStartTarget {
    pub(crate) fn label(self) -> &'static str {
        match self {
            QuickStartTarget::Workspace => "workspace",
            QuickStartTarget::Worktree => "grouped worktree",
            QuickStartTarget::FlatWorktree => "top-level worktree",
        }
    }

    pub(crate) fn creates_worktree(self) -> bool {
        matches!(self, Self::Worktree | Self::FlatWorktree)
    }

    pub(crate) fn groups_worktree(self) -> bool {
        self == Self::Worktree
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Self::Workspace => Self::Worktree,
            Self::Worktree => Self::FlatWorktree,
            Self::FlatWorktree => Self::Workspace,
        }
    }

    pub(crate) fn previous(self) -> Self {
        match self {
            Self::Workspace => Self::FlatWorktree,
            Self::Worktree => Self::Workspace,
            Self::FlatWorktree => Self::Worktree,
        }
    }
}

#[derive(Debug)]
pub(crate) struct QuickStartForm {
    pub(crate) prompt: String,
    pub(crate) branch: String,
    pub(crate) base: String,
    pub(crate) target: QuickStartTarget,
    pub(crate) harness: Harness,
    pub(crate) model: Option<String>,
}

pub(crate) fn open() -> Result<()> {
    let client = HerdrClient::from_env()?;
    let source = client.invocation_source()?;
    client
        .open_plugin_pane(
            Entrypoint::QuickStart,
            Placement::Popup,
            json!({
                "env": {
                    "SCATTERER_SOURCE_CWD": source.cwd.to_string_lossy(),
                },
            }),
        )
        .context("failed to open Scatterer quick-start popup")?;
    Ok(())
}

pub(crate) fn remove_flat_worktree() -> Result<()> {
    worktree::remove_current_flat_worktree()
}

pub(crate) fn run() -> Result<()> {
    let Some(form) = tui::run_quick_start_tui()? else {
        println!("scatterer: quick start cancelled");
        return Ok(());
    };

    let client = HerdrClient::from_env()?;
    let source = client.invocation_source()?;

    match form.target {
        QuickStartTarget::Workspace => run_workspace_quick_start(&client, &source.cwd, form),
        QuickStartTarget::Worktree | QuickStartTarget::FlatWorktree => {
            run_worktree_quick_start(&client, &source.cwd, form)
        }
    }
}

fn run_workspace_quick_start(
    client: &HerdrClient,
    source_cwd: &std::path::Path,
    form: QuickStartForm,
) -> Result<()> {
    let requested_branch = worktree::requested_workspace_branch(&form);
    let requested_base = worktree::base_for_form(&form);
    if let Some(branch) = &requested_branch {
        switch_or_create_branch(source_cwd, branch, requested_base.as_deref())?;
        if let Some(base) = requested_base.as_deref() {
            remember_parent_branch(source_cwd, branch, base)?;
        }
    }

    let (config, config_path) = load_project_config(source_cwd)?;
    let created = create_workspace(client, source_cwd)?;
    let session_name = requested_branch
        .clone()
        .or_else(|| git_branch(source_cwd))
        .unwrap_or_else(|| quick_start_name(&form.prompt));
    let layout = apply_scatterer_layout(
        client,
        &created.workspace_id,
        Some(&created.initial_tab_id),
        source_cwd,
        &config,
        requested_base.as_deref(),
        true,
    )?;
    pi::start_pi_agent(
        client,
        &layout.agent_pane_id,
        &created.workspace_id,
        &form,
        &session_name,
    )?;

    println!(
        "scatterer: quick-started {} {}",
        form.harness.label(),
        form.target.label()
    );
    if let Some(branch) = requested_branch.or_else(|| git_branch(source_cwd)) {
        println!("scatterer: branch {branch}");
    }
    if let Some(base) = requested_base {
        println!("scatterer: base {base}");
    }
    println!("scatterer: path {}", source_cwd.display());
    print_config_paths(config_path);

    Ok(())
}

fn run_worktree_quick_start(
    client: &HerdrClient,
    source_cwd: &std::path::Path,
    form: QuickStartForm,
) -> Result<()> {
    let (config, config_path) = load_project_config(source_cwd)?;
    let branch = worktree::branch_for_form(&form);
    let base = worktree::base_for_form(&form);
    let created = worktree::create_worktree(
        client,
        source_cwd,
        &branch,
        base.as_deref(),
        &form.prompt,
        form.target.groups_worktree(),
    )?;
    if let Some(parent) = base.as_deref() {
        remember_parent_branch(&created.path, &branch, parent)?;
    }
    run_worktree_setup(source_cwd, &created.path, &config)?;
    let layout = apply_scatterer_layout(
        client,
        &created.workspace_id,
        created.initial_tab_id.as_ref(),
        &created.path,
        &config,
        base.as_deref(),
        true,
    )?;
    pi::start_pi_agent(
        client,
        &layout.agent_pane_id,
        &created.workspace_id,
        &form,
        &branch,
    )?;

    println!(
        "scatterer: quick-started {} {}",
        form.harness.label(),
        form.target.label()
    );
    println!("scatterer: branch {branch}");
    if let Some(base) = base {
        println!("scatterer: base {base}");
    }
    println!("scatterer: path {}", created.path.display());
    print_config_paths(config_path);

    Ok(())
}

fn print_config_paths(config_path: Vec<std::path::PathBuf>) {
    for path in config_path {
        println!("scatterer: project config {}", path.display());
    }
}

pub(crate) fn quick_start_name(prompt: &str) -> String {
    let slug = slugify(prompt, 42);
    format!("quick {slug}")
}

#[cfg(test)]
mod tests {
    use super::QuickStartTarget;

    #[test]
    fn quick_start_targets_cycle_in_both_directions() {
        assert_eq!(
            QuickStartTarget::Workspace.next(),
            QuickStartTarget::Worktree
        );
        assert_eq!(
            QuickStartTarget::Worktree.next(),
            QuickStartTarget::FlatWorktree
        );
        assert_eq!(
            QuickStartTarget::FlatWorktree.next(),
            QuickStartTarget::Workspace
        );
        assert_eq!(
            QuickStartTarget::Workspace.previous(),
            QuickStartTarget::FlatWorktree
        );
        assert_eq!(
            QuickStartTarget::FlatWorktree.previous(),
            QuickStartTarget::Worktree
        );
    }
}
