use anyhow::{Result, anyhow};

mod agent_picker;
mod appearance;
mod config;
mod focus;
mod git;
mod herdr;
mod layout;
mod lazygit;
mod nav;
mod pane_env;
mod pr_picker;
mod quick_start;
mod review;
mod terminal_session;
mod theme;
mod util;
mod worktree_setup;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("apply-layout") | None => layout::apply_layout(),
        Some("appearance") => appearance::run(args),
        Some("appearance-sync") => appearance::run(["sync".to_string()].into_iter()),
        Some("appearance-watch") => appearance::run(["watch".to_string()].into_iter()),
        Some("open-quick-start") => quick_start::open(),
        Some("quick-start") => quick_start::run(),
        Some("remove-flat-worktree") => quick_start::remove_flat_worktree(),
        Some("open-pr-picker") => pr_picker::open(),
        Some("pr-picker") => pr_picker::run(),
        Some("open-agent-picker") => agent_picker::open(),
        Some("agent-picker") => agent_picker::run(),
        Some("open-lazygit") => lazygit::open(),
        Some("lazygit") => lazygit::run(),
        Some("toggle-review") => review::toggle(),
        Some("review") => review::run(),
        Some("nav") => nav::run(args),
        Some("nav-left") => nav::run_direction("left"),
        Some("nav-down") => nav::run_direction("down"),
        Some("nav-up") => nav::run_direction("up"),
        Some("nav-right") => nav::run_direction("right"),
        Some(other) => Err(anyhow!(
            "unknown command '{other}'. Try: scatterer apply-layout | appearance <sync|watch|install-launchd|uninstall-launchd> | open-quick-start | quick-start | remove-flat-worktree | open-pr-picker | pr-picker | open-agent-picker | agent-picker | open-lazygit | lazygit | toggle-review | review | nav <left|down|up|right>"
        )),
    }
}
