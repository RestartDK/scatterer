use crate::util::{first_string, non_empty_env, string_at};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::env;
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub(crate) struct InvocationSource {
    pub(crate) cwd: PathBuf,
}

pub(crate) fn herdr_socket_path() -> Result<PathBuf> {
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

pub(crate) fn resolve_invocation_source(socket_path: &Path) -> Result<InvocationSource> {
    if let Some(cwd) = non_empty_env("SCATTERER_SOURCE_CWD") {
        return Ok(InvocationSource {
            cwd: PathBuf::from(cwd),
        });
    }

    let context = env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());

    let pane_id =
        non_empty_env("HERDR_PANE_ID").or_else(|| context.as_ref().and_then(pane_id_from_context));
    let mut cwd = context
        .as_ref()
        .and_then(cwd_from_context)
        .map(PathBuf::from);

    if cwd.is_none() {
        let mut params = serde_json::Map::new();
        if let Some(pane_id) = &pane_id {
            params.insert("caller_pane_id".to_string(), json!(pane_id));
        }

        if let Ok(result) = socket_call(socket_path, "pane.current", Value::Object(params)) {
            let pane = result.get("pane").unwrap_or(&result);
            cwd = string_at(pane, &["foreground_cwd"])
                .or_else(|| string_at(pane, &["cwd"]))
                .map(PathBuf::from);
        }
    }

    let cwd = cwd.unwrap_or(env::current_dir().context("failed to resolve fallback cwd")?);

    Ok(InvocationSource { cwd })
}

pub(crate) fn socket_call(socket_path: &Path, method: &str, params: Value) -> Result<Value> {
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
            "method": method,
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

fn pane_id_from_context(context: &Value) -> Option<String> {
    first_string(
        context,
        &[
            &["pane_id"],
            &["focused_pane", "pane_id"],
            &["pane", "pane_id"],
        ],
    )
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

fn request_id(method: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("scatterer-{method}-{millis}")
}
