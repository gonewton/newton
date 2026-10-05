#!/usr/bin/env python3
"""Create a fresh disposable Git fixture for the real-agent Docker gate."""

import argparse
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--model", required=True, help="exact Pi provider/model")
    args = parser.parse_args()
    root = args.destination.resolve()
    root.mkdir(parents=True, exist_ok=False)
    fixture = Path(__file__).parent / "fixtures/optimization-docs"
    shutil.copytree(fixture / "content", root, dirs_exist_ok=True)
    definition = root / ".newton/definitions/documentation-trial"
    definition.mkdir(parents=True)
    for path in fixture.iterdir():
        if path.is_file():
            shutil.copyfile(path, definition / path.name)
    configs = root / ".newton/configs"
    configs.mkdir()
    if "\n" in args.model or "\r" in args.model:
        parser.error("model must be one line")
    (configs / "trial.conf").write_text(
        "project_root=.\ndefinition_file=.newton/definitions/documentation-trial/definition.yaml\n"
        "optimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy\n"
        f"parameter.agent=pi\nparameter.model={args.model}\n"
    )
    (root / ".gitignore").write_text(
        ".newton/state/\n.newton/artifacts/\n.newton/trial-evidence/\nlogs/\n"
    )

    def git(*args):
        subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True)

    git("init")
    git("add", ".")
    git(
        "-c",
        "user.name=Newton trial",
        "-c",
        "user.email=trial@localhost",
        "commit",
        "-m",
        "docs: seed three broken links and a runnable example",
    )
    print(root)


if __name__ == "__main__":
    main()
