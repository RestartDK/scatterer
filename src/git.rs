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
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRepo(PathBuf);

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temp_repo() -> TempRepo {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
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
}
