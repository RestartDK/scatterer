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
    non_empty_stdout(&output.stdout)
}

pub(crate) fn git_parent_branch(cwd: &Path) -> Option<String> {
    let branch = git_branch(cwd)?;
    configured_parent_branch(cwd, &branch).or_else(|| github_pr_base_branch(cwd))
}

pub(crate) fn remember_parent_branch(cwd: &Path, branch: &str, parent: &str) -> Result<()> {
    let branch = branch.trim();
    let parent = parent.trim();
    if branch.is_empty() || parent.is_empty() {
        return Ok(());
    }

    let key = format!("branch.{branch}.scatterer-parent");
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["config", "--local", key.as_str(), parent])
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("failed to remember parent branch '{parent}' for '{branch}'"))?;
    if !status.success() {
        return Err(anyhow!(
            "git config failed while remembering parent branch '{parent}' for '{branch}'"
        ));
    }

    Ok(())
}

pub(crate) fn switch_or_create_branch(cwd: &Path, branch: &str, base: Option<&str>) -> Result<()> {
    let branch = branch.trim();
    let base = base.map(str::trim).filter(|base| !base.is_empty());
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
        if let Some(base) = base {
            command.arg(base);
        }
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
    non_empty_stdout(&output.stdout).map(PathBuf::from)
}

fn configured_parent_branch(cwd: &Path, branch: &str) -> Option<String> {
    let key = format!("branch.{branch}.scatterer-parent");
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["config", "--get", key.as_str()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    non_empty_stdout(&output.stdout)
}

fn github_pr_base_branch(cwd: &Path) -> Option<String> {
    let output = Command::new("gh")
        .current_dir(cwd)
        .args([
            "pr",
            "view",
            "--json",
            "baseRefName",
            "--jq",
            ".baseRefName",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    non_empty_stdout(&output.stdout)
}

fn non_empty_stdout(stdout: &[u8]) -> Option<String> {
    let value = String::from_utf8_lossy(stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TempRepo(PathBuf);

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temp_repo() -> TempRepo {
        static NEXT_TEMP_REPO: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT_TEMP_REPO.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "scatterer-git-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp repo directory");
        TempRepo(path)
    }

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn switch_or_create_branch_uses_base_for_new_branch() {
        let repo = temp_repo();
        git(&repo.0, &["init", "-b", "main"]);
        git(&repo.0, &["config", "user.email", "scatterer@example.test"]);
        git(&repo.0, &["config", "user.name", "Scatterer Test"]);

        fs::write(repo.0.join("file.txt"), "main\n").expect("write main file");
        git(&repo.0, &["add", "."]);
        git(&repo.0, &["commit", "-m", "initial"]);
        let main_rev = git(&repo.0, &["rev-parse", "HEAD"]);

        git(&repo.0, &["switch", "-c", "parent"]);
        fs::write(repo.0.join("file.txt"), "parent\n").expect("write parent file");
        git(&repo.0, &["commit", "-am", "parent"]);
        let parent_rev = git(&repo.0, &["rev-parse", "HEAD"]);

        git(&repo.0, &["switch", "main"]);
        switch_or_create_branch(&repo.0, "child", Some("parent")).expect("create child branch");

        assert_eq!(git_branch(&repo.0).as_deref(), Some("child"));
        assert_eq!(git(&repo.0, &["rev-parse", "HEAD"]), parent_rev);
        assert_ne!(git(&repo.0, &["rev-parse", "HEAD"]), main_rev);
    }

    #[test]
    fn git_parent_branch_uses_remembered_parent() {
        let repo = temp_repo();
        git(&repo.0, &["init", "-b", "main"]);
        git(&repo.0, &["config", "user.email", "scatterer@example.test"]);
        git(&repo.0, &["config", "user.name", "Scatterer Test"]);

        fs::write(repo.0.join("file.txt"), "main\n").expect("write main file");
        git(&repo.0, &["add", "."]);
        git(&repo.0, &["commit", "-m", "initial"]);
        git(&repo.0, &["switch", "-c", "feature/child"]);

        remember_parent_branch(&repo.0, "feature/child", "feature/parent")
            .expect("remember parent branch");

        assert_eq!(
            git_parent_branch(&repo.0).as_deref(),
            Some("feature/parent")
        );
    }
}
