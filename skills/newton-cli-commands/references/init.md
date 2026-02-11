# newton init

## Purpose
Creates the `.newton` workspace metadata (configs, tasks, plan folders) and installs the Newton template through `aikit-sdk` so the default `.newton/scripts` toolchain is available.

## Required Input
- `[PATH]`: Optional workspace directory (defaults to the current directory). The path must already exist and be a directory before running `init`.

## Important Flags
- `--template-source <SOURCE>`: (Optional) Override the template source. The default is `gonewton/newton-templates` and refers to the built-in template shipped with the CLI. Provide a directory path to override with a custom template.

## Example Invocations
```bash
# Initialize the current directory
newton init

# Initialize another workspace
newton init /path/to/workspace
```

## Resulting Layout
- `.newton/configs/default.conf` with `project_root=.`, `coding_agent=opencode`, and default model/script entries.
- `.newton/scripts/*.sh` files for evaluator/advisor/executor/post-success/post-failure (executor stub added when missing).
- `.newton/state`, `.newton/tasks`, and `.newton/plan/default/{todo,completed,failed,draft}` directories.

## Aftermath
Once `init` succeeds you can run `newton run` without supplying a path because it defaults to the current directory and uses the `.newton/scripts/<phase>.sh` commands that `init` installed.
