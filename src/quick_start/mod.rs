mod pi;
mod tui;
mod worktree;

use crate::config::load_project_config;
use crate::focus::focus_workspace_later;
use crate::herdr::{herdr_socket_path, resolve_invocation_source, socket_call};
use crate::layout::apply_scatterer_layout;
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

#[derive(Debug)]
pub(crate) struct QuickStartForm {
    pub(crate) prompt: String,
    pub(crate) branch: String,
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
    let (config, config_path) = load_project_config(&source.cwd)?;
    let branch = worktree::branch_for_form(&form);
    let created = worktree::create_worktree(&socket_path, &source.cwd, &branch, &form.prompt)?;
    run_worktree_setup(&source.cwd, &created.path, &config)?;
    let pi_command = pi::pi_prompt_command(&form, &branch);

    apply_scatterer_layout(
        &socket_path,
        &created.workspace_id,
        created.initial_tab_id.as_deref(),
        &created.path,
        &config,
        Some(&pi_command),
    )?;

    println!("scatterer: quick-started {} worktree", form.harness.label());
    println!("scatterer: branch {branch}");
    println!("scatterer: path {}", created.path.display());
    for path in config_path {
        println!("scatterer: project config {}", path.display());
    }
    focus_workspace_later(&created.workspace_id);

    Ok(())
}

pub(crate) fn quick_start_name(prompt: &str) -> String {
    let slug = slugify(prompt, 42);
    format!("quick {slug}")
}
