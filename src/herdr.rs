use crate::ids::{PaneId, TabId, WorkspaceId};
use crate::util::{first_string, non_empty_env, string_at};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::env;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_PLUGIN_ID: &str = "daniel.scatterer";

/// Every Herdr JSON-RPC method Scatterer calls. Method names exist in exactly
/// one place, so a typo is a compile error instead of a runtime failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    WorkspaceCreate,
    WorkspaceClose,
    WorkspaceFocus,
    WorkspaceList,
    WorktreeCreate,
    AgentList,
    AgentFocus,
    AgentStart,
    AgentPrompt,
    PaneCurrent,
    PaneList,
    PaneClose,
    PaneRead,
    PaneProcessInfo,
    PaneReportMetadata,
    PaneSendText,
    LayoutApply,
    NotificationShow,
    PluginPaneOpen,
}

impl Method {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceCreate => "workspace.create",
            Self::WorkspaceClose => "workspace.close",
            Self::WorkspaceFocus => "workspace.focus",
            Self::WorkspaceList => "workspace.list",
            Self::WorktreeCreate => "worktree.create",
            Self::AgentList => "agent.list",
            Self::AgentFocus => "agent.focus",
            Self::AgentStart => "agent.start",
            Self::AgentPrompt => "agent.prompt",
            Self::PaneCurrent => "pane.current",
            Self::PaneList => "pane.list",
            Self::PaneClose => "pane.close",
            Self::PaneRead => "pane.read",
            Self::PaneProcessInfo => "pane.process_info",
            Self::PaneReportMetadata => "pane.report_metadata",
            Self::PaneSendText => "pane.send_text",
            Self::LayoutApply => "layout.apply",
            Self::NotificationShow => "notification.show",
            Self::PluginPaneOpen => "plugin.pane.open",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Scatterer plugin entrypoints as declared in `herdr-plugin.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entrypoint {
    QuickStart,
    PrPicker,
    AgentPicker,
    Lazygit,
    Review,
}

impl Entrypoint {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::QuickStart => "quick-start",
            Self::PrPicker => "pr-picker",
            Self::AgentPicker => "agent-picker",
            Self::Lazygit => "lazygit",
            Self::Review => "review",
        }
    }
}

impl fmt::Display for Entrypoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Placement of a plugin pane opened through `plugin.pane.open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    Popup,
    Overlay,
    Split,
}

impl Placement {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Popup => "popup",
            Self::Overlay => "overlay",
            Self::Split => "split",
        }
    }
}

/// Lifecycle status Herdr reports for an agent pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentStatus {
    Blocked,
    Working,
    Idle,
    Done,
    Unknown,
}

impl AgentStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Working => "working",
            Self::Idle => "idle",
            Self::Done => "done",
            Self::Unknown => "unknown",
        }
    }
}

impl From<&str> for AgentStatus {
    fn from(value: &str) -> Self {
        match value {
            "blocked" => Self::Blocked,
            "working" => Self::Working,
            "idle" => Self::Idle,
            "done" => Self::Done,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug)]
pub(crate) struct InvocationSource {
    pub(crate) cwd: PathBuf,
}

/// Parsed `workspace.create` response.
#[derive(Debug)]
pub(crate) struct CreatedWorkspace {
    pub(crate) workspace_id: WorkspaceId,
    pub(crate) initial_tab_id: TabId,
}

/// Parsed `worktree.create` response.
#[derive(Debug)]
pub(crate) struct CreatedWorktree {
    pub(crate) workspace_id: WorkspaceId,
    pub(crate) initial_tab_id: Option<TabId>,
    pub(crate) path: PathBuf,
}

/// The Herdr plugin id, honoring the `HERDR_PLUGIN_ID` override.
pub(crate) fn plugin_id() -> String {
    non_empty_env("HERDR_PLUGIN_ID").unwrap_or_else(|| DEFAULT_PLUGIN_ID.to_string())
}

/// A handle to the Herdr control socket. All Scatterer↔Herdr RPC flows
/// through this type, so method names, plugin ids, and entrypoints stay typed.
#[derive(Debug, Clone)]
pub(crate) struct HerdrClient {
    socket_path: PathBuf,
}

impl HerdrClient {
    /// Resolve the socket location from the environment.
    pub(crate) fn from_env() -> Result<Self> {
        Ok(Self {
            socket_path: herdr_socket_path()?,
        })
    }

