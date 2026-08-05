mod tui;

use crate::herdr::{herdr_socket_path, socket_call};
use crate::util::{non_empty_env, string_at};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentStatus {
    Blocked,
    Working,
    Idle,
    Done,
    Unknown,
}

impl AgentStatus {
    fn parse(value: Option<String>) -> Self {
        match value.as_deref() {
            Some("blocked") => Self::Blocked,
            Some("working") => Self::Working,
            Some("idle") => Self::Idle,
            Some("done") => Self::Done,
            _ => Self::Unknown,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Working => "working",
            Self::Idle => "idle",
            Self::Done => "done",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentRow {
    pub(super) workspace_id: String,
    pub(super) workspace_label: String,
    pub(super) tab_id: String,
    pub(super) pane_id: String,
    pub(super) label: String,
    pub(super) agent: String,
    pub(super) status: AgentStatus,
    pub(super) focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorkspaceGroup {
    pub(super) workspace_id: String,
    pub(super) label: String,
    pub(super) agents: Vec<AgentRow>,
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
            "entrypoint": "agent-picker",
            "placement": "popup",
        }),
    )
    .context("failed to open Scatterer agent picker popup")?;
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    tui::run_agent_picker_tui()
}

pub(super) fn load_agent_groups(socket_path: &Path) -> Result<Vec<WorkspaceGroup>> {
    let workspace_result = socket_call(socket_path, "workspace.list", json!({}))
        .context("failed to list Herdr workspaces")?;
    let agent_result =
        socket_call(socket_path, "agent.list", json!({})).context("failed to list Herdr agents")?;

    let mut groups = workspace_result
        .get("workspaces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|workspace| {
            let workspace_id = string_at(workspace, &["workspace_id"])?;
            let label = string_at(workspace, &["label"]).unwrap_or_else(|| workspace_id.clone());
            Some(WorkspaceGroup {
                workspace_id,
                label,
                agents: Vec::new(),
            })
        })
        .collect::<Vec<_>>();
    let mut group_indexes = groups
        .iter()
        .enumerate()
        .map(|(index, group)| (group.workspace_id.clone(), index))
        .collect::<HashMap<_, _>>();

    for value in agent_result
        .get("agents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(workspace_id) = string_at(value, &["workspace_id"]) else {
            continue;
        };
        let Some(pane_id) = string_at(value, &["pane_id"]) else {
            continue;
        };
        let tab_id = string_at(value, &["tab_id"]).unwrap_or_default();
        let display_agent = string_at(value, &["display_agent"])
            .or_else(|| string_at(value, &["agent"]))
            .unwrap_or_else(|| "agent".to_string());
        let label = string_at(value, &["name"])
            .or_else(|| string_at(value, &["title"]))
            .or_else(|| string_at(value, &["terminal_title_stripped"]))
            .unwrap_or_else(|| display_agent.clone());
        let status = AgentStatus::parse(string_at(value, &["agent_status"]));
        let focused = value
            .get("focused")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let group_index = if let Some(index) = group_indexes.get(&workspace_id).copied() {
            index
        } else {
            let index = groups.len();
            groups.push(WorkspaceGroup {
                workspace_id: workspace_id.clone(),
                label: workspace_id.clone(),
                agents: Vec::new(),
            });
            group_indexes.insert(workspace_id.clone(), index);
            index
        };
        let workspace_label = groups[group_index].label.clone();
        groups[group_index].agents.push(AgentRow {
            workspace_id,
            workspace_label,
            tab_id,
            pane_id,
            label,
            agent: display_agent,
            status,
            focused,
        });
    }

    groups.retain(|group| !group.agents.is_empty());
    for group in &mut groups {
        group.agents.sort_by(|left, right| {
            pane_sort_key(&left.pane_id)
                .cmp(&pane_sort_key(&right.pane_id))
                .then_with(|| left.pane_id.cmp(&right.pane_id))
        });
    }
    Ok(groups)
}

pub(super) fn read_visible_ansi(socket_path: &Path, pane_id: &str) -> Result<String> {
    let result = socket_call(
        socket_path,
        "pane.read",
        json!({
            "pane_id": pane_id,
            "source": "visible",
            "format": "ansi",
            "strip_ansi": false,
        }),
    )
    .with_context(|| format!("failed to read agent pane {pane_id}"))?;
    string_at(&result, &["read", "text"])
        .ok_or_else(|| anyhow::anyhow!("pane.read response did not include read.text"))
}

fn pane_sort_key(pane_id: &str) -> u64 {
    pane_id
        .rsplit_once('p')
        .and_then(|(_, number)| number.parse().ok())
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::pane_sort_key;

    #[test]
    fn public_pane_ids_sort_numerically() {
        assert!(pane_sort_key("w1:p2") < pane_sort_key("w1:p10"));
        assert_eq!(pane_sort_key("unknown"), u64::MAX);
    }
}
