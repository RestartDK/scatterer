use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) fn git_branch(cwd: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["branch", "--show-current"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

pub(crate) fn switch_or_create_branch(cwd: &Path, branch: &str) -> Result<()> {
    let branch = branch.trim();
    if branch.is_empty() {
        return Ok(());
    }
    if git_branch(cwd).as_deref() == Some(branch) {
        return Ok(());
    }

    let valid_branch = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["check-ref-format", "--branch", branch])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to validate git branch '{branch}'"))?;
    if !valid_branch.success() {
        return Err(anyhow!("'{branch}' is not a valid git branch name"));
    }

    let ref_name = format!("refs/heads/{branch}");
    let branch_exists = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["show-ref", "--verify", "--quiet", ref_name.as_str()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    let mut command = Command::new("git");
    command.arg("-C").arg(cwd);
    if branch_exists {
        command.args(["switch", branch]);
    } else {
        command.args(["switch", "-c", branch]);
    }

    let status = command
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("failed to switch to git branch '{branch}'"))?;
    if !status.success() {
        return Err(anyhow!(
            "git switch for branch '{branch}' failed with status {status}"
        ));
    }

    Ok(())
}

pub(crate) fn git_root(cwd: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}
