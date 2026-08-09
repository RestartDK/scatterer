# Scatterer

Personal Herdr workflow plugin. Scatterer 0.2.0 requires Herdr 0.7.5 or newer.

## Default layout

The `daniel.scatterer.apply-layout` action creates a new Herdr workspace/space
from the currently focused pane's cwd, then uses Herdr's declarative
`layout.apply` socket API to make an `agent` tab with `pi` on the left and
`hunk diff <parent-branch>... --watch` on the right. The parent branch comes
from the quick-start base ref or saved Scatterer branch metadata when present,
then falls back to the current GitHub PR base, then `main`.

Repeated invocations create another workspace/space. It does not append tabs to
the workspace you invoked it from. Project config can override the Hunk command
or add extra layout tabs such as a runner or git UI.

Hunk theming is best configured through Hunk itself. `theme = "auto"` queries the
terminal background and picks Hunk's light/dark defaults; `transparent_background
= true` lets the terminal background show through. Hunk also has a `tokyo-night`
dark theme, but not a matching Tokyo Night Day theme. Herdr itself supports
`name = "terminal"` or `dark_name = "tokyo-night"` / `light_name =
"tokyo-night-day"` in `~/.config/herdr/config.toml`.

## Scatterer popup theming

Scatterer's agent picker, PR picker, and quick-start popup share one theme
module. It reads Herdr's `[theme]` configuration, mirrors every built-in Herdr
palette, applies `[theme.custom]` overrides, and follows `auto_switch` on macOS.
Selection backgrounds, borders, text, muted labels, and semantic status colors
therefore match the active Herdr theme instead of using fixed ANSI colors.
External applications such as Hunk and lazygit continue to manage their own
themes.

## Quick start

The `daniel.scatterer.quick-start` action opens a Herdr popup TUI. Optionally
enter a multi-line Pi prompt, choose one of three workspace modes, optionally
enter a branch name and base ref, choose a Pi model from `pi --list-models`, and
submit with `Enter`. Use `Shift+Enter` or `Ctrl+J` to add prompt lines. The
modes are:

- `workspace (current checkout)` — opens another workspace for the current
  checkout, optionally switching or creating its branch first
- `worktree (Herdr group, indented)` — keeps Herdr's normal managed worktree
  behavior and displays the new workspace under the source repository
- `worktree (top-level space, not indented)` — creates the same Git worktree in
  Herdr's configured worktree directory, but reopens it as an ordinary top-level
  workspace without Herdr grouping provenance

Use Left/Right or Space while the `Mode` field is active to change the behavior
for that quick-start invocation.

In `workspace` mode, an empty branch keeps the current branch; entering a branch
switches to it or creates it before opening the workspace. If the branch is new,
an optional base ref creates it from that ref instead of the current `HEAD`. In
either worktree mode, an empty branch uses `daniel/<prompt-slug>`, so either a
prompt or branch is required. An optional base ref is passed to Herdr when
creating the worktree; blank uses the source checkout's current ref. For stacked
PRs, enter the child PR branch in `Branch` and the parent PR branch in `Base
ref`. The branch name is also used as the worktree workspace name; when Scatterer
starts Pi explicitly for a prompt/model selection, it uses the branch or current
workspace name as the Pi session name. Scatterer then:

1. creates a Herdr workspace, or creates a Git worktree and opens either a
   grouped or top-level workspace for it
2. for new worktrees only, runs project worktree setup from merged Scatterer
   config, `.herdr/setup.json`, and executable `.herdr/setup-worktree.sh` /
   `.herdr/post-worktree-create.sh` hooks when present
3. applies the Scatterer layout in the workspace
4. starts Pi through Herdr's live-agent facade, waits for it to become ready,
   and atomically submits the entered multiline prompt when present

For top-level mode, Scatterer lets Herdr create the checkout first so the path
still honors `[worktrees].directory` (for example
`~/.herdr/worktrees/<repo>/<branch-slug>`). It then calls `workspace.close` on
the temporary grouped workspace—this does not remove the checkout—and opens the
same path with `workspace.create`. The final workspace is a real Git worktree but
is not indented. Scatterer records these checkouts in its Herdr plugin state.
Invoke `daniel.scatterer.remove-flat-worktree` from inside one to remove a clean
checkout safely and close its workspace; the branch is retained. Dirty
worktrees are refused. Avoid Herdr's `Open worktree` action if you do not want a
top-level checkout grouped again.

Layout pane commands import `direnv export bash` before starting so tools like Pi,
hunk, and any project-configured runner/git commands inherit the workspace's
allowed `.envrc`.
If lorri has not finished evaluating yet, Scatterer waits and retries briefly
before launching panes. If direnv still fails, Scatterer continues without that
environment and disables direnv hooks in the fallback shell so the same `.envrc`
error does not repeat. Set `[env] direnv = false` in Scatterer config to disable
direnv per project.

Scatterer starts Pi in the layout's shell pane with Herdr's `agent.start` API,
passing `--name "<branch-or-session>"` and an optional `--model`. It then uses
`agent.prompt` for the initial message, avoiding shell quoting and preserving
multiline text atomically.

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

## PR picker

The `daniel.scatterer.pr-picker` action opens a compact Herdr popup and lists
PRs attached to active Herdr agents. It scans active agents, finds their
current worktree/branch, resolves the matching GitHub PR with `gh`, and shows:

- Nerd Font PR state icon: open, draft, merged, or closed
- PR number, title, review decision, CI state, comments, files, and lines changed
- selected-PR details with associated agent, agent status, branch, and URL

Controls:

```txt
↑/↓ or j/k   select
Enter        focus the associated workspace/agent
o            open PR in browser, or copy the URL via terminal clipboard over SSH
r            refresh
y            copy PR URL
q/Esc        close
```

Refreshing the picker reports `pr_url`, `pr_number`, and `pr_state` pane
metadata. It also reports exactly one styled-sidebar badge token for the current
state: `pr_open`, `pr_draft`, `pr_merged`, or `pr_closed`. Each badge displays
the PR number, Nerd Font state icon, and state inside the normal Herdr Agents
view; Scatterer does not replace or filter that view.

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

## Development install

A Nix development shell supplies Rust, Clippy, rustfmt, pkg-config, and Darwin's
`libiconv` linkage:

```sh
cd ~/Projects/scatterer
nix develop
cargo test --locked
herdr plugin link .
herdr plugin action invoke daniel.scatterer.apply-layout
herdr plugin action invoke daniel.scatterer.quick-start
herdr plugin action invoke daniel.scatterer.pr-picker
herdr plugin action invoke daniel.scatterer.agent-picker
herdr plugin action invoke daniel.scatterer.remove-flat-worktree
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
key = "prefix+shift+a"
type = "plugin_action"
command = "daniel.scatterer.quick-start"
description = "scatterer quick start"

