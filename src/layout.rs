use crate::config::{ProjectConfig, load_project_config};
use crate::git::{git_branch, git_parent_branch};
use crate::herdr::{herdr_socket_path, resolve_invocation_source, socket_call};
use crate::pane_env;
use crate::util::{first_string, shell_quote};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Debug)]
pub(crate) struct CreatedWorkspace {
    pub(crate) workspace_id: String,
    pub(crate) initial_tab_id: String,
}

#[derive(Debug)]
pub(crate) struct AppliedLayout {
    pub(crate) agent_pane_id: String,
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
        false,
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
    parent_branch_hint: Option<&str>,
    defer_agent_start: bool,
) -> Result<AppliedLayout> {
    let cwd_string = cwd.to_string_lossy().to_string();
    let agent = config.layout.agent.as_deref().unwrap_or(
        "if command -v pi >/dev/null 2>&1; then pi; else echo 'pi not found on PATH'; fi",
    );
    let computed_hunk_command;
    let hunk = if let Some(hunk) = config.layout.hunk.as_deref() {
        hunk
    } else {
        computed_hunk_command = default_hunk_command(cwd, parent_branch_hint);
        computed_hunk_command.as_str()
    };
    let runner = optional_command(config.layout.runner.as_deref());
    let git = optional_command(config.layout.git.as_deref());

    let load_direnv = config.env.direnv_enabled();
    let agent_pane = if defer_agent_start {
        // Start an interactive shell first. Herdr's agent.start facade then
        // validates that Pi becomes ready in this exact pane before prompting.
        pane("pi", &cwd_string, "true", load_direnv)
    } else {
        pane("pi", &cwd_string, agent, load_direnv)
    };
    let dev_root = json!({
        "type": "split",
        "direction": "right",
        "ratio": 0.58,
        "first": agent_pane,
        "second": pane("hunk", &cwd_string, hunk, load_direnv),
    });

    let agent_layout = apply_tab(
        socket_path,
        workspace_id,
        replace_tab_id,
        "agent",
        dev_root,
        true,
    )?;
    let agent_pane_id = pane_id_with_label(&agent_layout, "pi").ok_or_else(|| {
        anyhow!("layout.apply response did not include the Scatterer agent pane: {agent_layout}")
    })?;
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

    Ok(AppliedLayout { agent_pane_id })
}

pub(crate) fn create_workspace(socket_path: &Path, cwd: &Path) -> Result<CreatedWorkspace> {
    let label = workspace_label(cwd);
    create_workspace_with_label(socket_path, cwd, &label)
}

pub(crate) fn create_workspace_with_label(
    socket_path: &Path,
    cwd: &Path,
    label: &str,
) -> Result<CreatedWorkspace> {
    let cwd_string = cwd.to_string_lossy().to_string();
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

fn default_hunk_command(cwd: &Path, parent_branch_hint: Option<&str>) -> String {
    let parent_branch = parent_branch_hint
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(ToString::to_string)
        .or_else(|| git_parent_branch(cwd))
        .unwrap_or_else(|| "main".to_string());

    format!(
        "if command -v hunk >/dev/null 2>&1; then hunk diff {}... --watch; else echo 'hunk not found on PATH'; fi",
        shell_quote(&parent_branch)
    )
}

fn apply_tab(
    socket_path: &Path,
    workspace_id: &str,
    replace_tab_id: Option<&str>,
    tab_label: &str,
    root: Value,
    focus: bool,
) -> Result<Value> {
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
        .with_context(|| format!("failed to apply '{tab_label}' tab"))
}

fn pane_id_with_label(result: &Value, expected_label: &str) -> Option<String> {
    fn visit(node: &Value, expected_label: &str) -> Option<String> {
        if node.get("type").and_then(Value::as_str) == Some("pane")
            && node.get("label").and_then(Value::as_str) == Some(expected_label)
        {
            return node
                .get("pane_id")
                .and_then(Value::as_str)
                .map(ToString::to_string);
        }
        visit(node.get("first")?, expected_label)
            .or_else(|| visit(node.get("second")?, expected_label))
    }

    result
        .get("layout")
        .and_then(|layout| layout.get("root"))
        .and_then(|root| visit(root, expected_label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hunk_command_uses_parent_hint_with_watch() {
        let command = default_hunk_command(Path::new("."), Some("parent/branch"));

        assert!(command.contains("hunk diff 'parent/branch'... --watch"));
    }

    #[test]
    fn finds_labeled_agent_pane_in_applied_layout() {
        let result = json!({
            "layout": {
                "root": {
                    "type": "split",
                    "first": { "type": "pane", "label": "pi", "pane_id": "w1:p2" },
                    "second": { "type": "pane", "label": "hunk", "pane_id": "w1:p3" }
                }
            }
        });

        assert_eq!(pane_id_with_label(&result, "pi").as_deref(), Some("w1:p2"));
    }
}
