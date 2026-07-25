use crate::herdr::{herdr_socket_path, socket_call};
use anyhow::{Context, Result};
use serde_json::json;

pub(crate) fn focus_pane(workspace_id: &str, pane_id: &str) -> Result<()> {
    let socket_path = herdr_socket_path()?;
    socket_call(
        &socket_path,
        "workspace.focus",
        json!({ "workspace_id": workspace_id }),
    )
    .with_context(|| format!("failed to focus workspace {workspace_id}"))?;
    socket_call(&socket_path, "agent.focus", json!({ "target": pane_id }))
        .with_context(|| format!("failed to focus agent pane {pane_id}"))?;
    Ok(())
}
