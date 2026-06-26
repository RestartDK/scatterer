use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProjectConfig {
    #[serde(default)]
    pub(crate) layout: LayoutConfig,
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

pub(crate) fn load_project_config(cwd: &Path) -> Result<(ProjectConfig, Option<PathBuf>)> {
    let Some(path) = find_project_config(cwd) else {
        return Ok((ProjectConfig::default(), None));
    };

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let config = toml::from_str::<ProjectConfig>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok((config, Some(path)))
}

fn find_project_config(cwd: &Path) -> Option<PathBuf> {
    for dir in cwd.ancestors() {
        for name in [".scatterer.toml", "scatterer.toml"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
