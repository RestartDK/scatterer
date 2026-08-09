use crate::herdr::{herdr_socket_path, resolve_invocation_source, socket_call};
use crate::util::{non_empty_env, string_at};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::env;
use std::path::Path;
use std::process::Command;

const REVIEW_ENTRYPOINT: &str = "review";
const REVIEW_PANE_TITLE: &str = "Review";

pub(crate) fn toggle() -> Result<()> {
    let socket_path = herdr_socket_path()?;
    let plugin_id =
        non_empty_env("HERDR_PLUGIN_ID").unwrap_or_else(|| "daniel.scatterer".to_string());
    let source = resolve_invocation_source(&socket_path)?;
    let current = current_pane(&socket_path)?;
    let pane_id = required_string(&current, &["pane_id"])?;
    let workspace_id = required_string(&current, &["workspace_id"])?;
    let tab_id = required_string(&current, &["tab_id"])?;

    if let Some(existing) = find_review_pane(&socket_path, &workspace_id, &tab_id)? {
        socket_call(&socket_path, "pane.close", json!({ "pane_id": existing }))
            .context("failed to close Review pane")?;
        return Ok(());
    }

    if !is_git_repository(&source.cwd) {
        socket_call(
            &socket_path,
            "notification.show",
            json!({
                "title": "Review unavailable",
                "body": format!("Not a Git repository: {}", source.cwd.display()),
                "position": "bottom-right",
                "sound": "none",
            }),
        )
        .context("failed to show the Review error notification")?;
        return Ok(());
    }

    socket_call(
        &socket_path,
        "plugin.pane.open",
        json!({
            "plugin_id": plugin_id,
            "entrypoint": REVIEW_ENTRYPOINT,
            "placement": "split",
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
    let socket_path = herdr_socket_path()?;
    let source = resolve_invocation_source(&socket_path)?;
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
    let Some(pane_id) = non_empty_env("HERDR_PANE_ID") else {
        return;
    };
    let Ok(socket_path) = herdr_socket_path() else {
        return;
    };
    let _ = socket_call(&socket_path, "pane.close", json!({ "pane_id": pane_id }));
}

fn current_pane(socket_path: &Path) -> Result<Value> {
    let caller_pane_id = non_empty_env("HERDR_PANE_ID").or_else(context_pane_id);
    let result = socket_call(
        socket_path,
        "pane.current",
        json!({ "caller_pane_id": caller_pane_id }),
    )
    .context("failed to resolve the focused Herdr pane")?;
    result
        .get("pane")
        .cloned()
        .ok_or_else(|| anyhow!("Herdr pane.current response did not include a pane: {result}"))
}

fn context_pane_id() -> Option<String> {
    let context = env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())?;
    string_at(&context, &["focused_pane", "pane_id"])
        .or_else(|| string_at(&context, &["pane", "pane_id"]))
        .or_else(|| string_at(&context, &["pane_id"]))
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
    socket_path: &Path,
    workspace_id: &str,
    tab_id: &str,
) -> Result<Option<String>> {
    let result = socket_call(
        socket_path,
        "pane.list",
        json!({ "workspace_id": workspace_id }),
    )
    .context("failed to list Herdr panes")?;
    let panes = result
        .get("panes")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Herdr pane.list response did not include panes: {result}"))?;

    for pane in panes {
        if string_at(pane, &["tab_id"]).as_deref() != Some(tab_id) {
            continue;
        }
        if is_review_pane(socket_path, pane) {
            return Ok(string_at(pane, &["pane_id"]));
        }
    }
    Ok(None)
}

fn is_review_pane(socket_path: &Path, pane: &Value) -> bool {
    if ["label", "title"]
        .iter()
        .filter_map(|field| string_at(pane, &[*field]))
        .any(|value| value == REVIEW_PANE_TITLE)
    {
        return true;
    }

    let Some(pane_id) = string_at(pane, &["pane_id"]) else {
        return false;
    };
    let Ok(result) = socket_call(
        socket_path,
        "pane.process_info",
        json!({ "pane_id": pane_id }),
    ) else {
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
