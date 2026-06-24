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
prompt, optionally enter a branch name, and submit. Scatterer then:

1. creates a Git worktree from the current repo
2. applies the same three-tab layout in the new worktree workspace
3. starts Pi in the `agent` tab with the entered prompt as Pi's initial message

Pi supports this directly via its CLI: `pi [messages...]` starts interactive Pi
with an initial prompt. Scatterer currently runs:

```sh
pi --name "quick <prompt-slug>" "<prompt>"
```

Other harnesses can be added later.

## Development install

```sh
cd ~/Projects/scatterer
herdr plugin link .
herdr plugin action invoke daniel.scatterer.apply-layout
herdr plugin action invoke daniel.scatterer.quick-start
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
```

With Daniel's current `prefix = "ctrl+x"`, these are `ctrl+x` then `shift+s`
for layout and `ctrl+x` then `shift+a` for quick start.

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
