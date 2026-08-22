mod agents;
mod github;
mod tui;

use crate::herdr::{AgentStatus, Entrypoint, HerdrClient, Placement};
use crate::ids::{PaneId, WorkspaceId};
use anyhow::{Context, Result};
use serde_json::json;

/// PR lifecycle state. Variant order is the picker sort order, so deriving
/// `Ord` replaces a hand-written rank function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum PrState {
    Open,
    Draft,
    Closed,
    Merged,
}

impl PrState {
    pub(super) fn icon(self) -> &'static str {
        match self {
            Self::Open => "\u{F407}",
            Self::Draft => "\u{F4DD}",
            Self::Merged => "\u{F419}",
            Self::Closed => "\u{F4DC}",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Draft => "DRAFT",
            Self::Merged => "MERGED",
            Self::Closed => "CLOSED",
        }
    }

    /// Lowercase token used in Herdr pane metadata.
    pub(super) fn metadata_key(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Draft => "draft",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }
}

/// Aggregated CI status for a PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CheckState {
    Pass,
    Pending,
    Fail,
    None,
}

impl CheckState {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Pass => "CI OK",
            Self::Pending => "CI WAIT",
            Self::Fail => "CI FAIL",
            Self::None => "CI -",
        }
    }

    pub(super) fn icon(self) -> &'static str {
        match self {
            Self::Pass => "\u{F4A4}",
            Self::Pending => "\u{F4AA}",
            Self::Fail => "\u{F530}",
            Self::None => "-",
        }
    }
}

/// GitHub's `reviewDecision` values, parsed once at the `gh` boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

impl ReviewDecision {
    pub(super) fn from_gh(value: &str) -> Option<Self> {
        match value {
            "APPROVED" => Some(Self::Approved),
            "CHANGES_REQUESTED" => Some(Self::ChangesRequested),
            "REVIEW_REQUIRED" => Some(Self::ReviewRequired),
            _ => None,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes",
            Self::ReviewRequired => "review",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PrRow {
    pub(super) url: String,
    pub(super) number: u64,
    pub(super) title: String,
    pub(super) state: PrState,
    pub(super) checks: CheckState,
    pub(super) comments: usize,
    pub(super) additions: u64,
    pub(super) deletions: u64,
    pub(super) changed_files: u64,
    pub(super) review: Option<ReviewDecision>,
    pub(super) agent: String,
    pub(super) agent_status: AgentStatus,
    pub(super) branch: String,
    pub(super) workspace_id: WorkspaceId,
    pub(super) pane_id: PaneId,
}

pub(crate) fn open() -> Result<()> {
    HerdrClient::from_env()?
        .open_plugin_pane(Entrypoint::PrPicker, Placement::Popup, json!({}))
        .context("failed to open Scatterer PR picker popup")?;
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    tui::run_pr_picker_tui()
}
