use crate::git::{git_branch, git_root};
use crate::util::command_exists;
use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
enum SetupCommand {
    Shell { label: String, command: String },
    Argv { label: String, argv: Vec<String> },
}

pub(super) fn run_worktree_setup(source_cwd: &Path, worktree_path: &Path) -> Result<()> {
    let source_root = git_root(source_cwd).unwrap_or_else(|| source_cwd.to_path_buf());
    let worktree_root = git_root(worktree_path).unwrap_or_else(|| worktree_path.to_path_buf());
    let setup_root = if worktree_root.join(".herdr").exists() {
        worktree_root
    } else {
        source_root.clone()
    };

    let setup_path = setup_root.join(".herdr").join("setup.json");
    let setup_commands = load_setup_commands(&setup_path)?;
    let script_hooks = load_script_hooks(&setup_root);
    if setup_commands.is_empty() && script_hooks.is_empty() {
        return Ok(());
    }

    eprintln!(
        "scatterer: running worktree setup for {}",
        worktree_path.display()
    );
    for (index, command) in setup_commands.iter().enumerate() {
        match command {
            SetupCommand::Shell { label, command } => {
                eprintln!(
                    "scatterer: setup [{}/{}] {label}",
                    index + 1,
                    setup_commands.len()
                );
                let status = setup_base_command(
                    shell_program(),
                    &source_root,
                    Some(&setup_path),
                    worktree_path,
                )
                .arg("-lc")
                .arg(command)
                .status()
                .with_context(|| format!("failed to run setup command '{label}'"))?;
                if !status.success() {
                    return Err(anyhow!(
                        "setup command '{label}' failed with status {}",
                        status
                    ));
                }
            }
            SetupCommand::Argv { label, argv } => {
                if argv.is_empty() {
                    continue;
                }
                eprintln!(
                    "scatterer: setup [{}/{}] {label}",
                    index + 1,
                    setup_commands.len()
                );
                let status =
                    setup_base_command(&argv[0], &source_root, Some(&setup_path), worktree_path)
                        .args(&argv[1..])
                        .status()
                        .with_context(|| format!("failed to run setup command '{label}'"))?;
                if !status.success() {
                    return Err(anyhow!(
                        "setup command '{label}' failed with status {}",
                        status
                    ));
                }
            }
        }
    }

    let setup_file_env = setup_path.is_file().then_some(setup_path.as_path());
    for hook in script_hooks {
        eprintln!("scatterer: setup hook {}", hook.display());
        let status = setup_base_command(&hook, &source_root, setup_file_env, worktree_path)
            .status()
            .with_context(|| format!("failed to run setup hook {}", hook.display()))?;
        if !status.success() {
            return Err(anyhow!(
                "setup hook {} failed with status {}",
                hook.display(),
                status
            ));
        }
    }

    Ok(())
}

fn load_setup_commands(setup_path: &Path) -> Result<Vec<SetupCommand>> {
    if !setup_path.is_file() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(setup_path)
        .with_context(|| format!("failed to read {}", setup_path.display()))?;
    let payload: Value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", setup_path.display()))?;
    let Some(commands) = payload.get("commands") else {
        return Ok(Vec::new());
    };
    if commands.is_null() {
        return Ok(Vec::new());
    }
    let commands = commands
        .as_array()
        .ok_or_else(|| anyhow!("{}.commands must be an array", setup_path.display()))?;

    commands
        .iter()
        .enumerate()
        .map(|(index, entry)| normalize_setup_command(entry, index))
        .collect()
}

fn normalize_setup_command(entry: &Value, index: usize) -> Result<SetupCommand> {
    if let Some(command) = entry.as_str() {
        return Ok(SetupCommand::Shell {
            label: command.to_string(),
            command: command.to_string(),
        });
    }

    if let Some(array) = entry.as_array() {
        let argv = array
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("setup command argv entries must be strings"))
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(SetupCommand::Argv {
            label: argv.join(" "),
            argv,
        });
    }

    if let Some(object) = entry.as_object() {
        let label = object
            .get("name")
            .or_else(|| object.get("description"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("command {index}"));

        if let Some(command) = object.get("run").and_then(Value::as_str) {
            return Ok(SetupCommand::Shell {
                label,
                command: command.to_string(),
            });
        }

        if let Some(array) = object.get("argv").and_then(Value::as_array) {
            let argv = array
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| anyhow!("setup command argv entries must be strings"))
                })
                .collect::<Result<Vec<_>>>()?;
            return Ok(SetupCommand::Argv { label, argv });
        }
    }

    Err(anyhow!(
        "setup commands[{index}] must be a string, string array, or object with run/argv"
    ))
}

fn load_script_hooks(setup_root: &Path) -> Vec<PathBuf> {
    ["setup-worktree.sh", "post-worktree-create.sh"]
        .into_iter()
        .map(|name| setup_root.join(".herdr").join(name))
        .filter(|path| is_executable_file(path))
        .collect()
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn setup_base_command<S: AsRef<std::ffi::OsStr>>(
    program: S,
    source_root: &Path,
    setup_path: Option<&Path>,
    worktree_path: &Path,
) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(worktree_path)
        .env("HERDR_SOURCE_ROOT", source_root)
        .env(
            "HERDR_SETUP_FILE",
            setup_path
                .map(|path| path.as_os_str())
                .unwrap_or_else(|| std::ffi::OsStr::new("")),
        )
        .env("HERDR_WORKTREE_PATH", worktree_path)
        .env(
            "HERDR_WORKTREE_BRANCH",
            git_branch(worktree_path).unwrap_or_default(),
        );
    command
}

fn shell_program() -> &'static str {
    if command_exists("bash") { "bash" } else { "sh" }
}
