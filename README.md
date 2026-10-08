# Tasker

Tasker is a local Rust TUI for tracking development tasks against one GitHub
repository. Each task gets a copyable branch name, and its status is refreshed
through the GitHub CLI. A task is automatically completed when its pull request
is merged.

## Requirements

- Rust 1.85 or newer
- [GitHub CLI](https://cli.github.com/) installed and authenticated
- A Wayland compositor with clipboard support

Authenticate GitHub CLI before starting Tasker:

```sh
gh auth login
```

## Install

From this repository:

```sh
cargo install --path .
tasker
```

On first launch, enter the repository to track in `owner/repo` format. Tasker
validates access using `gh repo view` and stores the setting locally.

The SQLite database is stored at:

```text
~/.local/share/tasker/tasker.db
```

## Shortcuts

| Key | Action |
| --- | --- |
| `c` | Create a task (`Tab` switches to the optional description) |
| `e` | Edit the selected task's description |
| `y` | Copy the selected branch name |
| `r` | Refresh GitHub status |
| `Enter` | Open or close task details |
| `j` / `Down` | Select the next task |
| `k` / `Up` | Select the previous task |
| `q` | Quit |

Tasker reads your username from the authenticated GitHub CLI account. Task
titles become branch names in the form `<username>/<id>-<slug>`, for example
`icehunt/42-add-filtering`. Tasker only generates and copies names; it does not
create, switch, push, or delete Git branches. Branch names created by older
Tasker versions are left unchanged so their existing pull requests keep
tracking correctly.

## GitHub Status

Tasker refreshes when it starts, every 60 seconds, and when `r` is pressed. It
runs `gh pr list` and `gh api` without invoking a shell. The displayed states
are:

- `Not pushed`
- `Branch pushed`
- `Draft PR`
- `PR open`
- `PR closed`
- `Merged`

Only `Merged` automatically completes a task. Closing a pull request without
merging it leaves the task active.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
