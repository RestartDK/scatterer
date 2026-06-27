use crate::config::{ProjectConfig, load_project_config};
use crate::git::git_branch;
use crate::herdr::{herdr_socket_path, resolve_invocation_source, socket_call};
use crate::pane_env;
use crate::util::first_string;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Debug)]
pub(crate) struct CreatedWorkspace {
    pub(crate) workspace_id: String,
    pub(crate) initial_tab_id: String,
}

pub(crate) fn apply_layout() -> Result<()> {
    let socket_path = herdr_socket_path()?;
    let source = resolve_invocation_source(&socket_path)?;
    let (config, config_path) = load_project_config(&source.cwd)?;
    let created = create_workspace(&socket_path, &source.cwd)?;

    apply_scatterer_layout(
        &socket_path,
        &created.workspace_id,
        Some(&created.initial_tab_id),
        &source.cwd,
        &config,
        None,
    )?;

    println!(
        "scatterer: created workspace {} and applied layout",
        created.workspace_id
    );
    println!("scatterer: cwd {}", source.cwd.display());
    if config_path.is_empty() {
        println!("scatterer: no Scatterer config found; used default agent/hunk commands");
    } else {
        for path in config_path {
            println!("scatterer: project config {}", path.display());
        }
    }

    Ok(())
}

pub(crate) fn apply_scatterer_layout(
    socket_path: &Path,
    workspace_id: &str,
    replace_tab_id: Option<&str>,
    cwd: &Path,
    config: &ProjectConfig,
    agent_override: Option<&str>,
) -> Result<()> {
    let cwd_string = cwd.to_string_lossy().to_string();
    let agent_default = config.layout.agent.as_deref().unwrap_or(
        "if command -v pi >/dev/null 2>&1; then pi; else echo 'pi not found on PATH'; fi",
    );
    let agent = agent_override.unwrap_or(agent_default);
    let hunk = config.layout.hunk.as_deref().unwrap_or(
        "if command -v hunk >/dev/null 2>&1; then hunk; else echo 'hunk not found on PATH'; fi",
    );
    let runner = optional_command(config.layout.runner.as_deref());
    let git = optional_command(config.layout.git.as_deref());

    let load_direnv = config.env.direnv_enabled();
    let dev_root = json!({
        "type": "split",
        "direction": "right",
        "ratio": 0.58,
        "first": pane("pi", &cwd_string, agent, load_direnv),
        "second": pane("hunk", &cwd_string, hunk, load_direnv),
    });

    apply_tab(
        socket_path,
        workspace_id,
        replace_tab_id,
        "agent",
        dev_root,
        true,
    )?;
    if let Some(runner) = runner {
        apply_tab(
            socket_path,
            workspace_id,
            None,
            "runner",
            pane("runner", &cwd_string, runner, load_direnv),
            false,
        )?;
    }
    if let Some(git) = git {
        apply_tab(
            socket_path,
            workspace_id,
            None,
            "git",
            pane("git", &cwd_string, git, load_direnv),
            false,
        )?;
    }

    Ok(())
}

pub(crate) fn create_workspace(socket_path: &Path, cwd: &Path) -> Result<CreatedWorkspace> {
    let cwd_string = cwd.to_string_lossy().to_string();
    let label = workspace_label(cwd);
    let result = socket_call(
        socket_path,
        "workspace.create",
        json!({
            "cwd": cwd_string,
            "label": label,
            "focus": true,
        }),
    )
    .context("failed to create Scatterer workspace")?;

    let workspace_id = first_string(
        &result,
        &[
            &["workspace", "workspace_id"],
            &["workspace", "id"],
            &["workspace_id"],
        ],
    )
    .ok_or_else(|| anyhow!("workspace.create response did not include a workspace id: {result}"))?;

    let initial_tab_id = first_string(
        &result,
        &[
            &["tab", "tab_id"],
            &["tab", "id"],
            &["root_pane", "tab_id"],
            &["pane", "tab_id"],
            &["tab_id"],
        ],
    )
    .ok_or_else(|| {
        anyhow!("workspace.create response did not include an initial tab id: {result}")
    })?;

    Ok(CreatedWorkspace {
        workspace_id,
        initial_tab_id,
    })
}

fn workspace_label(cwd: &Path) -> String {
    let name = git_branch(cwd).unwrap_or_else(|| {
        cwd.file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("workspace")
            .to_string()
    });
    format!("{name} · scatterer")
}

fn pane(label: &str, cwd: &str, command: &str, load_direnv: bool) -> Value {
    json!({
        "type": "pane",
        "label": label,
        "cwd": cwd,
        "command": pane_env::shell_command(command, load_direnv),
    })
}

fn optional_command(command: Option<&str>) -> Option<&str> {
    command.and_then(|command| {
        let command = command.trim();
        (!command.is_empty()).then_some(command)
    })
}

fn apply_tab(
    socket_path: &Path,
    workspace_id: &str,
    replace_tab_id: Option<&str>,
    tab_label: &str,
    root: Value,
    focus: bool,
) -> Result<()> {
    let mut params = serde_json::Map::new();
    if let Some(tab_id) = replace_tab_id {
        // `layout.apply` accepts either `tab_id` for replacement or
        // `workspace_id` for a new tab, but not both.
        params.insert("tab_id".to_string(), json!(tab_id));
    } else {
        params.insert("workspace_id".to_string(), json!(workspace_id));
    }
    params.insert("tab_label".to_string(), json!(tab_label));
    params.insert("focus".to_string(), json!(focus));
    params.insert("root".to_string(), root);

    socket_call(socket_path, "layout.apply", Value::Object(params))
        .with_context(|| format!("failed to apply '{tab_label}' tab"))?;
    Ok(())
}
