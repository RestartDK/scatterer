use std::path::Path;
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
    use std::path::PathBuf;
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
    fn git_parent_branch_uses_remembered_parent() {
        let repo = temp_repo();
        git(&repo.0, &["init", "-b", "main"]);
        git(&repo.0, &["config", "user.email", "scatterer@example.test"]);
        git(&repo.0, &["config", "user.name", "Scatterer Test"]);

        fs::write(repo.0.join("file.txt"), "main\n").expect("write main file");
        git(&repo.0, &["add", "."]);
        git(&repo.0, &["commit", "-m", "initial"]);
        git(&repo.0, &["switch", "-c", "feature/child"]);

        git(
            &repo.0,
            &[
                "config",
                "--local",
                "branch.feature/child.scatterer-parent",
                "feature/parent",
            ],
        );

        assert_eq!(
            git_parent_branch(&repo.0).as_deref(),
            Some("feature/parent")
        );
    }
}
