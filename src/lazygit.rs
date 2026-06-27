use crate::herdr::{herdr_socket_path, resolve_invocation_source, socket_call};
use crate::util::non_empty_env;
use anyhow::{Context, Result, anyhow};
use serde_json::json;
use std::env;
use std::process::Command;

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
            "entrypoint": "lazygit",
            "placement": "overlay",
            "focus": true,
            "env": {
                "SCATTERER_SOURCE_CWD": source.cwd.to_string_lossy(),
            },
        }),
    )
    .context("failed to open lazygit overlay")?;
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    let socket_path = herdr_socket_path()?;
    let source = resolve_invocation_source(&socket_path)?;
    env::set_current_dir(&source.cwd)
        .with_context(|| format!("failed to enter {}", source.cwd.display()))?;

    let status = Command::new("lazygit")
        .status()
        .context("failed to launch lazygit")?;
    if !status.success() {
        return Err(anyhow!("lazygit exited with status {status}"));
    }

    Ok(())
}
