use anyhow::{Result, anyhow};

mod config;
mod focus;
mod git;
mod herdr;
mod layout;
mod pane_env;
mod pr_picker;
mod quick_start;
mod util;
mod worktree_setup;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("apply-layout") | None => layout::apply_layout(),
        Some("open-quick-start") => quick_start::open(),
        Some("quick-start") => quick_start::run(),
        Some("open-pr-picker") => pr_picker::open(),
        Some("pr-picker") => pr_picker::run(),
        Some("focus-target") => focus::focus_target(args),
        Some(other) => Err(anyhow!(
            "unknown command '{other}'. Try: scatterer apply-layout | open-quick-start | quick-start | open-pr-picker | pr-picker"
        )),
    }
}
