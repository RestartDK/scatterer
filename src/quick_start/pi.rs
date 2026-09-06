use super::{QuickStartForm, quick_start_name};
use crate::config::EnvConfig;
use crate::herdr::{HerdrClient, Method};
use crate::ids::{PaneId, WorkspaceId};
use crate::pane_env;
use crate::util::slugify;
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::path::Path;
use std::process::Stdio;

pub(super) fn start_pi_agent(
    client: &HerdrClient,
    pane_id: &PaneId,
    workspace_id: &WorkspaceId,
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

    client
        .call(
            Method::AgentStart,
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
        client
            .call(
                Method::AgentPrompt,
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

fn herdr_agent_name(session_name: &str, workspace_id: &WorkspaceId) -> String {
    let session = slugify(session_name, 22);
    let workspace = slugify(workspace_id.as_str(), 6);
    format!("pi-{session}-{workspace}")
        .chars()
        .take(32)
        .collect()
}

pub(super) fn available_pi_models(cwd: &Path, env: &EnvConfig) -> Result<Vec<String>> {
    let output = pane_env::command(cwd, env)?
        .args(["pi", "--list-models"])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .context("failed to start environment launcher for Pi model discovery")?;
    if !output.status.success() {
        bail!(
            "Pi model discovery failed ({}) in {}; fix environment preparation and retry",
            output.status,
            cwd.display()
        );
    }

    let mut models = vec!["default".to_string()];
    let stdout = String::from_utf8(output.stdout).context("Pi returned non-UTF-8 model output")?;
    for line in stdout.lines().skip(1) {
        let mut parts = line.split_whitespace();
        if let (Some(provider), Some(model)) = (parts.next(), parts.next()) {
            models.push(format!("{provider}/{model}"));
        }
    }
    models.sort();
    models.dedup();
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_uses_target_directory_and_launcher_environment() {
        let cwd = std::env::temp_dir();
        let env = EnvConfig {
            launcher: Some(vec![
                "sh".into(),
                "-c".into(),
                "test \"$PWD\" -ef \"$1\" && printf 'provider model\\nfixture authenticated\\n'"
                    .into(),
                "test-launcher".into(),
                cwd.to_string_lossy().into_owned(),
            ]),
        };
        assert_eq!(
            available_pi_models(&cwd, &env).unwrap(),
            ["default", "fixture/authenticated"]
        );
    }

    #[test]
    fn discovery_failure_is_not_an_empty_model_list() {
        let env = EnvConfig {
            launcher: Some(vec!["false".into()]),
        };
        assert!(available_pi_models(Path::new("."), &env).is_err());
    }

    #[test]
    fn herdr_agent_names_are_strict_and_bounded() {
        let name = herdr_agent_name(
            "Feature/Very Long Branch With Spaces",
            &WorkspaceId::from("w123456789"),
        );
        assert!(name.len() <= 32);
        assert!(name.starts_with("pi-"));
        assert!(
            name.chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
        );
    }
}
