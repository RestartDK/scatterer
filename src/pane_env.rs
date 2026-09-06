use crate::config::EnvConfig;
use crate::util::shell_quote;
use anyhow::Result;
use std::path::Path;
use std::process::Command;

pub(crate) fn command(cwd: &Path, env: &EnvConfig) -> Result<Command> {
    let (program, args) = env.launcher()?;
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    Ok(command)
}

pub(crate) fn shell_command(command: &str, env: &EnvConfig) -> Result<Vec<String>> {
    let (program, args) = env.launcher()?;
    let launcher = std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ");
    let wrapped = format!(
        r#"{launcher} "${{SHELL:-/bin/sh}}" -c {}
result=$?
if [ "$result" -eq 0 ]; then exec "${{SHELL:-/bin/sh}}"; fi
if [ -n "${{PI_HERDR_READY_FILE:-}}" ]; then
  printf '%s\n' "$result" > "$PI_HERDR_READY_FILE"
  unset PI_HERDR_READY_FILE
fi
printf '\n[scatterer] command exited with status %s; press Enter to open a shell\n' "$result" >&2
read -r answer || exit "$result"
exec "${{SHELL:-/bin/sh}}"
"#,
        shell_quote(command),
    );
    Ok(vec!["sh".into(), "-c".into(), wrapped])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_failure_never_starts_the_requested_command() {
        let env = EnvConfig {
            launcher: Some(vec!["false".into()]),
        };
        let argv = shell_command("printf command-started", &env).unwrap();
        let output = Command::new(&argv[0])
            .args(&argv[1..])
            .env("SHELL", "/bin/sh")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("command-started"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("status 1"));
    }

    #[test]
    fn empty_launcher_is_rejected() {
        let env = EnvConfig {
            launcher: Some(vec![]),
        };
        assert!(shell_command("true", &env).is_err());
    }
}
