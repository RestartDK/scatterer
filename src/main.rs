use anyhow::{Result, anyhow};

mod agent_picker;
mod appearance;
mod config;
mod git;
mod herdr;
mod ids;
mod layout;
mod lazygit;
mod nav;
mod pane_env;
mod review;
mod space_status;
mod terminal_session;
mod theme;
mod util;

use nav::Direction;

/// Every Scatterer CLI subcommand. Parsing happens once at the boundary; the
/// dispatch below is an exhaustive match, so adding a variant forces both the
/// parser and the dispatcher to handle it.
#[derive(Debug)]
enum Command {
    ApplyLayout,
    Appearance(Vec<String>),
    RefreshSpaces,
    RefreshSpace,
    OpenAgentPicker,
    AgentPicker,
    OpenLazygit,
    Lazygit,
    ToggleReview,
    Review,
    Nav(Direction),
}

impl Command {
    const USAGE: &'static str = "scatterer apply-layout | appearance <sync|watch|install-launchd|uninstall-launchd> | refresh-spaces | refresh-space | open-agent-picker | agent-picker | open-lazygit | lazygit | toggle-review | review | nav <left|down|up|right>";

    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let Some(name) = args.next() else {
            return Ok(Self::ApplyLayout);
        };
        let command = match name.as_str() {
            "apply-layout" => Self::ApplyLayout,
            "appearance" => Self::Appearance(args.by_ref().collect()),
            "appearance-sync" => Self::Appearance(vec!["sync".to_string()]),
            "appearance-watch" => Self::Appearance(vec!["watch".to_string()]),
            "refresh-spaces" => Self::RefreshSpaces,
            "refresh-space" => Self::RefreshSpace,
            "open-agent-picker" => Self::OpenAgentPicker,
            "agent-picker" => Self::AgentPicker,
            "open-lazygit" => Self::OpenLazygit,
            "lazygit" => Self::Lazygit,
            "toggle-review" => Self::ToggleReview,
            "review" => Self::Review,
            "nav" => {
                let direction = args
                    .next()
                    .ok_or_else(|| {
                        anyhow!("nav missing direction: expected left, down, up, or right")
                    })?
                    .parse()?;
                Self::Nav(direction)
            }
            "nav-left" => Self::Nav(Direction::Left),
            "nav-down" => Self::Nav(Direction::Down),
            "nav-up" => Self::Nav(Direction::Up),
            "nav-right" => Self::Nav(Direction::Right),
            other => {
                return Err(anyhow!("unknown command '{other}'. Try: {}", Self::USAGE));
            }
        };
        if let Some(extra) = args.next() {
            return Err(anyhow!("unexpected argument '{extra}'"));
        }
        Ok(command)
    }
}

fn main() -> Result<()> {
    match Command::parse(std::env::args().skip(1))? {
        Command::ApplyLayout => layout::apply_layout(),
        Command::Appearance(args) => appearance::run(args.into_iter()),
        Command::RefreshSpaces => space_status::refresh_spaces(),
        Command::RefreshSpace => space_status::refresh_space(),
        Command::OpenAgentPicker => agent_picker::open(),
        Command::AgentPicker => agent_picker::run(),
        Command::OpenLazygit => lazygit::open(),
        Command::Lazygit => lazygit::run(),
        Command::ToggleReview => review::toggle(),
        Command::Review => review::run(),
        Command::Nav(direction) => nav::run(direction),
    }
}
