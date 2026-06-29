use crate::herdr::{herdr_socket_path, socket_call};
use crate::util::string_at;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const LAUNCHD_LABEL: &str = "dev.herdr.scatterer.appearance-sync";
const DEFAULT_POLL_SECONDS: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorScheme {
    Dark,
    Light,
}

impl ColorScheme {
    fn label(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    fn terminal_report(self) -> &'static str {
        match self {
            // CSI ? 997 ; 1 n = dark, CSI ? 997 ; 2 n = light.
            // Pi understands this color-scheme notification, but Herdr does not
            // currently forward the parent terminal's notifications to child
            // panes. This is a temporary bridge; when there is time, upstream
            // Herdr should get a PR that proxies CSI ? 996 n / CSI ? 2031 h/l
            // and broadcasts these reports to subscribed child panes directly.
            Self::Dark => "\x1b[?997;1n",
            Self::Light => "\x1b[?997;2n",
        }
    }
}

pub(crate) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    match args.next().as_deref() {
        Some("sync") | None => sync_once(),
        Some("watch") => watch(),
        Some("install") | Some("install-launchd") => install_launchd(),
        Some("uninstall") | Some("uninstall-launchd") => uninstall_launchd(),
        Some(other) => Err(anyhow!(
            "unknown appearance command '{other}'. Try: scatterer appearance sync | watch | install-launchd | uninstall-launchd"
        )),
    }
}

fn sync_once() -> Result<()> {
    ensure_macos()?;
    let scheme = detect_macos_color_scheme()?;
    let notified = notify_pi_panes(scheme)?;
    println!(
        "scatterer: synced macOS {} appearance to {notified} Pi pane(s)",
        scheme.label()
    );
    Ok(())
}

fn watch() -> Result<()> {
    ensure_macos()?;
    let poll_interval = appearance_poll_interval();
    eprintln!(
        "scatterer: watching macOS appearance every {}s",
        poll_interval.as_secs()
    );

    let mut last_scheme = None;
    loop {
        match detect_macos_color_scheme() {
            Ok(scheme) if last_scheme != Some(scheme) => {
                match notify_pi_panes(scheme) {
                    Ok(notified) => eprintln!(
                        "scatterer: synced macOS {} appearance to {notified} Pi pane(s)",
                        scheme.label()
                    ),
                    Err(error) => eprintln!("scatterer: appearance sync failed: {error:#}"),
                }
                last_scheme = Some(scheme);
            }
            Ok(_) => {}
            Err(error) => eprintln!("scatterer: failed to detect macOS appearance: {error:#}"),
        }
        thread::sleep(poll_interval);
    }
}

fn install_launchd() -> Result<()> {
    ensure_macos()?;
    let plist_path = launchd_plist_path()?;
    let log_dir = home_dir()?.join("Library").join("Logs");
    fs::create_dir_all(
        plist_path
            .parent()
            .context("LaunchAgents path has no parent")?,
    )
    .context("failed to create LaunchAgents directory")?;
    fs::create_dir_all(&log_dir).context("failed to create ~/Library/Logs")?;

    let executable =
        env::current_exe().context("failed to resolve current scatterer executable")?;
    let plist = launchd_plist(&executable, &log_dir)?;
    fs::write(&plist_path, plist)
        .with_context(|| format!("failed to write {}", plist_path.display()))?;

    let domain = launchd_domain();
    let _ = run_launchctl(["bootout", domain.as_str(), path_str(&plist_path)?]);
    run_launchctl(["bootstrap", domain.as_str(), path_str(&plist_path)?])?;
    let service = format!("{domain}/{LAUNCHD_LABEL}");
    let _ = run_launchctl(["enable", service.as_str()]);
    let _ = run_launchctl(["kickstart", "-k", service.as_str()]);

    println!("scatterer: installed {LAUNCHD_LABEL}");
    println!("scatterer: plist {}", plist_path.display());
    println!(
        "scatterer: logs {}",
        log_dir.join(format!("{LAUNCHD_LABEL}.out.log")).display()
    );
    Ok(())
}

fn uninstall_launchd() -> Result<()> {
    ensure_macos()?;
    let plist_path = launchd_plist_path()?;
    let domain = launchd_domain();
    let _ = run_launchctl(["bootout", domain.as_str(), path_str(&plist_path)?]);
    if plist_path.exists() {
        fs::remove_file(&plist_path)
            .with_context(|| format!("failed to remove {}", plist_path.display()))?;
    }
    println!("scatterer: uninstalled {LAUNCHD_LABEL}");
    Ok(())
}

