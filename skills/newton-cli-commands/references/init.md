# newton init

## Purpose

`newton init` bootstraps a workspace by creating the `.newton/` layout, installing the template artifacts, and writing the default config values Newton needs to start orchestrating loops.

## Usage

- `newton init [PATH]`: Initialize the provided directory. When `PATH` is omitted, the current working directory is used.
- `--template-source <SOURCE>`: Override the default `gonewton/newton-templates` source with another GitHub repo, URL, or local folder that contains `aikit.toml`.

## What the command does

- Uses **aikit-sdk** to install the Newton template (README, scripts, tasks, plan directories, etc.) under `.newton/`.
- Ensures `.newton/configs/`, `.newton/tasks/`, `.newton/plan/default/{todo,completed,failed,draft}`, and `.newton/state/` exist even if the template omits them.
- Writes `.newton/configs/default.conf` with `project_root=.`, `coding_agent=opencode`, `coding_model=zai-coding-plan/glm-4.7`, and optional `post_success_script`/`post_fail_script` entries when the scripts are present.
- Creates a stub `.newton/scripts/executor.sh` if the template did not provide one so `newton run` can start.

## Follow-up

After initialization you can simply run `newton run` from the workspace root (no path argument) to begin optimization; Newton now defaults to the current directory when no workspace path is supplied.
