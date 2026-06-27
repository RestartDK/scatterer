use super::{QuickStartForm, quick_start_name};
use crate::util::shell_quote;
use std::process::{Command, Stdio};

pub(super) fn pi_agent_command(form: &QuickStartForm, session_name: &str) -> Option<String> {
    let prompt = form.prompt.trim();
    if prompt.is_empty() && form.model.is_none() {
        return None;
    }

    let name = pi_session_name(form, session_name);
    let mut command = format!(
        "if command -v pi >/dev/null 2>&1; then pi --name {}",
        shell_quote(&name),
    );
    if let Some(model) = &form.model {
        command.push_str(" --model ");
        command.push_str(&shell_quote(model));
    }
    if !prompt.is_empty() {
        command.push(' ');
        command.push_str(&shell_quote(prompt));
    }
    command.push_str("; else echo 'pi not found on PATH'; fi");
    Some(command)
}

fn pi_session_name(form: &QuickStartForm, session_name: &str) -> String {
    let session_name = session_name.trim();
    if session_name.is_empty() {
        quick_start_name(&form.prompt)
    } else {
        session_name.to_string()
    }
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
