use super::{QuickStartForm, quick_start_name};
use crate::herdr::socket_call;
use crate::util::slugify;
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) fn start_pi_agent(
    socket_path: &Path,
    pane_id: &str,
    workspace_id: &str,
    form: &QuickStartForm,
    session_name: &str,
) -> Result<()> {
    let session_name = pi_session_name(form, session_name);
    let agent_name = herdr_agent_name(&session_name, workspace_id);
    let mut args = vec!["--name".to_string(), session_name];
    if let Some(model) = &form.model {
        args.push("--model".to_string());
        args.push(model.clone());
    }

    socket_call(
        socket_path,
        "agent.start",
        json!({
            "name": agent_name,
            "kind": "pi",
            "pane_id": pane_id,
            "args": args,
            "timeout_ms": 60_000,
        }),
    )
    .with_context(|| format!("failed to start Pi in pane {pane_id}"))?;

    let prompt = form.prompt.trim();
    if !prompt.is_empty() {
        socket_call(
            socket_path,
            "agent.prompt",
            json!({
                "target": pane_id,
                "text": prompt,
            }),
        )
        .with_context(|| format!("failed to prompt Pi in pane {pane_id}"))?;
    }

    Ok(())
}

fn pi_session_name(form: &QuickStartForm, session_name: &str) -> String {
    let session_name = session_name.trim();
    if session_name.is_empty() {
        quick_start_name(&form.prompt)
    } else {
        session_name.to_string()
    }
}

fn herdr_agent_name(session_name: &str, workspace_id: &str) -> String {
    let session = slugify(session_name, 22);
    let workspace = slugify(workspace_id, 6);
    format!("pi-{session}-{workspace}")
        .chars()
        .take(32)
        .collect()
}

pub(super) fn available_pi_models() -> Vec<String> {
    let output = Command::new("pi")
        .arg("--list-models")
        .stdin(Stdio::null())
        .output();

    let mut models = vec!["default".to_string()];
    let Ok(output) = output else {
        return models;
    };
    if !output.status.success() {
        return models;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().skip(1) {
        let mut parts = line.split_whitespace();
        let Some(provider) = parts.next() else {
            continue;
        };
        let Some(model) = parts.next() else {
            continue;
        };
        models.push(format!("{provider}/{model}"));
    }
    models.sort();
    models.dedup();
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn herdr_agent_names_are_strict_and_bounded() {
        let name = herdr_agent_name("Feature/Very Long Branch With Spaces", "w123456789");
        assert!(name.len() <= 32);
        assert!(name.starts_with("pi-"));
        assert!(
            name.chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
        );
    }
}