fn notify_pi_panes(scheme: ColorScheme) -> Result<usize> {
    let socket_path = herdr_socket_path()?;
    let result =
        socket_call(&socket_path, "pane.list", json!({})).context("failed to list Herdr panes")?;
    let panes = result
        .get("panes")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("pane.list response did not include panes: {result}"))?;

    let mut notified = 0usize;
    for pane in panes {
        let Some(pane_id) = string_at(pane, &["pane_id"]) else {
            continue;
        };
        if string_at(pane, &["agent"]).as_deref() != Some("pi") {
            continue;
        }
        if !pane_foreground_is_pi(&socket_path, &pane_id).unwrap_or(false) {
            continue;
        }
        socket_call(
            &socket_path,
            "pane.send_text",
            json!({
                "pane_id": pane_id,
                "text": scheme.terminal_report(),
            }),
        )
        .with_context(|| format!("failed to send appearance event to pane {pane_id}"))?;
        notified += 1;
    }

    Ok(notified)
}

fn pane_foreground_is_pi(socket_path: &Path, pane_id: &str) -> Result<bool> {
    let result = socket_call(
        socket_path,
        "pane.process_info",
        json!({ "pane_id": pane_id }),
    )?;
    let process_info = result.get("process_info").unwrap_or(&result);
    let processes = process_info
        .get("foreground_processes")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            anyhow!("pane.process_info response did not include foreground_processes")
        })?;

    Ok(processes.iter().any(process_looks_like_pi))
}

fn process_looks_like_pi(process: &Value) -> bool {
    ["argv0", "name"].into_iter().any(|field| {
        string_at(process, &[field])
            .map(|name| process_basename(&name) == "pi")
            .unwrap_or(false)
    })
}

fn process_basename(name: &str) -> String {
    Path::new(name)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .unwrap_or(name)
        .to_string()
}

fn detect_macos_color_scheme() -> Result<ColorScheme> {
    ensure_macos()?;
    let output = Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .stdin(Stdio::null())
        .output()
        .context("failed to run defaults read -g AppleInterfaceStyle")?;

    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout);
        if value.trim().eq_ignore_ascii_case("dark") {
            return Ok(ColorScheme::Dark);
        }
    }

    Ok(ColorScheme::Light)
}

fn ensure_macos() -> Result<()> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        bail!("Scatterer appearance sync is currently macOS-only")
    }
}

fn appearance_poll_interval() -> Duration {
    let seconds = env::var("SCATTERER_APPEARANCE_POLL_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_POLL_SECONDS);
    Duration::from_secs(seconds)
}

fn launchd_plist_path() -> Result<PathBuf> {
    Ok(home_dir()?
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCHD_LABEL}.plist")))
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}

fn launchd_plist(executable: &Path, log_dir: &Path) -> Result<String> {
    let out_log = log_dir.join(format!("{LAUNCHD_LABEL}.out.log"));
    let err_log = log_dir.join(format!("{LAUNCHD_LABEL}.err.log"));
    let home = home_dir()?;
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{executable}</string>
    <string>appearance</string>
    <string>watch</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{out_log}</string>
  <key>StandardErrorPath</key>
  <string>{err_log}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key>
    <string>{home}</string>
  </dict>
</dict>
</plist>
"#,
        label = LAUNCHD_LABEL,
        executable = escape_xml(&executable.to_string_lossy()),
        out_log = escape_xml(&out_log.to_string_lossy()),
        err_log = escape_xml(&err_log.to_string_lossy()),
        home = escape_xml(&home.to_string_lossy()),
    ))
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn launchd_domain() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}

fn run_launchctl<'a>(args: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let args = args.into_iter().collect::<Vec<_>>();
    let output = Command::new("/bin/launchctl")
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to run launchctl {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!(
            "launchctl {} failed with status {}{}",
            args.join(" "),
            output.status,
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        ));
    }
    Ok(())
}

fn path_str(path: &Path) -> Result<&str> {
    path.as_os_str()
        .to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_scheme_reports_match_pi_parser() {
        assert_eq!(ColorScheme::Dark.terminal_report(), "\x1b[?997;1n");
        assert_eq!(ColorScheme::Light.terminal_report(), "\x1b[?997;2n");
    }

    #[test]
    fn pi_process_matches_argv0() {
        let process = json!({ "name": "node", "argv0": "pi" });
        assert!(process_looks_like_pi(&process));
    }
}
