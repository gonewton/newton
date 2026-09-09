"""Trusted-host Cargo.lock dependency audit adapter; not a security sandbox.

Uses only the standard library. All configuration arrives as JSON arguments,
never shell interpolation or new environment variables. Artifacts are retained.
"""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
import uuid


def main(role, input_path):
    request = json.loads(Path(input_path).read_text())
    root = Path(request["workspace"]).resolve()
    params = {
        key: value["value"]
        for key, value in request["parameters"].items()
        if value["kind"] == "literal"
    }
    deadline = time.monotonic() + request.get("remaining_seconds", 60)

    def run(argv, cwd=root, allowed=(0,), env_overrides=None):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise RuntimeError("security adapter time budget exhausted")
        environment = None
        if env_overrides:
            environment = os.environ.copy()
            environment.update(env_overrides)
        result = subprocess.run(
            argv,
            cwd=cwd,
            text=True,
            capture_output=True,
            timeout=remaining,
            check=False,
            env=environment,
        )
        if result.returncode not in allowed:
            raise RuntimeError(
                f"{argv[0]} failed ({result.returncode}): {result.stderr[-3000:]}"
            )
        return result

    def git(*args, cwd=root):
        return run(["git", *args], cwd=cwd).stdout.strip()

    def protected_manifest_view(value, path=()):
        """Remove only dependency-resolution data from parsed Cargo manifests."""
        if isinstance(value, list):
            return [protected_manifest_view(item, path) for item in value]
        if not isinstance(value, dict):
            return value
        result = {}
        dependency_tables = {"dependencies", "dev-dependencies", "build-dependencies"}
        for key, item in value.items():
            if not path and key in dependency_tables | {"patch", "replace"}:
                continue
            if path == ("workspace",) and key == "dependencies":
                continue
            if len(path) == 2 and path[0] == "target" and key in dependency_tables:
                continue
            projected = protected_manifest_view(item, (*path, key))
            # A target selector containing only dependencies is dependency data.
            if path == ("target",) and isinstance(projected, dict) and not projected:
                continue
            result[key] = projected
        if not path and isinstance(result.get("target"), dict) and not result["target"]:
            result.pop("target")
        return result

    def manifest_policy(base_commit, candidate_dir, changed_paths):
        violations = []
        for relative in changed_paths:
            if Path(relative).name != "Cargo.toml":
                continue
            candidate_path = candidate_dir / relative
            base = run(
                ["git", "show", f"{base_commit}:{relative}"],
                cwd=root,
                allowed=(0, 128),
            )
            if base.returncode != 0 or not candidate_path.is_file():
                violations.append(
                    f"{relative}: Cargo manifests cannot be added or removed"
                )
                continue
            try:
                base_manifest = tomllib.loads(base.stdout)
                candidate_manifest = tomllib.loads(
                    candidate_path.read_text(encoding="utf-8")
                )
            except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
                violations.append(f"{relative}: invalid Cargo manifest: {error}")
                continue
            if protected_manifest_view(base_manifest) != protected_manifest_view(
                candidate_manifest
            ):
                violations.append(
                    f"{relative}: only dependency tables, patch/replace entries, and Cargo.lock may change"
                )
        return violations

    def tree_entries(commit):
        entries = {}
        for entry in git("ls-tree", "-r", "-z", "--full-tree", commit).split("\0"):
            if not entry:
                continue
            metadata, relative = entry.split("\t", 1)
            mode, kind, object_id = metadata.split(" ", 2)
            entries[relative] = (mode, kind, object_id)
        return entries

    def validate_committed_cargo_inputs(commit, checkout):
        """Reject Cargo control inputs that are external to the immutable tree."""
        entries = tree_entries(commit)
        cargo_inputs = [
            relative
            for relative in entries
            if Path(relative).name in ("Cargo.toml", "Cargo.lock")
        ]
        if "Cargo.toml" not in cargo_inputs or "Cargo.lock" not in cargo_inputs:
            raise RuntimeError(
                "software-security requires committed root Cargo.toml and Cargo.lock"
            )
        for relative in cargo_inputs:
            mode, kind, _ = entries[relative]
            path = checkout / relative
            if mode == "120000" or path.is_symlink():
                raise RuntimeError(
                    f"{relative} must be a committed regular file, not a symlink"
                )
            if kind != "blob" or not path.is_file():
                raise RuntimeError(f"{relative} must be a committed regular file")

        # Cargo reads these files from the invocation directory. An ignored or
        # untracked project config can otherwise replace the test runner without
        # appearing in the candidate diff.
        for relative in (".cargo/config", ".cargo/config.toml"):
            path = checkout / relative
            present = path.exists() or path.is_symlink()
            if not present:
                continue
            entry = entries.get(relative)
            if entry is None:
                raise RuntimeError(f"{relative} must be committed before optimization")
            if (
                entry[0] == "120000"
                or entry[1] != "blob"
                or path.is_symlink()
                or not path.is_file()
            ):
                raise RuntimeError(
                    f"{relative} must be a committed regular file, not a symlink"
                )

    def validate_original_state(original):
        if git("rev-parse", "HEAD") != original or git(
            "status", "--porcelain", "--untracked-files=no"
        ):
            raise RuntimeError(
                "original project state changed during exploration; reconcile before continuing"
            )
        validate_committed_cargo_inputs(original, root)

    def isolated_evaluation_checkout(candidate):
        # This directory is created only after agent development has returned.
        # Keeping it outside the source repository prevents Cargo from inheriting
        # ignored or untracked .cargo/config files from that repository.
        evaluation_root = Path(tempfile.mkdtemp(prefix="newton-security-evaluation-"))
        evaluation_dir = evaluation_root / "checkout"
        cargo_home = evaluation_root / "cargo-home"
        worktree_registered = False
        try:
            cargo_home.mkdir()
            for parent in evaluation_dir.parents:
                for name in ("config", "config.toml"):
                    config = parent / ".cargo" / name
                    if config.exists() or config.is_symlink():
                        raise RuntimeError(
                            f"isolated evaluation ancestor contains Cargo configuration: {config}"
                        )
            git(
                "worktree",
                "add",
                "--detach",
                str(evaluation_dir),
                candidate["artifact_id"],
            )
            worktree_registered = True
            validate_committed_cargo_inputs(candidate["artifact_id"], evaluation_dir)
        except Exception as error:
            try:
                remove_evaluation_checkout(
                    evaluation_root, evaluation_dir, worktree_registered
                )
            except Exception as cleanup_error:
                raise RuntimeError(
                    f"isolated evaluation setup failed and cleanup also failed: {cleanup_error}"
                ) from error
            raise
        return evaluation_root, evaluation_dir, cargo_home

    def remove_evaluation_checkout(
        evaluation_root, evaluation_dir, worktree_registered=True
    ):
        issues = []
        registered = worktree_registered
        try:
            listed = subprocess.run(
                ["git", "worktree", "list", "--porcelain"],
                cwd=root,
                text=True,
                capture_output=True,
                timeout=30,
                check=False,
            )
            if listed.returncode != 0:
                issues.append(
                    f"git worktree inspection failed: {listed.stderr[-3000:]}"
                )
            else:
                target = evaluation_dir.resolve(strict=False)
                registered = registered or any(
                    Path(line.removeprefix("worktree ")).resolve(strict=False) == target
                    for line in listed.stdout.splitlines()
                    if line.startswith("worktree ")
                )
        except (OSError, subprocess.TimeoutExpired) as error:
            issues.append(f"git worktree inspection failed: {error}")
        if registered:
            try:
                cleanup = subprocess.run(
                    ["git", "worktree", "remove", "--force", str(evaluation_dir)],
                    cwd=root,
                    text=True,
                    capture_output=True,
                    timeout=30,
                    check=False,
                )
                if cleanup.returncode != 0:
                    issues.append(
                        f"git worktree removal failed: {cleanup.stderr[-3000:]}"
                    )
            except (OSError, subprocess.TimeoutExpired) as error:
                issues.append(f"git worktree removal failed: {error}")
        try:
            if evaluation_root.exists() or evaluation_root.is_symlink():
                shutil.rmtree(evaluation_root)
        except OSError as error:
            issues.append(f"temporary directory removal failed: {error}")
        if issues:
            raise RuntimeError("; ".join(issues))

    def output(value):
        path = Path(request["result_file"])
        temporary = path.with_suffix(".tmp")
        temporary.write_text(json.dumps({"done": True, "metadata": value}))
        temporary.replace(path)

    scanner = params.get("scanner_command")
    tests = params.get("test_command")
    for name, command in [("scanner_command", scanner), ("test_command", tests)]:
        if (
            not isinstance(command, list)
            or not command
            or not all(isinstance(x, str) and x for x in command)
        ):
            raise RuntimeError(f"parameter.{name} must be a nonempty JSON argv array")
        if not shutil.which(command[0]):
            raise RuntimeError(
                f"missing {command[0]}; install the evaluator/test prerequisite or configure parameter.{name}"
            )
    db = Path(params.get("advisory_db", "")).expanduser()
    expected_db = params.get("advisory_db_revision", "")
    if not str(params.get("advisory_db", "")) or not expected_db:
        raise RuntimeError(
            "set parameter.advisory_db to a prepared RustSec database and parameter.advisory_db_revision to its Git commit; Newton does not fetch it"
        )
    if not db.is_absolute():
        db = root / db
    if git("rev-parse", "HEAD", cwd=db) != expected_db:
        raise RuntimeError(
            "advisory database revision differs from parameter.advisory_db_revision"
        )
    if git("status", "--porcelain", "--untracked-files=no", cwd=db):
        raise RuntimeError("advisory database has tracked modifications")
    if role == "preflight":
        head = git("rev-parse", "HEAD")
        validate_committed_cargo_inputs(head, root)
        if git("status", "--porcelain", "--untracked-files=no"):
            raise RuntimeError(
                "commit or stash tracked changes before starting; untracked files are not evaluated"
            )
        agent = params.get("agent", "")
        if agent not in ("pi", "claude", "codex") or not shutil.which(agent):
            raise RuntimeError(
                "configure parameter.agent as installed pi, claude, or codex; authentication stays in its existing configuration"
            )
        if not params.get("model"):
            raise RuntimeError(
                "set parameter.model to a model supported by the configured agent"
            )
        run([*scanner, "--version"])
        output(
            {
                "ready": True,
                "surface": "Cargo.lock RustSec advisory matches; not comprehensive security or compliance",
            }
        )
        return

    artifacts = root / ".newton" / "optimize-artifacts" / request["run_id"] / "security"
    artifacts.mkdir(parents=True, exist_ok=True)
    origin = artifacts / "original.json"
    if not origin.exists():
        initial = git("rev-parse", "HEAD")
        validate_original_state(initial)
        origin.write_text(json.dumps({"commit": initial}))
    original = json.loads(origin.read_text())["commit"]
    validate_original_state(original)
    accepted = request.get("accepted_result")
    base = accepted["candidate"]["artifact_id"] if accepted else original
    candidate_dir = artifacts / f"candidate-{request['cycle']}"

    if role == "prepare":
        if candidate_dir.exists():
            raise RuntimeError(
                "candidate directory already exists; reconcile previous work instead of replaying it"
            )
        git("worktree", "add", "--detach", str(candidate_dir), base)
        prompt = (
            "Improve dependency security in this detached candidate worktree. "
            "Change only Cargo.toml manifests and Cargo.lock. Do not edit tests, evaluator files, "
            "other worktrees, refs, or configuration. Do not publish, merge, or deploy. "
            "You may leave changes uncommitted or create local commits in this detached worktree; "
            "do not rewrite or discard the base history. "
            "Run the project's tests. Newton will independently audit the resulting immutable commit. "
            "The goal is fewer RustSec advisory matches in Cargo.lock. "
            f"Pinned advisory database: {db}; revision: {expected_db}. "
            f"Scanner argv: {json.dumps(scanner)}; test argv: {json.dumps(tests)}. "
            "These instructions are scope guidance, not a sandbox."
        )
        output({"candidate_directory": str(candidate_dir), "prompt": prompt})
        return

    if role == "snapshot":
        validate_original_state(original)
        candidate_head = git("rev-parse", "HEAD", cwd=candidate_dir)
        run(
            ["git", "merge-base", "--is-ancestor", base, candidate_head],
            cwd=candidate_dir,
        )
        git("add", "--all", cwd=candidate_dir)
        tree = git("write-tree", cwd=candidate_dir)
        commit = git(
            "-c",
            "user.name=Newton",
            "-c",
            "user.email=newton@localhost",
            "commit-tree",
            tree,
            "-p",
            base,
            "-m",
            "chore: retain optimization candidate",
            cwd=candidate_dir,
        )
        validate_committed_cargo_inputs(commit, candidate_dir)
        git("update-ref", f"refs/newton/candidates/{request['candidate_id']}", commit)
        output(
            {
                "candidate": {
                    "id": request["candidate_id"],
                    "artifact_id": commit,
                    "base_artifact_id": base,
                    "created_under_revision": request["requirements_revision"],
                }
            }
        )
        return

    if role != "grade":
        raise RuntimeError(f"unsupported adapter role: {role}")
    if request["stage"] == "baseline":
        candidate = (
            accepted["candidate"]
            if accepted
            else {
                "id": request["candidate_id"],
                "artifact_id": original,
                "base_artifact_id": original,
                "created_under_revision": request["requirements_revision"],
            }
        )
    else:
        candidate = request["candidate"]
    evaluation_id = str(uuid.uuid4())
    evaluation_root, evaluation_dir, cargo_home = isolated_evaluation_checkout(
        candidate
    )
    evaluation_error = None
    try:
        evaluation_env = {"CARGO_HOME": str(cargo_home)}
        audit = run(
            [
                *scanner,
                "--json",
                "--no-fetch",
                "--db",
                str(db),
                "--file",
                str(evaluation_dir / "Cargo.lock"),
            ],
            cwd=evaluation_dir,
            allowed=(0, 1),
            env_overrides=evaluation_env,
        )
        report = json.loads(audit.stdout)
        vulnerabilities = report["vulnerabilities"]
        count = vulnerabilities["count"]
        if (
            type(count) is not int
            or count < 0
            or not isinstance(vulnerabilities.get("list"), list)
            or count != len(vulnerabilities["list"])
        ):
            raise RuntimeError(
                "cargo-audit returned an invalid vulnerabilities count/list"
            )
        if (audit.returncode == 0) != (count == 0):
            raise RuntimeError(
                "cargo-audit status disagrees with reported vulnerabilities"
            )
        evidence = artifacts / f"audit-{evaluation_id}.json"
        evidence.write_text(audit.stdout)
        test_result = run(
            tests,
            cwd=evaluation_dir,
            allowed=tuple(range(256)),
            env_overrides=evaluation_env,
        )
        test_log = artifacts / f"tests-{evaluation_id}.txt"
        test_log.write_text(test_result.stdout + test_result.stderr)
        if git("rev-parse", "HEAD", cwd=evaluation_dir) != candidate[
            "artifact_id"
        ] or git("status", "--porcelain", "--untracked-files=no", cwd=evaluation_dir):
            raise RuntimeError(
                "evaluation changed tracked candidate state; evidence is invalid"
            )
        validate_committed_cargo_inputs(candidate["artifact_id"], evaluation_dir)
        if git("rev-parse", "HEAD", cwd=db) != expected_db or git(
            "status", "--porcelain", "--untracked-files=no", cwd=db
        ):
            raise RuntimeError("advisory inputs changed during evaluation")
        validate_original_state(original)
        changed = [
            path
            for path in git(
                "-c",
                "core.quotePath=false",
                "diff",
                "--name-only",
                "-z",
                candidate["base_artifact_id"],
                candidate["artifact_id"],
            ).split("\0")
            if path
        ]
        manifest_violations = manifest_policy(
            candidate["base_artifact_id"], evaluation_dir, changed
        )
        dependency_only = (
            all(
                path == "Cargo.lock" or Path(path).name == "Cargo.toml"
                for path in changed
            )
            and not manifest_violations
        )

        def check(passed, refs):
            return {
                "evaluator": "cargo-audit",
                "status": "satisfied" if passed else "violated",
                "evidence": refs,
            }

        evaluation = {
            "id": evaluation_id,
            "run_id": request["run_id"],
            "cycle": request["cycle"],
            "candidate_id": candidate["id"],
            "artifact_id": candidate["artifact_id"],
            "base_artifact_id": candidate["base_artifact_id"],
            "requirements_revision": request["requirements_revision"],
            "evaluator_revisions": {
                key: value["revision"]
                for key, value in request["requirements"]["evaluators"].items()
            },
            "measurements": {
                "vulnerabilities": {
                    "status": "produced",
                    "measurement": {
                        "kind": "numeric",
                        "unit": "affected_package_advisories",
                        "direction": "minimize",
                    },
                    "samples": [count],
                }
            },
            "constraints": {
                "tests_pass": check(
                    test_result.returncode == 0,
                    [str(test_log), str(evidence), f"{db}@{expected_db}"],
                ),
                "dependency_files_only": check(
                    dependency_only, changed + manifest_violations
                ),
            },
            "completion_checks": {},
        }
        output({"candidate": candidate, "evaluation": evaluation})
    except Exception as error:
        evaluation_error = error
        raise
    finally:
        try:
            remove_evaluation_checkout(evaluation_root, evaluation_dir)
        except Exception as cleanup_error:
            if evaluation_error is None:
                raise
            evaluation_error.add_note(
                f"isolated evaluation cleanup also failed: {cleanup_error}"
            )


if __name__ == "__main__":
    try:
        main(*sys.argv[1:])
        print("NEWTON_HELPER_COMPLETED", flush=True)
    except Exception as error:
        print(f"software-security: {error}", file=sys.stderr)
        sys.exit(1)
