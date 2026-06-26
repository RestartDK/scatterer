use crate::herdr::{herdr_socket_path, socket_call};
use crate::util::debug_log;
use anyhow::{Result, anyhow};
use serde_json::json;
use std::env;
use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

pub(crate) fn focus_workspace_later(workspace_id: &str) {
    spawn_focus_process(workspace_id, None);
}

pub(crate) fn focus_pane_later(workspace_id: &str, pane_id: &str) {
    spawn_focus_process(workspace_id, Some(pane_id));
}

pub(crate) fn focus_target(mut args: impl Iterator<Item = String>) -> Result<()> {
    debug_log("focus_target start");
    let socket_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("focus-target missing socket path"))?;
    let workspace_id = args
        .next()
        .ok_or_else(|| anyhow!("focus-target missing workspace id"))?;
    let pane_id = args.next();

    // Herdr overlay panes restore focus as they close. Retry via the socket API
    // from this detached helper so the final focus wins after restoration.
    debug_log(&format!(
        "focus_target socket={} workspace={} pane={:?}",
        socket_path.display(),
        workspace_id,
        pane_id
    ));
    for delay in [200_u64, 450, 800, 1200] {
        thread::sleep(Duration::from_millis(delay));
        match socket_call(
            &socket_path,
            "workspace.focus",
            json!({ "workspace_id": workspace_id }),
        ) {
            Ok(_) => debug_log("workspace.focus ok"),
            Err(err) => debug_log(&format!("workspace.focus err: {err}")),
        }
        if let Some(pane_id) = &pane_id {
            match socket_call(&socket_path, "agent.focus", json!({ "target": pane_id })) {
                Ok(_) => debug_log("agent.focus ok"),
                Err(err) => debug_log(&format!("agent.focus err: {err}")),
            }
        }
    }

    Ok(())
}

fn spawn_focus_process(workspace_id: &str, pane_id: Option<&str>) {
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let Ok(socket_path) = herdr_socket_path() else {
        return;
    };
    debug_log(&format!(
        "spawn_focus_process exe={} socket={} workspace={} pane={:?}",
        exe.display(),
        socket_path.display(),
        workspace_id,
        pane_id
    ));

    let mut command = Command::new(exe);
    command
        .arg("focus-target")
        .arg(socket_path)
        .arg(workspace_id);
    if let Some(pane_id) = pane_id {
        command.arg(pane_id);
    }

    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => debug_log("spawned focus-target helper"),
        Err(err) => debug_log(&format!("failed to spawn focus-target helper: {err}")),
    }
}
