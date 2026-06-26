mod agents;
mod github;
mod tui;

use crate::herdr::{herdr_socket_path, socket_call};
use crate::util::non_empty_env;
use anyhow::{Context, Result};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PrState {
    Open,
    Draft,
    Merged,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CheckState {
    Pass,
    Pending,
    Fail,
    None,
}

#[derive(Debug, Clone)]
pub(super) struct PrRow {
    pub(super) url: String,
    pub(super) number: u64,
    pub(super) title: String,
    pub(super) state: PrState,
    pub(super) checks: CheckState,
    pub(super) comments: usize,
    pub(super) review: Option<String>,
    pub(super) agent: String,
    pub(super) agent_status: String,
    pub(super) branch: String,
    pub(super) workspace_id: String,
    pub(super) pane_id: String,
}

pub(crate) fn open() -> Result<()> {
    let socket_path = herdr_socket_path()?;
    let plugin_id =
        non_empty_env("HERDR_PLUGIN_ID").unwrap_or_else(|| "daniel.scatterer".to_string());
    socket_call(
        &socket_path,
        "plugin.pane.open",
        json!({
            "plugin_id": plugin_id,
            "entrypoint": "pr-picker",
            "placement": "overlay",
            "focus": true,
        }),
    )
    .context("failed to open Scatterer PR picker overlay")?;
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    tui::run_pr_picker_tui()
}

fn pr_state_rank(state: PrState) -> u8 {
    match state {
        PrState::Open => 0,
        PrState::Draft => 1,
        PrState::Closed => 2,
        PrState::Merged => 3,
    }
}
