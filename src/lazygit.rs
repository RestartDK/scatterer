use crate::herdr::{Entrypoint, HerdrClient, Placement};
use anyhow::{Context, Result, anyhow};
use serde_json::json;
use std::env;
use std::process::Command;

pub(crate) fn open() -> Result<()> {
    let client = HerdrClient::from_env()?;
    let source = client.invocation_source()?;
    client
        .open_plugin_pane(
            Entrypoint::Lazygit,
            Placement::Overlay,
            json!({
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
    let client = HerdrClient::from_env()?;
    let source = client.invocation_source()?;
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
