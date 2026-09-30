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

impl Merge for ProjectConfig {
    fn merge(&mut self, next: ProjectConfig) {
        self.layout.merge(next.layout);
        self.env.merge(next.env);
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
