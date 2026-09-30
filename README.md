# Scatterer

Personal Herdr workflow plugin. Scatterer 0.3.1 requires Herdr 0.7.5 or newer.

## Default layout

The `daniel.scatterer.apply-layout` action creates a new Herdr workspace/space
from the currently focused pane's cwd, then uses Herdr's declarative
`layout.apply` socket API to make an `agent` tab with `pi` on the left and
`hunk diff <parent-branch>... --watch` on the right. The parent branch comes
from saved Scatterer branch metadata when present, then falls back to the
current GitHub PR base, then `main`.

Repeated invocations create another workspace/space. It does not append tabs to
the workspace you invoked it from. Project config can override the Hunk command
or add extra layout tabs such as a runner or git UI.

Hunk theming is best configured through Hunk itself. `theme = "auto"` queries the
terminal background and picks Hunk's light/dark defaults; `transparent_background
= true` lets the terminal background show through. Hunk also has a `tokyo-night`
dark theme, but not a matching Tokyo Night Day theme. Herdr itself supports
`name = "terminal"` or `dark_name = "tokyo-night"` / `light_name =
"tokyo-night-day"` in `~/.config/herdr/config.toml`.

## Agent picker theming

The agent picker reads Herdr's `[theme]` configuration, mirrors every built-in Herdr
palette, applies `[theme.custom]` overrides, and follows `auto_switch` on macOS.
Selection backgrounds, borders, text, muted labels, and semantic status colors
therefore match the active Herdr theme instead of using fixed ANSI colors.
External applications such as Hunk and lazygit continue to manage their own
themes.

## Layout execution

Layout pane commands import `direnv export bash` before starting so tools like
Pi, Hunk, and project runner commands inherit the workspace's allowed `.envrc`.
If lorri has not finished evaluating yet, Scatterer waits and retries briefly
before launching panes. If direnv still fails, Scatterer continues without that
environment and disables direnv hooks in the fallback shell. Set
`[env] direnv = false` in Scatterer config to disable direnv per project.

## Lazygit overlay

The `daniel.scatterer.lazygit` action opens `lazygit` in a Herdr overlay using
the focused pane's current working directory.

## Review pane

The `daniel.scatterer.review-toggle` action opens a `Review` right split running
`tuicr --working-tree` from the focused pane's current working directory.
Invoking it again from the same tab closes that review pane. Quitting the review
also closes its pane and restores the space to the remaining layout. Outside a
Git repository, Scatterer shows an in-app Herdr error toast without opening a
pane.

## macOS appearance sync

Scatterer includes a macOS-only temporary bridge for Pi panes while Herdr does
not yet forward host terminal light/dark color-scheme events to child panes.
Enable Herdr's own UI auto-switching in `~/.config/herdr/config.toml`:

```toml
[theme]
auto_switch = true
dark_name = "tokyo-night"
light_name = "tokyo-night-day"
```

Then run a one-shot sync, or install the LaunchAgent watcher:

```sh
scatterer appearance sync
scatterer appearance install-launchd
scatterer appearance uninstall-launchd
```

The watcher polls macOS `AppleInterfaceStyle` and sends Pi-compatible terminal
color-scheme reports (`CSI ? 997 ; 1/2 n`) into active Pi panes. This should be
removed once Herdr upstream can proxy child-pane color-scheme queries and
notifications (`CSI ? 996 n`, `CSI ? 2031 h/l`). Daniel should open that Herdr PR
when there is time.

## Vim/Herdr navigation

The `daniel.scatterer.nav-left`, `nav-down`, `nav-up`, and `nav-right` actions
provide the Herdr side of Vim-style pane navigation. Each action checks the
focused pane's foreground process with `herdr pane process-info` and, for wrapper
processes such as `sudo`/network-namespace shells, scans descendants too:

- if it is Vim/Neovim, Scatterer sends the matching `ctrl+h/j/k/l` key into that
  pane so the editor can move between its own splits
- if it is `ssh`/`mosh-client`, Scatterer also sends the key into the pane so a
  remote Neovim/Herdr session can handle it instead of local Herdr stealing it
- otherwise, Scatterer moves Herdr focus directly with `herdr pane focus`

For seamless split-edge handoff, Neovim still needs a small Lua keymap that tries
`wincmd h/j/k/l` first and calls `herdr pane focus --direction ... --current`
when the current Neovim window does not change.

## Workspace PR status

Scatterer reports `repo` and one PR badge per workspace when Herdr starts, when
the workspace is created or focused, and when a tab or pane is focused. The repo
name comes from the focused pane's current directory, so it follows a `cd`. The
workspace worktree is the fallback when a workspace has no pane. The Space
sidebar can show `pr_open`, `pr_draft`, `pr_merged`, or `pr_closed` with the PR
number, state icon, and state. Open and draft PRs also report `pr_additions` and
`pr_deletions` (`+798` and `-74`); configure those as separate green and red
sidebar tokens. For top-level worktrees, Scatterer gets the repo name from Git's
common directory.

A `cd` inside the pane you are already in fires no Herdr event, so the repo and
PR tokens keep their last value until the next focus change. PR state is not
polled while a workspace stays focused. Run
`herdr plugin action invoke daniel.scatterer.refresh-spaces` to refresh all
workspaces without switching focus.

## Agent session picker

The `daniel.scatterer.agent-picker` action opens a Telescope-style Herdr popup.
It follows Herdr's session navigator structure and status language, groups live
agents by workspace, and renders the selected agent's current terminal screen
as ANSI-styled text. Wide terminals place the picker on the left and preview on
the right. Narrow terminals stack the picker above the preview. Moving the
pointer over a row updates the preview; clicking or pressing `Enter` focuses the
real agent pane.

Controls:

```txt
↑/↓ or j/k   select and preview
/            search
b/w/i/d/a    blocked/working/idle/done/all
PageUp/Down  scroll the terminal preview
r            refresh agents and preview
Enter/click  focus the selected agent
q/Esc        close
```

Only the selected pane is read. Scatterer requests `pane.read` with `source =
"visible"` and `format = "ansi"`, then parses the returned styles into Ratatui
cells. The preview is read-only and does not focus, resize, or send input to the
agent.

## Herdr upstream follow-ups

Keep these limitations in mind for possible Herdr changes:

1. **Pane previews cannot currently be event-driven.** The public socket API can
   subscribe to pane lifecycle and agent-status events, but it does not expose a
   usable `pane.output_changed` subscription or terminal-frame stream.
   `pane.read` also currently returns no useful changing revision. Scatterer
   therefore polls only the highlighted pane every 150 ms. A coalesced
   `pane.output_changed { pane_id, revision }` event would let the plugin read
   only after output changes; a terminal-cell delta stream would avoid snapshot
   reads entirely. High-volume events need per-pane coalescing and backpressure.
2. **SSH forwarding can become stale in long-lived Herdr sessions.** A Herdr
   server can outlive the SSH connection whose forwarding environment it
   inherited. After reconnecting, forwarded socket paths such as
   `SSH_AUTH_SOCK` may point at the old, dead connection, and panes created by
   the existing Herdr server continue inheriting that stale value. Herdr needs a
   way to refresh connection-scoped environment from the currently attached
   client, or proxy forwarded sockets through a stable Herdr-owned path.

## Nix

The flake exposes the binary as a package, so Nix consumers (for example a
Home Manager configuration) can install Scatterer without a Rust toolchain:

```sh
nix run github:RestartDK/scatterer -- apply-layout
nix build   # ./result/bin/scatterer
```

`packages.plugin` is a ready-to-link Herdr plugin root. Its manifest is
rewritten as data for the immutable store: action, pane, startup, and event
commands invoke the built binary directly. The development launcher and cargo
build hook are omitted:

```sh
nix build .#plugin
herdr plugin link ./result/share/herdr/plugins/scatterer
```

Formatting is orchestrated by treefmt (rustfmt, nixfmt, taplo, shfmt) and
enforced three ways from one config: `nix fmt` locally, a lefthook pre-commit
hook that the devshell installs automatically (enter via `nix develop` or
direnv), and `nix flake check` in CI. If a commit is blocked, the fixed files
are left in the working tree; stage them and retry.

## Development install

A Nix development shell supplies Rust, Clippy, rustfmt, pkg-config, treefmt,
Darwin's `libiconv` linkage, and installs the git hooks on entry:

```sh
cd ~/Projects/scatterer
nix develop   # or `direnv allow` once
cargo test --locked
herdr plugin link .
herdr plugin action invoke daniel.scatterer.apply-layout
herdr plugin action invoke daniel.scatterer.agent-picker
herdr plugin action invoke daniel.scatterer.lazygit
herdr plugin action invoke daniel.scatterer.review-toggle
herdr plugin action invoke daniel.scatterer.appearance-sync
herdr plugin action invoke daniel.scatterer.appearance-install-launchd
herdr plugin action invoke daniel.scatterer.nav-left
```

## Keybinding

Herdr plugins cannot self-install keybindings. Add this to your Herdr config.
To replace Herdr's built-in `prefix+g` navigator with the Scatterer agent
picker, clear the built-in `goto` binding first:

```toml
[keys]
goto = ""