[[keys.command]]
key = "prefix+shift+p"
type = "plugin_action"
command = "daniel.scatterer.pr-picker"
description = "scatterer PR picker"

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
for layout, `ctrl+x` then `shift+a` for quick start, `ctrl+x` then `shift+p`
for PR picker, `ctrl+x` then `g` for the agent session picker, `ctrl+x` then
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

[quick_start.setup]
# Shell commands run in each new quick-start worktree before the layout is
# applied. Commands from multiple config files are appended in merge order.
commands = [
  "touch .lorri-off",
  "direnv allow",
]
```

Set `runner` only in projects that need a runner tab, for example:

```toml
[layout]
runner = "npm run dev"
```

Quick-start also supports setup files and executable hooks alongside config
commands:

```txt
.herdr/setup.json
.herdr/setup-worktree.sh
.herdr/post-worktree-create.sh
```

Scatterer checks both the source checkout and the newly-created worktree, so
local-only setup in your source checkout still runs even when the worktree has a
tracked `.herdr` directory. Identical tracked files are skipped to avoid running
the same setup twice.

These can be local-only too. For personal Cobb-style setup, keep the files in
your checkout and ignore them locally:

```sh
cat >> .git/info/exclude <<'EOF'
.scatterer.local.toml
.herdr/setup.json
.herdr/setup-worktree.sh
.herdr/post-worktree-create.sh
EOF
```
