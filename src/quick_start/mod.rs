mod pi;
mod tui;
mod worktree;

use crate::config::load_project_config;
use crate::focus::focus_workspace_later;
use crate::git::{git_branch, switch_or_create_branch};
use crate::herdr::{herdr_socket_path, resolve_invocation_source, socket_call};
use crate::layout::{apply_scatterer_layout, create_workspace};
use crate::util::{non_empty_env, slugify};
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
}

impl QuickStartTarget {
    pub(crate) fn label(self) -> &'static str {
        match self {
            QuickStartTarget::Workspace => "workspace",
            QuickStartTarget::Worktree => "worktree",
        }
    }
}

#[derive(Debug)]
pub(crate) struct QuickStartForm {
    pub(crate) prompt: String,
    pub(crate) branch: String,
    pub(crate) target: QuickStartTarget,
    pub(crate) harness: Harness,
    pub(crate) model: Option<String>,
}

pub(crate) fn open() -> Result<()> {
    let socket_path = herdr_socket_path()?;
    let plugin_id =
        non_empty_env("HERDR_PLUGIN_ID").unwrap_or_else(|| "daniel.scatterer".to_string());
    let source = resolve_invocation_source(&socket_path)?;
    socket_call(
        &socket_path,
        "plugin.pane.open",
        json!({
            "plugin_id": plugin_id,
            "entrypoint": "quick-start",
            "placement": "overlay",
            "focus": true,
            "env": {
                "SCATTERER_SOURCE_CWD": source.cwd.to_string_lossy(),
            },
        }),
    )
    .context("failed to open Scatterer quick-start overlay")?;
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    let Some(form) = tui::run_quick_start_tui()? else {
        println!("scatterer: quick start cancelled");
        return Ok(());
    };

    let socket_path = herdr_socket_path()?;
    let source = resolve_invocation_source(&socket_path)?;

    match form.target {
        QuickStartTarget::Workspace => run_workspace_quick_start(&socket_path, &source.cwd, form),
        QuickStartTarget::Worktree => run_worktree_quick_start(&socket_path, &source.cwd, form),
    }
}

fn run_workspace_quick_start(
    socket_path: &std::path::Path,
    source_cwd: &std::path::Path,
    form: QuickStartForm,
) -> Result<()> {
    let requested_branch = worktree::requested_workspace_branch(&form);
    if let Some(branch) = &requested_branch {
        switch_or_create_branch(source_cwd, branch)?;
    }

    let (config, config_path) = load_project_config(source_cwd)?;
    let created = create_workspace(socket_path, source_cwd)?;
    let session_name = requested_branch
        .clone()
        .or_else(|| git_branch(source_cwd))
        .unwrap_or_else(|| quick_start_name(&form.prompt));
    let pi_command = pi::pi_agent_command(&form, &session_name);

    apply_scatterer_layout(
        socket_path,
        &created.workspace_id,
        Some(&created.initial_tab_id),
        source_cwd,
        &config,
        pi_command.as_deref(),
    )?;

    println!(
        "scatterer: quick-started {} {}",
        form.harness.label(),
        form.target.label()
    );
    if let Some(branch) = requested_branch.or_else(|| git_branch(source_cwd)) {
        println!("scatterer: branch {branch}");
    }
    println!("scatterer: path {}", source_cwd.display());
    print_config_paths(config_path);
    focus_workspace_later(&created.workspace_id);

    Ok(())
}

fn run_worktree_quick_start(
    socket_path: &std::path::Path,
    source_cwd: &std::path::Path,
    form: QuickStartForm,
) -> Result<()> {
    let (config, config_path) = load_project_config(source_cwd)?;
    let branch = worktree::branch_for_form(&form);
    let created = worktree::create_worktree(socket_path, source_cwd, &branch, &form.prompt)?;
    run_worktree_setup(source_cwd, &created.path, &config)?;
    let pi_command = pi::pi_agent_command(&form, &branch);

    apply_scatterer_layout(
        socket_path,
        &created.workspace_id,
        created.initial_tab_id.as_deref(),
        &created.path,
        &config,
        pi_command.as_deref(),
    )?;

    println!(
        "scatterer: quick-started {} {}",
        form.harness.label(),
        form.target.label()
    );
    println!("scatterer: branch {branch}");
    println!("scatterer: path {}", created.path.display());
    print_config_paths(config_path);
    focus_workspace_later(&created.workspace_id);

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
