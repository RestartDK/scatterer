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

The `daniel.scatterer.quick-start` action opens a compact Herdr overlay TUI.
Enter a prompt, optionally enter a branch name, choose a Pi model from
`pi --list-models`, and submit. Scatterer then:

1. creates a Git worktree from the current repo
2. applies the same three-tab layout in the new worktree workspace
3. starts Pi in the `agent` tab with the entered prompt as Pi's initial message

Pi supports this directly via its CLI: `pi [messages...]` starts interactive Pi
with an initial prompt. Scatterer currently runs:

```sh
pi --name "quick <prompt-slug>" [--model "provider/model"] "<prompt>"
```

## PR picker

The `daniel.scatterer.pr-picker` action opens the same compact overlay style and
lists PRs attached to active Herdr agents. It scans active agents, finds their
current worktree/branch, resolves the matching GitHub PR with `gh`, and shows:

- Nerd Font PR state icon: open, draft, merged, or closed
- PR number, title, review decision, CI state, and comment count
- associated agent, agent status, and branch

Controls:

```txt
↑/↓ or j/k   select
Enter        focus the associated workspace/agent
o            open PR in browser
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
key = "prefix+shift+r"
type = "plugin_action"
command = "daniel.scatterer.pr-picker"
description = "scatterer PR picker"
```

With Daniel's current `prefix = "ctrl+x"`, these are `ctrl+x` then `shift+s`
for layout, `ctrl+x` then `shift+a` for quick start, and `ctrl+x` then
`shift+r` for PR picker.

## Per-project runner/commands

Put `.scatterer.toml` or `scatterer.toml` in a project directory to override the
commands used by the layout:

```toml
[layout]
agent = "pi"
hunk = "hunk"
runner = "process-compose up"
git = "lazygit"
```

`runner` is the main one you'll normally customize per project, for example:

```toml
[layout]
runner = "npm run dev"
```
