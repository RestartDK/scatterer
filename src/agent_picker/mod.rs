mod tui;

use crate::herdr::{AgentStatus, Entrypoint, HerdrClient, Placement};
use crate::ids::{PaneId, TabId, WorkspaceId};
use crate::util::string_at;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentRow {
    pub(super) workspace_id: WorkspaceId,
    pub(super) workspace_label: String,
    pub(super) tab_id: TabId,
    pub(super) pane_id: PaneId,
    pub(super) label: String,
    pub(super) agent: String,
    pub(super) status: AgentStatus,
    pub(super) focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorkspaceGroup {
    pub(super) workspace_id: WorkspaceId,
    pub(super) label: String,
    pub(super) agents: Vec<AgentRow>,
}

pub(crate) fn open() -> Result<()> {
    HerdrClient::from_env()?
        .open_plugin_pane(Entrypoint::AgentPicker, Placement::Popup, json!({}))
        .context("failed to open Scatterer agent picker popup")?;
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    tui::run_agent_picker_tui()
}

impl WorkspaceGroup {
    /// Load every workspace with its agents, grouped and sorted for display.
    pub(super) fn load_all(client: &HerdrClient) -> Result<Vec<WorkspaceGroup>> {
        let workspaces = client.list_workspaces()?;
        let agents = client.list_agents()?;

        let mut groups = workspaces
            .iter()
            .filter_map(|workspace| {
            let workspace_id = string_at(workspace, &["workspace_id"]).map(WorkspaceId::from)?;
            let label = string_at(workspace, &["label"])
                .unwrap_or_else(|| workspace_id.as_str().to_string());
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

        for value in &agents {
        let Some(workspace_id) = string_at(value, &["workspace_id"]).map(WorkspaceId::from) else {
            continue;
        };
        let Some(pane_id) = string_at(value, &["pane_id"]).map(PaneId::from) else {
            continue;
        };
        let tab_id = TabId::from(string_at(value, &["tab_id"]).unwrap_or_default());
        let display_agent = string_at(value, &["display_agent"])
            .or_else(|| string_at(value, &["agent"]))
            .unwrap_or_else(|| "agent".to_string());
        let label = string_at(value, &["name"])
            .or_else(|| string_at(value, &["title"]))
            .or_else(|| string_at(value, &["terminal_title_stripped"]))
            .unwrap_or_else(|| display_agent.clone());
        let status = string_at(value, &["agent_status"])
            .as_deref()
            .map_or(AgentStatus::Unknown, AgentStatus::from);
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
                label: workspace_id.as_str().to_string(),
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
                left.pane_id
                    .sort_key()
                    .cmp(&right.pane_id.sort_key())
                    .then_with(|| left.pane_id.cmp(&right.pane_id))
            });
        }
        Ok(groups)
    }
}