    pub(crate) fn call(&self, method: Method, params: Value) -> Result<Value> {
        socket_call(&self.socket_path, method, params)
    }

    /// Open one of Scatterer's own plugin panes. Centralizes the
    /// `plugin.pane.open` payload that was previously duplicated per command.
    pub(crate) fn open_plugin_pane(
        &self,
        entrypoint: Entrypoint,
        placement: Placement,
        extra: Value,
    ) -> Result<Value> {
        let mut params = serde_json::Map::new();
        params.insert("plugin_id".to_string(), json!(plugin_id()));
        params.insert("entrypoint".to_string(), json!(entrypoint.as_str()));
        params.insert("placement".to_string(), json!(placement.as_str()));
        if let Value::Object(extra) = extra {
            params.extend(extra);
        }
        self.call(Method::PluginPaneOpen, Value::Object(params))
    }

    /// Determine the directory the user invoked Scatterer from.
    pub(crate) fn invocation_source(&self) -> Result<InvocationSource> {
        if let Some(cwd) = non_empty_env("SCATTERER_SOURCE_CWD") {
            return Ok(InvocationSource {
                cwd: PathBuf::from(cwd),
            });
        }

        let context = env::var("HERDR_PLUGIN_CONTEXT_JSON")
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());

        let pane_id = non_empty_env("HERDR_PANE_ID")
            .map(PaneId::from)
            .or_else(|| context.as_ref().and_then(pane_id_from_context));
        let mut cwd = context
            .as_ref()
            .and_then(cwd_from_context)
            .map(PathBuf::from);

        if cwd.is_none()
            && let Ok(pane) = self.current_pane(pane_id.as_ref())
        {
            cwd = string_at(&pane, &["foreground_cwd"])
                .or_else(|| string_at(&pane, &["cwd"]))
                .map(PathBuf::from);
        }

        let cwd = cwd.unwrap_or(env::current_dir().context("failed to resolve fallback cwd")?);

        Ok(InvocationSource { cwd })
    }

    // ---- workspaces ----

