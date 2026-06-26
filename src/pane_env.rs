use serde_json::{Value, json};

pub(crate) fn shell_command(command: &str, load_direnv: bool) -> Value {
    let direnv_prelude = if load_direnv {
        r#"scatterer_disable_direnv_for_shell=0
if command -v direnv >/dev/null 2>&1 && [ -f .envrc ]; then
  scatterer_real_direnv="$(command -v direnv || true)"
  scatterer_direnv_loaded=0
  scatterer_direnv_stderr="$(mktemp -t scatterer-direnv.XXXXXX 2>/dev/null || printf '%s' "${TMPDIR:-/tmp}/scatterer-direnv.$$.$RANDOM")"
  for scatterer_direnv_attempt in 1 2 3 4 5; do
    if direnv_export="$(direnv export bash 2>"$scatterer_direnv_stderr")"; then
      cat "$scatterer_direnv_stderr" >&2
      eval "$direnv_export"
      scatterer_direnv_loaded=1
      break
    fi

    if grep -q 'lorri has not completed an evaluation' "$scatterer_direnv_stderr" && [ "$scatterer_direnv_attempt" != "5" ]; then
      printf '[scatterer] waiting for lorri direnv evaluation (%s/5)...\n' "$scatterer_direnv_attempt" >&2
      sleep 3
      continue
    fi

    cat "$scatterer_direnv_stderr" >&2
    break
  done
  rm -f "$scatterer_direnv_stderr"

  if [ "$scatterer_direnv_loaded" != "1" ]; then
    scatterer_disable_direnv_for_shell=1
    printf '[scatterer] direnv export failed; continuing without direnv env\n' >&2
  fi
  unset direnv_export scatterer_direnv_attempt scatterer_direnv_loaded scatterer_direnv_stderr
fi
"#
    } else {
        ""
    };

    let wrapped = format!(
        r#"{direnv_prelude}{command}
status=$?
if [ "${{scatterer_disable_direnv_for_shell:-0}}" = "1" ]; then
  scatterer_direnv_stub_dir="${{XDG_CACHE_HOME:-$HOME/.cache}}/scatterer/direnv-disabled-bin"
  if mkdir -p "$scatterer_direnv_stub_dir" && [ -d "$scatterer_direnv_stub_dir" ]; then
    cat > "$scatterer_direnv_stub_dir/direnv" <<'SCATTERER_DIRENV_STUB'
#!/usr/bin/env sh
case "${{1:-}}" in
  hook|export) exit 0 ;;
esac
if [ -n "${{SCATTERER_REAL_DIRENV:-}}" ] && [ -x "$SCATTERER_REAL_DIRENV" ]; then
  exec "$SCATTERER_REAL_DIRENV" "$@"
fi
printf 'scatterer: direnv is disabled in this fallback shell because initial direnv setup failed.\n' >&2
exit 1
SCATTERER_DIRENV_STUB
    chmod 700 "$scatterer_direnv_stub_dir/direnv"
    export SCATTERER_REAL_DIRENV="${{scatterer_real_direnv:-}}"
    export PATH="$scatterer_direnv_stub_dir:$PATH"
    printf '\n[scatterer] direnv disabled for fallback shell to avoid repeating the failed .envrc load.\n' >&2
  fi
fi
printf '\n[scatterer] command exited with status %s; starting shell...\n' "$status"
exec "${{SHELL:-sh}}"
"#
    );
    json!(["bash", "-lc", wrapped])
}
