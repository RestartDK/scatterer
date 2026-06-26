use serde_json::{Value, json};

pub(crate) fn shell_command(command: &str, load_direnv: bool) -> Value {
    let direnv_prelude = if load_direnv {
        r#"if command -v direnv >/dev/null 2>&1 && [ -f .envrc ]; then
  if direnv_export="$(direnv export bash)"; then
    eval "$direnv_export"
  else
    printf '[scatterer] direnv export failed; continuing without direnv env\n' >&2
  fi
fi
"#
    } else {
        ""
    };

    let wrapped = format!(
        r#"{direnv_prelude}{command}
status=$?
printf '\n[scatterer] command exited with status %s; starting shell...\n' "$status"
exec "${{SHELL:-sh}}"
"#
    );
    json!(["bash", "-lc", wrapped])
}
