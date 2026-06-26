use crate::config::{ProjectConfig, load_project_config};
use crate::herdr::{herdr_socket_path, resolve_invocation_source, socket_call};
use crate::util::first_string;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Debug)]
struct CreatedWorkspace {
    workspace_id: String,
    initial_tab_id: String,
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
    if let Some(path) = config_path {
        println!("scatterer: project config {}", path.display());
    } else {
        println!("scatterer: no .scatterer.toml found; used default commands");
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
    let runner = config.layout.runner.as_deref().unwrap_or(
        "if command -v process-compose >/dev/null 2>&1 && { [ -f process-compose.yml ] || [ -f process-compose.yaml ]; }; then process-compose up; else echo 'No process-compose.yml/yaml found. Configure [layout].runner in .scatterer.toml to override this tab.'; fi",
    );
    let git = config.layout.git.as_deref().unwrap_or(
        "if command -v lazygit >/dev/null 2>&1; then lazygit; else echo 'lazygit not found; showing git status'; git status --short; fi",
    );

    let dev_root = json!({
        "type": "split",
        "direction": "right",
        "ratio": 0.58,
        "first": pane("pi", &cwd_string, agent),
        "second": pane("hunk", &cwd_string, hunk),
    });

    apply_tab(
        socket_path,
        workspace_id,
        replace_tab_id,
        "agent",
        dev_root,
        true,
    )?;
    apply_tab(
        socket_path,
        workspace_id,
        None,
        "runner",
        pane("runner", &cwd_string, runner),
        false,
    )?;
    apply_tab(
        socket_path,
        workspace_id,
        None,
        "git",
        pane("lazygit", &cwd_string, git),
        false,
    )?;

    Ok(())
}

fn create_workspace(socket_path: &Path, cwd: &Path) -> Result<CreatedWorkspace> {
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
    let name = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("workspace");
    format!("{name} · scatterer")
}

fn pane(label: &str, cwd: &str, command: &str) -> Value {
    json!({
        "type": "pane",
        "label": label,
        "cwd": cwd,
        "command": shell_command(command),
    })
}

fn shell_command(command: &str) -> Value {
    let wrapped = format!(
        r#"if command -v direnv >/dev/null 2>&1 && [ -f .envrc ]; then
  if direnv_export="$(direnv export bash)"; then
    eval "$direnv_export"
  else
    printf '[scatterer] direnv export failed; continuing without direnv env\n' >&2
  fi
fi
{command}
status=$?
printf '\n[scatterer] command exited with status %s; starting shell...\n' "$status"
exec "${{SHELL:-sh}}"
"#
    );
    json!(["bash", "-lc", wrapped])
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
