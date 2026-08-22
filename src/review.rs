use crate::herdr::{Entrypoint, HerdrClient, Placement};
use crate::ids::{PaneId, TabId, WorkspaceId};
use crate::util::{non_empty_env, string_at};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::env;
use std::path::Path;
use std::process::Command;

const REVIEW_PANE_TITLE: &str = "Review";

pub(crate) fn toggle() -> Result<()> {
    let client = HerdrClient::from_env()?;
    let source = client.invocation_source()?;
    let caller_pane_id = non_empty_env("HERDR_PANE_ID")
        .map(PaneId::from)
        .or_else(context_pane_id);
    let current = client.current_pane(caller_pane_id.as_ref())?;
    let pane_id = PaneId::from(required_string(&current, &["pane_id"])?);
    let workspace_id = WorkspaceId::from(required_string(&current, &["workspace_id"])?);
    let tab_id = TabId::from(required_string(&current, &["tab_id"])?);

    if let Some(existing) = find_review_pane(&client, &workspace_id, &tab_id)? {
        client
            .close_pane(&existing)
            .context("failed to close Review pane")?;
        return Ok(());
    }

    if !is_git_repository(&source.cwd) {
        client
            .show_notification(
                "Review unavailable",
                &format!("Not a Git repository: {}", source.cwd.display()),
            )
            .context("failed to show the Review error notification")?;
        return Ok(());
    }

    client
        .open_plugin_pane(
            Entrypoint::Review,
            Placement::Split,
            json!({
                "target_pane_id": pane_id,
                "direction": "right",
                "focus": true,
                "env": {
                    "SCATTERER_SOURCE_CWD": source.cwd.to_string_lossy(),
                },
            }),
        )
        .context("failed to open Review pane")?;
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    let result = run_tuicr();
    close_own_pane();
    result
}

fn run_tuicr() -> Result<()> {
    let client = HerdrClient::from_env()?;
    let source = client.invocation_source()?;
    env::set_current_dir(&source.cwd)
        .with_context(|| format!("failed to enter {}", source.cwd.display()))?;

    let status = Command::new("tuicr")
        .arg("--working-tree")
        .status()
        .context("failed to launch tuicr")?;
    if !status.success() {
        return Err(anyhow!("tuicr exited with status {status}"));
    }
    Ok(())
}

fn close_own_pane() {
    let Some(pane_id) = non_empty_env("HERDR_PANE_ID").map(PaneId::from) else {
        return;
    };
    let Ok(client) = HerdrClient::from_env() else {
        return;
    };
    let _ = client.close_pane(&pane_id);
}

fn context_pane_id() -> Option<PaneId> {
    let context = env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())?;
    string_at(&context, &["focused_pane", "pane_id"])
        .or_else(|| string_at(&context, &["pane", "pane_id"]))
        .or_else(|| string_at(&context, &["pane_id"]))
        .map(PaneId::from)
}

fn is_git_repository(cwd: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn find_review_pane(
    client: &HerdrClient,
    workspace_id: &WorkspaceId,
    tab_id: &TabId,
) -> Result<Option<PaneId>> {
    let panes = client.list_panes(Some(workspace_id))?;

    for pane in &panes {
        if string_at(pane, &["tab_id"]).as_deref() != Some(tab_id.as_str()) {
            continue;
        }
        if is_review_pane(client, pane) {
            return Ok(string_at(pane, &["pane_id"]).map(PaneId::from));
        }
    }
    Ok(None)
}

fn is_review_pane(client: &HerdrClient, pane: &Value) -> bool {
    if ["label", "title"]
        .iter()
        .filter_map(|field| string_at(pane, &[*field]))
        .any(|value| value == REVIEW_PANE_TITLE)
    {
        return true;
    }

    let Some(pane_id) = string_at(pane, &["pane_id"]).map(PaneId::from) else {
        return false;
    };
    let Ok(result) = client.pane_process_info(&pane_id) else {
        return false;
    };
    result
        .get("process_info")
        .and_then(|info| info.get("foreground_processes"))
        .and_then(Value::as_array)
        .is_some_and(|processes| processes.iter().any(process_is_review))
}

fn process_is_review(process: &Value) -> bool {
    let argv = process.get("argv").and_then(Value::as_array);
    let executable = string_at(process, &["argv0"]).or_else(|| {
        argv.and_then(|args| args.first())
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    });
    let basename = executable
        .as_deref()
        .and_then(|value| Path::new(value).file_name().and_then(|name| name.to_str()));

    basename == Some("tuicr")
        || (basename == Some("scatterer")
            && argv.is_some_and(|args| args.iter().any(|arg| arg.as_str() == Some("review"))))
}

fn required_string(value: &Value, path: &[&str]) -> Result<String> {
    string_at(value, path).ok_or_else(|| anyhow!("missing {} in {value}", path.join(".")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_tuicr_executable_paths() {
        assert!(process_is_review(&json!({
            "argv0": "/nix/store/example-tuicr/bin/tuicr",
            "argv": ["/nix/store/example-tuicr/bin/tuicr", "--working-tree"]
        })));
        assert!(!process_is_review(&json!({
            "argv0": "/bin/zsh",
            "argv": ["/bin/zsh"]
        })));
    }

    #[test]
    fn recognizes_review_runner_argument() {
        assert!(process_is_review(&json!({
            "argv0": "scatterer",
            "argv": ["scatterer", "review"]
        })));
    }
}
