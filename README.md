# Scatterer

Personal Herdr workflow plugin.

## Default layout

The `daniel.scatterer.apply-layout` action creates a new Herdr workspace/space
from the currently focused pane's cwd, then uses Herdr's declarative
`layout.apply` socket API to make three tabs:

1. `agent`: `pi` on the left, `hunk` on the right
2. `runner`: `process-compose up`
3. `git`: `lazygit`

Repeated invocations create another workspace/space. It does not append tabs to
the workspace you invoked it from.

## Quick start

The `daniel.scatterer.quick-start` action opens a Herdr overlay TUI. Enter a
multi-line prompt, optionally enter a branch name, choose a Pi model from
`pi --list-models`, and submit with `Enter`. Use `Shift+Enter` to add prompt
lines. If the branch is empty, Scatterer
uses `daniel/<prompt-slug>`. The branch name is also used as the worktree
workspace name and Pi session name. Scatterer then:

1. creates a Git worktree from the current repo
2. runs project worktree setup from merged Scatterer config, `.herdr/setup.json`,
   and executable `.herdr/setup-worktree.sh` / `.herdr/post-worktree-create.sh`
   hooks when present
3. applies the same three-tab layout in the new worktree workspace
4. starts Pi in the `agent` tab with the entered prompt as Pi's initial message

Layout pane commands import `direnv export bash` before starting so tools like Pi,
hunk, `process-compose`, and lazygit inherit the worktree's allowed `.envrc`.
If lorri has not finished evaluating yet, Scatterer waits and retries briefly
before launching panes. If direnv still fails, Scatterer continues without that
environment and disables direnv hooks in the fallback shell so the same `.envrc`
error does not repeat. Set `[env] direnv = false` in Scatterer config to disable
direnv per project.

Pi supports this directly via its CLI: `pi [messages...]` starts interactive Pi
with an initial prompt. Scatterer currently runs:

```sh
pi --name "<branch>" [--model "provider/model"] "<prompt>"
```

## Lazygit overlay

The `daniel.scatterer.lazygit` action opens `lazygit` in a Herdr overlay using
the focused pane's current working directory.

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
```

With Daniel's current `prefix = "ctrl+x"`, these are `ctrl+x` then `shift+s`
for layout, `ctrl+x` then `shift+a` for quick start, `ctrl+x` then `shift+p`
for PR picker, and `ctrl+x` then `shift+g` for lazygit.

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
# `direnv export bash` before launching pi/hunk/runner/lazygit.
direnv = true

[layout]
agent = "pi"
hunk = "hunk"
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

`runner` is the main layout command you'll normally customize per project, for
example:

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