[[keys.command]]
key = "prefix+shift+s"
type = "plugin_action"
command = "daniel.scatterer.apply-layout"
description = "scatterer layout"

[[keys.command]]
key = "prefix+g"
type = "plugin_action"
command = "daniel.scatterer.agent-picker"
description = "scatterer agent session picker"

[[keys.command]]
key = "prefix+shift+g"
type = "plugin_action"
command = "daniel.scatterer.lazygit"
description = "lazygit"

[[keys.command]]
key = "prefix+u"
type = "plugin_action"
command = "daniel.scatterer.review-toggle"
description = "toggle Review pane"

[[keys.command]]
key = "ctrl+h"
type = "plugin_action"
command = "daniel.scatterer.nav-left"
description = "navigate left (vim/herdr)"

[[keys.command]]
key = "ctrl+j"
type = "plugin_action"
command = "daniel.scatterer.nav-down"
description = "navigate down (vim/herdr)"

[[keys.command]]
key = "ctrl+k"
type = "plugin_action"
command = "daniel.scatterer.nav-up"
description = "navigate up (vim/herdr)"

[[keys.command]]
key = "ctrl+l"
type = "plugin_action"
command = "daniel.scatterer.nav-right"
description = "navigate right (vim/herdr)"
```

With Daniel's current `prefix = "ctrl+x"`, these are `ctrl+x` then `shift+s`
for layout, `ctrl+x` then `g` for the agent session picker, `ctrl+x` then
`shift+g` for lazygit, and `ctrl+x` then `u` to toggle Review. The navigation bindings
are direct `ctrl+h/j/k/l` chords, which shadow shell readline defaults such as
`ctrl+l` clear-screen and `ctrl+k` kill-line.

## Per-project configuration

Scatterer merges configuration files from parent directories down to the current
project directory. In each directory it loads files in this order:

1. `scatterer.toml`
2. `.scatterer.toml`
3. `.scatterer.local.toml`

Use `.scatterer.toml` for project config you are comfortable committing. Use
`.scatterer.local.toml` for personal machine-local overrides and add it to
`.git/info/exclude` or your global git ignore.

```toml
[env]
# Defaults to true. When enabled, Scatterer-created panes run
# `direnv export bash` before launching pi/hunk and any configured tabs.
direnv = true

[layout]
agent = "pi"
# Optional override; omitted default is `hunk diff <parent-branch>... --watch`.
# hunk = "hunk diff main... --watch"
# Optional per-project tabs. Defaults do not include process-compose or lazygit.
runner = "process-compose up"
git = "lazygit"
```

Set `runner` only in projects that need a runner tab, for example:

```toml
[layout]
runner = "npm run dev"
```
