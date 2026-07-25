use crate::herdr::{herdr_socket_path, socket_call};
use crate::pr_picker;
use crate::util::non_empty_env;
use anyhow::{Context, Result, anyhow};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

pub(crate) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    match args.next().as_deref() {
        Some("enable-pr") => enable_pr_view(true),
        Some("disable-pr") => disable_pr_view(),
        Some("restore") => restore_pr_view(),
        Some(other) => Err(anyhow!("unknown agent-view command '{other}'")),
        None => Err(anyhow!("agent-view command is required")),
    }
}

fn enable_pr_view(persist: bool) -> Result<()> {
    pr_picker::refresh_metadata()?;
    let socket_path = herdr_socket_path()?;
    socket_call(
        &socket_path,
        "agent.view.set",
        json!({
            "source": view_source(),
            "label": "pull requests",
            "filter": {
                "op": "exists",
                "field": { "token": "pr_url" },
            },
            "sort": [
                { "field": "attention", "order": "desc" },
                { "field": "state_change_seq", "order": "desc" },
            ],
        }),
    )
    .context("failed to enable the Scatterer PR agent view")?;
    if persist {
        fs::write(view_state_path()?, b"enabled\n")
            .context("failed to save the Scatterer PR agent view preference")?;
    }
    println!("scatterer: Agents view now shows panes associated with pull requests");
    Ok(())
}

fn disable_pr_view() -> Result<()> {
    let socket_path = herdr_socket_path()?;
    socket_call(
        &socket_path,
        "agent.view.clear",
        json!({ "source": view_source() }),
    )
    .context("failed to clear the Scatterer PR agent view")?;
    let path = view_state_path()?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    println!("scatterer: restored the default Agents view");
    Ok(())
}

fn restore_pr_view() -> Result<()> {
    if view_state_path()?.exists() {
        enable_pr_view(false)?;
    }
    Ok(())
}

fn view_source() -> String {
    let plugin_id =
        non_empty_env("HERDR_PLUGIN_ID").unwrap_or_else(|| "daniel.scatterer".to_string());
    format!("plugin:{plugin_id}")
}

fn view_state_path() -> Result<PathBuf> {
    let directory = non_empty_env("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HERDR_PLUGIN_STATE_DIR is unavailable"))?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("failed to create {}", directory.display()))?;
    Ok(directory.join("pr-agent-view"))
}
