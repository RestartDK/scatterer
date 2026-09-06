use anyhow::{Context, Result, bail};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tempfile::TempDir;

pub(crate) struct ShellReady {
    directory: TempDir,
}

impl ShellReady {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            directory: tempfile::tempdir()?,
        })
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.directory.path().join("result")
    }

    pub(crate) fn wait(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            match std::fs::read_to_string(self.path()) {
                Ok(result) if result.trim() == "0" => return Ok(()),
                Ok(result) if !result.trim().is_empty() => bail!(
                    "environment preparation failed with status {}; inspect the agent pane; Pi was not started",
                    result.trim()
                ),
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("could not read shell preparation result"),
            }
            if Instant::now() >= deadline {
                bail!(
                    "environment preparation timed out; inspect the agent pane and repo-env shell integration; Pi was not started"
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_result_controls_readiness() {
        let ready = ShellReady::new().unwrap();
        std::fs::write(ready.path(), "17\n").unwrap();
        assert!(ready.wait().is_err());
        std::fs::write(ready.path(), "0\n").unwrap();
        assert!(ready.wait().is_ok());
    }
}
