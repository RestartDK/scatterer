use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProjectConfig {
    #[serde(default)]
    pub(crate) layout: LayoutConfig,
    #[serde(default)]
    pub(crate) env: EnvConfig,
    #[serde(default)]
    pub(crate) quick_start: QuickStartConfig,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct LayoutConfig {
    /// Command for the agent pane in tab 1.
    pub(crate) agent: Option<String>,
    /// Command for the hunk pane in tab 1.
    #[serde(alias = "diff")]
    pub(crate) hunk: Option<String>,
    /// Optional command for a project-specific runner tab.
    pub(crate) runner: Option<String>,
    /// Optional command for a project-specific git tab.
    pub(crate) git: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct EnvConfig {
    /// Whether Scatterer-created panes should import `direnv export bash` before
    /// launching their command. Defaults to true.
    pub(crate) direnv: Option<bool>,
}

impl EnvConfig {
    pub(crate) fn direnv_enabled(&self) -> bool {
        self.direnv.unwrap_or(true)
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct QuickStartConfig {
    #[serde(default)]
    pub(crate) setup: QuickStartSetupConfig,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct QuickStartSetupConfig {
    /// Shell commands to run in each newly created quick-start worktree before
    /// the layout is applied. Commands are merged in config discovery order, so
    /// local config can add personal setup without replacing project defaults.
    #[serde(default)]
    pub(crate) commands: Vec<String>,
}

/// Layered config merging: later (more local) config wins field by field.
trait Merge {
    fn merge(&mut self, next: Self);
}

/// A later `Some` overrides; a later `None` keeps the earlier value.
impl<T> Merge for Option<T> {
    fn merge(&mut self, next: Self) {
        if next.is_some() {
            *self = next;
        }
    }
}

/// Lists accumulate across config layers instead of replacing each other.
impl<T> Merge for Vec<T> {
    fn merge(&mut self, mut next: Self) {
        self.append(&mut next);
    }
}

impl Merge for ProjectConfig {
    fn merge(&mut self, next: ProjectConfig) {
        self.layout.merge(next.layout);
        self.env.merge(next.env);
        self.quick_start.merge(next.quick_start);
    }
}

impl Merge for LayoutConfig {
    fn merge(&mut self, next: LayoutConfig) {
        self.agent.merge(next.agent);
        self.hunk.merge(next.hunk);
        self.runner.merge(next.runner);
        self.git.merge(next.git);
    }
}

impl Merge for EnvConfig {
    fn merge(&mut self, next: EnvConfig) {
        self.direnv.merge(next.direnv);
    }
}

impl Merge for QuickStartConfig {
    fn merge(&mut self, next: QuickStartConfig) {
        self.setup.merge(next.setup);
    }
}

impl Merge for QuickStartSetupConfig {
    fn merge(&mut self, next: QuickStartSetupConfig) {
        self.commands.merge(next.commands);
    }
}

pub(crate) fn load_project_config(cwd: &Path) -> Result<(ProjectConfig, Vec<PathBuf>)> {
    let mut config = ProjectConfig::default();
    let mut loaded_paths = Vec::new();

    for path in find_project_config_paths(cwd) {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let next = toml::from_str::<ProjectConfig>(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        config.merge(next);
        loaded_paths.push(path);
    }

    Ok((config, loaded_paths))
}

fn find_project_config_paths(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = cwd.ancestors().collect::<Vec<_>>();
    dirs.reverse();

    let mut paths = Vec::new();
    for dir in dirs {
        // Load generic names first, then dotfile, then personal local overrides.
        // This preserves compatibility with `scatterer.toml` while making
        // `.scatterer.local.toml` the final override layer.
        for name in ["scatterer.toml", ".scatterer.toml", ".scatterer.local.toml"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                paths.push(candidate);
            }
        }
    }
    paths
}
