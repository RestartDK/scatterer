use super::{QuickStartForm, quick_start_name};
use crate::util::shell_quote;
use std::process::{Command, Stdio};

pub(super) fn pi_prompt_command(form: &QuickStartForm) -> String {
    let name = quick_start_name(&form.prompt);
    let mut command = format!(
        "if command -v pi >/dev/null 2>&1; then pi --name {}",
        shell_quote(&name),
    );
    if let Some(model) = &form.model {
        command.push_str(" --model ");
        command.push_str(&shell_quote(model));
    }
    command.push(' ');
    command.push_str(&shell_quote(&form.prompt));
    command.push_str("; else echo 'pi not found on PATH'; fi");
    command
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