    pub(crate) fn create_workspace(&self, cwd: &Path, label: &str) -> Result<CreatedWorkspace> {
        let result = self
            .call(
                Method::WorkspaceCreate,
                json!({
                    "cwd": cwd.to_string_lossy(),
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
        .map(WorkspaceId::from)
        .ok_or_else(|| {
            anyhow!("workspace.create response did not include a workspace id: {result}")
        })?;

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
        .map(TabId::from)
        .ok_or_else(|| {
            anyhow!("workspace.create response did not include an initial tab id: {result}")
        })?;

        Ok(CreatedWorkspace {
            workspace_id,
            initial_tab_id,
        })
    }

    pub(crate) fn close_workspace(&self, workspace_id: &WorkspaceId) -> Result<()> {
        self.call(
            Method::WorkspaceClose,
            json!({ "workspace_id": workspace_id }),
        )
        .with_context(|| format!("failed to close workspace {workspace_id}"))?;
        Ok(())
    }

    pub(crate) fn list_workspaces(&self) -> Result<Vec<Value>> {
        let result = self
            .call(Method::WorkspaceList, json!({}))
            .context("failed to list Herdr workspaces")?;
        Ok(result
            .get("workspaces")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub(crate) fn create_worktree(
        &self,
        cwd: &Path,
        branch: &str,
        base: Option<&str>,
        label: &str,
        focus: bool,
    ) -> Result<CreatedWorktree> {
        let mut payload = json!({
            "cwd": cwd.to_string_lossy(),
            "branch": branch,
            "label": label,
            "focus": focus,
        });
        if let Some(base) = base.map(str::trim).filter(|base| !base.is_empty()) {
            payload["base"] = json!(base);
        }

        let result = self
            .call(Method::WorktreeCreate, payload)
            .context("failed to create Git worktree workspace")?;

        let workspace_id = first_string(
            &result,
            &[
                &["workspace", "workspace_id"],
                &["workspace", "id"],
                &["workspace_id"],
            ],
        )
        .map(WorkspaceId::from)
        .ok_or_else(|| {
            anyhow!("worktree.create response did not include a workspace id: {result}")
        })?;

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
        .map(TabId::from);

        let path = first_string(
            &result,
            &[&["worktree", "path"], &["workspace", "cwd"], &["path"]],
        )
        .map(PathBuf::from)
        .ok_or_else(|| {
            anyhow!("worktree.create response did not include a checkout path: {result}")
        })?;

        Ok(CreatedWorktree {
            workspace_id,
            initial_tab_id,
            path,
        })
    }

    // ---- agents ----

    pub(crate) fn list_agents(&self) -> Result<Vec<Value>> {
        let result = self
            .call(Method::AgentList, json!({}))
            .context("failed to list Herdr agents")?;
        Ok(result
            .get("agents")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// Focus a workspace, then the agent pane inside it.
    pub(crate) fn focus_agent_pane(
        &self,
        workspace_id: &WorkspaceId,
        pane_id: &PaneId,
    ) -> Result<()> {
        self.call(
            Method::WorkspaceFocus,
            json!({ "workspace_id": workspace_id }),
        )
        .with_context(|| format!("failed to focus workspace {workspace_id}"))?;
        self.call(Method::AgentFocus, json!({ "target": pane_id }))
            .with_context(|| format!("failed to focus agent pane {pane_id}"))?;
        Ok(())
    }

    // ---- panes ----

    /// Resolve the focused pane, unwrapping the `pane` envelope when present.
    pub(crate) fn current_pane(&self, caller_pane_id: Option<&PaneId>) -> Result<Value> {
        let result = self
            .call(
                Method::PaneCurrent,
                json!({ "caller_pane_id": caller_pane_id }),
            )
            .context("failed to resolve the focused Herdr pane")?;
        Ok(result.get("pane").cloned().unwrap_or(result))
    }

    pub(crate) fn list_panes(&self, workspace_id: Option<&WorkspaceId>) -> Result<Vec<Value>> {
        let params = match workspace_id {
            Some(workspace_id) => json!({ "workspace_id": workspace_id }),
            None => json!({}),
        };
        let result = self
            .call(Method::PaneList, params)
            .context("failed to list Herdr panes")?;
        result
            .get("panes")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| anyhow!("Herdr pane.list response did not include panes: {result}"))
    }

    pub(crate) fn close_pane(&self, pane_id: &PaneId) -> Result<()> {
        self.call(Method::PaneClose, json!({ "pane_id": pane_id }))
            .with_context(|| format!("failed to close pane {pane_id}"))?;
        Ok(())
    }

    pub(crate) fn pane_process_info(&self, pane_id: &PaneId) -> Result<Value> {
        self.call(Method::PaneProcessInfo, json!({ "pane_id": pane_id }))
    }

    /// Read the visible pane content with ANSI styling preserved.
    pub(crate) fn read_pane_visible_ansi(&self, pane_id: &PaneId) -> Result<String> {
        let result = self
            .call(
                Method::PaneRead,
                json!({
                    "pane_id": pane_id,
                    "source": "visible",
                    "format": "ansi",
                    "strip_ansi": false,
                }),
            )
            .with_context(|| format!("failed to read pane {pane_id}"))?;
        string_at(&result, &["read", "text"])
            .ok_or_else(|| anyhow!("pane.read response did not include read.text"))
    }

    /// Read recent unwrapped pane history as plain text.
    pub(crate) fn read_pane_recent_text(&self, pane_id: &PaneId, lines: u64) -> Result<String> {
        let result = self
            .call(
                Method::PaneRead,
                json!({
                    "pane_id": pane_id,
                    "source": "recent-unwrapped",
                    "lines": lines,
                }),
            )
            .with_context(|| format!("failed to read pane {pane_id}"))?;
        string_at(&result, &["read", "text"])
            .ok_or_else(|| anyhow!("pane.read response did not include read.text"))
    }

    pub(crate) fn send_text_to_pane(&self, pane_id: &PaneId, text: &str) -> Result<()> {
        self.call(
            Method::PaneSendText,
            json!({ "pane_id": pane_id, "text": text }),
        )
        .with_context(|| format!("failed to send text to pane {pane_id}"))?;
        Ok(())
    }

    // ---- notifications ----

    pub(crate) fn show_notification(&self, title: &str, body: &str) -> Result<()> {
        self.call(
            Method::NotificationShow,
            json!({
                "title": title,
                "body": body,
                "position": "bottom-right",
                "sound": "none",
            }),
        )
        .context("failed to show Herdr notification")?;
        Ok(())
    }
}

fn herdr_socket_path() -> Result<PathBuf> {
    if let Some(path) = non_empty_env("HERDR_SOCKET_PATH") {
        return Ok(PathBuf::from(path));
    }

    let config_home = non_empty_env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| non_empty_env("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| anyhow!("HERDR_SOCKET_PATH is not set and HOME is unavailable"))?;

    let herdr_dir = config_home.join("herdr");
    let socket = match non_empty_env("HERDR_SESSION").as_deref() {
        Some(session) if session != "default" => {
            herdr_dir.join("sessions").join(session).join("herdr.sock")
        }
        _ => herdr_dir.join("herdr.sock"),
    };
    Ok(socket)
}

fn socket_call(socket_path: &Path, method: Method, params: Value) -> Result<Value> {
    #[cfg(not(unix))]
    {
        let _ = (socket_path, method, params);
        return Err(anyhow!(
            "scatterer currently supports Herdr's Unix socket only"
        ));
    }

    #[cfg(unix)]
    {
        let mut stream = UnixStream::connect(socket_path).with_context(|| {
            format!(
                "failed to connect to Herdr socket {}",
                socket_path.display()
            )
        })?;
        let request = json!({
            "id": request_id(method),
            "method": method.as_str(),
            "params": params,
        });
        writeln!(stream, "{request}").context("failed to write Herdr socket request")?;
        stream
            .flush()
            .context("failed to flush Herdr socket request")?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .context("failed to read Herdr socket response")?;
        if line.trim().is_empty() {
            return Err(anyhow!("Herdr socket returned an empty response"));
        }

        let response: Value = serde_json::from_str(&line)
            .with_context(|| format!("failed to parse Herdr socket response: {line}"))?;
        if let Some(error) = response.get("error") {
            let code = string_at(error, &["code"]).unwrap_or_else(|| "error".to_string());
            let message = string_at(error, &["message"]).unwrap_or_else(|| error.to_string());
            return Err(anyhow!("Herdr {method} failed: {code}: {message}"));
        }

        response
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("Herdr {method} response did not include result: {response}"))
    }
}

fn pane_id_from_context(context: &Value) -> Option<PaneId> {
    first_string(
        context,
        &[
            &["pane_id"],
            &["focused_pane", "pane_id"],
            &["pane", "pane_id"],
        ],
    )
    .map(PaneId::from)
}

fn cwd_from_context(context: &Value) -> Option<String> {
    first_string(
        context,
        &[
            &["focused_pane", "foreground_cwd"],
            &["focused_pane", "cwd"],
            &["pane", "foreground_cwd"],
            &["pane", "cwd"],
            &["workspace", "cwd"],
            &["worktree", "path"],
        ],
    )
}

fn request_id(method: Method) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("scatterer-{method}-{millis}")
}
