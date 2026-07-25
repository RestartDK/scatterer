use anyhow::{Context, Result};
use crossterm::{
    event::{KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io;

pub(crate) struct TerminalSession {
    keyboard_enhancements: bool,
    active: bool,
}

impl TerminalSession {
    pub(crate) fn enter(keyboard_enhancements: bool) -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        let mut session = Self {
            keyboard_enhancements,
            active: true,
        };
        let result = if keyboard_enhancements {
            execute!(
                io::stdout(),
                EnterAlternateScreen,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )
        } else {
            execute!(io::stdout(), EnterAlternateScreen)
        };
        if let Err(error) = result {
            let _ = session.finish();
            return Err(error).context("failed to enter alternate screen");
        }
        Ok(session)
    }

    pub(crate) fn finish(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;

        let leave_result = if self.keyboard_enhancements {
            execute!(
                io::stdout(),
                PopKeyboardEnhancementFlags,
                LeaveAlternateScreen
            )
        } else {
            execute!(io::stdout(), LeaveAlternateScreen)
        }
        .context("failed to leave alternate screen");
        let raw_result = disable_raw_mode().context("failed to disable raw mode");

        leave_result?;
        raw_result
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
