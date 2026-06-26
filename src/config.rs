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
    /// Command for the single runner pane in tab 2.
    pub(crate) runner: Option<String>,
    /// Command for the single git pane in tab 3.
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
    /// Shell commands to run in each newly created worktree before the layout is
    /// applied. Commands are merged in config discovery order, so local config
    /// can add personal setup without replacing project defaults.
    #[serde(default)]
    pub(crate) commands: Vec<String>,
}

impl ProjectConfig {
    fn merge(&mut self, next: ProjectConfig) {
        self.layout.merge(next.layout);
        self.env.merge(next.env);
        self.quick_start.merge(next.quick_start);
    }
}

impl LayoutConfig {
    fn merge(&mut self, next: LayoutConfig) {
        if next.agent.is_some() {
            self.agent = next.agent;
        }
        if next.hunk.is_some() {
            self.hunk = next.hunk;
        }
        if next.runner.is_some() {
            self.runner = next.runner;
        }
        if next.git.is_some() {
            self.git = next.git;
        }
    }
}

impl EnvConfig {
    fn merge(&mut self, next: EnvConfig) {
        if next.direnv.is_some() {
            self.direnv = next.direnv;
        }
    }
}

impl QuickStartConfig {
    fn merge(&mut self, next: QuickStartConfig) {
        self.setup.merge(next.setup);
    }
}

impl QuickStartSetupConfig {
    fn merge(&mut self, mut next: QuickStartSetupConfig) {
        self.commands.append(&mut next.commands);
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
