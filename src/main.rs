use anyhow::{Result, anyhow};

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
        Some("open-pr-picker") => pr_picker::open(),
        Some("pr-picker") => pr_picker::run(),
        Some("open-lazygit") => lazygit::open(),
        Some("lazygit") => lazygit::run(),
        Some("nav") => nav::run(args),
        Some("nav-left") => nav::run_direction("left"),
        Some("nav-down") => nav::run_direction("down"),
        Some("nav-up") => nav::run_direction("up"),
        Some("nav-right") => nav::run_direction("right"),
        Some("focus-target") => focus::focus_target(args),
        Some(other) => Err(anyhow!(
            "unknown command '{other}'. Try: scatterer apply-layout | appearance <sync|watch|install-launchd|uninstall-launchd> | open-quick-start | quick-start | open-pr-picker | pr-picker | open-lazygit | lazygit | nav <left|down|up|right>"
        )),
    }
}
