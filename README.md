# Scatterer

Personal Herdr workflow plugin.

## Default layout

The `daniel.scatterer.apply-layout` action creates a new Herdr workspace/space
from the currently focused pane's cwd, then uses Herdr's declarative
`layout.apply` socket API to make an `agent` tab with `pi` on the left and
`hunk` on the right.

Repeated invocations create another workspace/space. It does not append tabs to
the workspace you invoked it from. Project config can add extra layout tabs such
as a runner or git UI.

## Quick start

The `daniel.scatterer.quick-start` action opens a Herdr overlay TUI. Optionally
enter a multi-line Pi prompt, choose whether to open a normal workspace or create
a new worktree, optionally enter a branch name, choose a Pi model from
`pi --list-models`, and submit with `Enter`. Use `Shift+Enter` to add prompt
lines.

In `workspace` mode, an empty branch keeps the current branch; entering a branch
switches to it or creates it before opening the workspace. In `worktree` mode,
an empty branch uses `daniel/<prompt-slug>`, so either a prompt or branch is
required. The branch name is also used as the worktree workspace name; when
Scatterer starts Pi explicitly for a prompt/model selection, it uses the branch
or current workspace name as the Pi session name. Scatterer then:

1. creates a Herdr workspace, or creates a Git worktree and opens its workspace
2. for new worktrees only, runs project worktree setup from merged Scatterer
   config, `.herdr/setup.json`, and executable `.herdr/setup-worktree.sh` /
   `.herdr/post-worktree-create.sh` hooks when present
3. applies the Scatterer layout in the workspace
4. starts Pi with the entered prompt as Pi's initial message when a prompt is
   present; if the prompt is empty, the workspace opens without an initial agent
   message

Layout pane commands import `direnv export bash` before starting so tools like Pi,
hunk, and any project-configured runner/git commands inherit the workspace's
allowed `.envrc`.
If lorri has not finished evaluating yet, Scatterer waits and retries briefly
before launching panes. If direnv still fails, Scatterer continues without that
environment and disables direnv hooks in the fallback shell so the same `.envrc`
error does not repeat. Set `[env] direnv = false` in Scatterer config to disable
direnv per project.

Pi supports this directly via its CLI: `pi [messages...]` starts interactive Pi
with an initial prompt. When a prompt or non-default model is selected, Scatterer runs:

```sh
pi --name "<branch-or-session>" [--model "provider/model"] ["<prompt>"]
```

## Lazygit overlay

The `daniel.scatterer.lazygit` action opens `lazygit` in a Herdr overlay using
the focused pane's current working directory.

## Vim/Herdr navigation

The `daniel.scatterer.nav-left`, `nav-down`, `nav-up`, and `nav-right` actions
provide the Herdr side of Vim-style pane navigation. Each action checks the
focused pane's foreground process with `herdr pane process-info --current`:

- if it is Vim/Neovim, Scatterer sends the matching `ctrl+h/j/k/l` key into that
  pane so the editor can move between its own splits
- otherwise, Scatterer moves Herdr focus directly with `herdr pane focus`

For seamless split-edge handoff, Neovim still needs a small Lua keymap that tries
`wincmd h/j/k/l` first and calls `herdr pane focus --direction ... --current`
when the current Neovim window does not change.

## PR picker

The `daniel.scatterer.pr-picker` action opens the same compact overlay style and
lists PRs attached to active Herdr agents. It scans active agents, finds their
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

## Development install

```sh
cd ~/Projects/scatterer
herdr plugin link .
herdr plugin action invoke daniel.scatterer.apply-layout
herdr plugin action invoke daniel.scatterer.quick-start
herdr plugin action invoke daniel.scatterer.pr-picker
herdr plugin action invoke daniel.scatterer.lazygit
herdr plugin action invoke daniel.scatterer.nav-left
```

## Keybinding

Herdr plugins cannot self-install keybindings. Add this to your Herdr config:

```toml
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
key = "prefix+shift+g"
type = "plugin_action"
command = "daniel.scatterer.lazygit"
description = "lazygit"

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
for PR picker, and `ctrl+x` then `shift+g` for lazygit. The navigation bindings
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
hunk = "hunk"
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
