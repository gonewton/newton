"""Trusted-host Cargo.lock dependency audit adapter; not a security sandbox.

Uses only the standard library. All configuration arrives as JSON arguments,
never shell interpolation or new environment variables. Artifacts are retained.
"""

import json
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid


def main(role, input_path):
    request = json.loads(Path(input_path).read_text())
    root = Path(request["workspace"]).resolve()
    params = {key: value["value"] for key, value in request["parameters"].items()
              if value["kind"] == "literal"}
    deadline = time.monotonic() + request.get("remaining_seconds", 60)

    def run(argv, cwd=root, allowed=(0,)):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise RuntimeError("security adapter time budget exhausted")
        result = subprocess.run(argv, cwd=cwd, text=True, capture_output=True,
                                timeout=remaining, check=False)
        if result.returncode not in allowed:
            raise RuntimeError(f"{argv[0]} failed ({result.returncode}): {result.stderr[-3000:]}")
        return result

    def git(*args, cwd=root):
        return run(["git", *args], cwd=cwd).stdout.strip()

    def output(value):
        path = Path(request["result_file"])
        temporary = path.with_suffix(".tmp")
        temporary.write_text(json.dumps({"done": True, "metadata": value}))
        temporary.replace(path)

    scanner = params.get("scanner_command")
    tests = params.get("test_command")
    for name, command in [("scanner_command", scanner), ("test_command", tests)]:
        if not isinstance(command, list) or not command or not all(isinstance(x, str) and x for x in command):
            raise RuntimeError(f"parameter.{name} must be a nonempty JSON argv array")
        if not shutil.which(command[0]):
            raise RuntimeError(f"missing {command[0]}; install the evaluator/test prerequisite or configure parameter.{name}")
    db = Path(params.get("advisory_db", "")).expanduser()
    expected_db = params.get("advisory_db_revision", "")
    if not str(params.get("advisory_db", "")) or not expected_db:
        raise RuntimeError("set parameter.advisory_db to a prepared RustSec database and parameter.advisory_db_revision to its Git commit; Newton does not fetch it")
    if not db.is_absolute():
        db = root / db
    if git("rev-parse", "HEAD", cwd=db) != expected_db:
        raise RuntimeError("advisory database revision differs from parameter.advisory_db_revision")
    if git("status", "--porcelain", "--untracked-files=no", cwd=db):
        raise RuntimeError("advisory database has tracked modifications")
    if not (root / "Cargo.lock").is_file() or not (root / "Cargo.toml").is_file():
        raise RuntimeError("software-security supports a committed root Cargo.toml and Cargo.lock only")
    if role == "preflight":
        git("cat-file", "-e", "HEAD:Cargo.lock")
        if git("status", "--porcelain", "--untracked-files=no"):
            raise RuntimeError("commit or stash tracked changes before starting; untracked files are not evaluated")
        agent = params.get("agent", "")
        if agent not in ("pi", "claude", "codex") or not shutil.which(agent):
            raise RuntimeError("configure parameter.agent as installed pi, claude, or codex; authentication stays in its existing configuration")
        if not params.get("model"):
            raise RuntimeError("set parameter.model to a model supported by the configured agent")
        run([*scanner, "--version"])
        output({"ready": True, "surface": "Cargo.lock RustSec advisory matches; not comprehensive security or compliance"})
        return

    artifacts = root / ".newton" / "optimize-artifacts" / request["run_id"] / "security"
    artifacts.mkdir(parents=True, exist_ok=True)
    origin = artifacts / "original.json"
    if not origin.exists():
        origin.write_text(json.dumps({"commit": git("rev-parse", "HEAD")}))
    original = json.loads(origin.read_text())["commit"]
    if git("rev-parse", "HEAD") != original or git("status", "--porcelain", "--untracked-files=no"):
        raise RuntimeError("original project state changed during exploration; reconcile before continuing")
    accepted = request.get("accepted_result")
    base = accepted["candidate"]["artifact_id"] if accepted else original
    candidate_dir = artifacts / f"candidate-{request['cycle']}"

    if role == "prepare":
        if candidate_dir.exists():
            raise RuntimeError("candidate directory already exists; reconcile previous work instead of replaying it")
        git("worktree", "add", "--detach", str(candidate_dir), base)
        prompt = (
            "Improve dependency security in this detached candidate worktree. "
            "Change only Cargo.toml manifests and Cargo.lock. Do not edit tests, evaluator files, "
            "other worktrees, refs, or configuration. Do not publish, merge, or deploy. "
            "Run the project's tests. Newton will independently audit the resulting immutable commit. "
            "The goal is fewer RustSec advisory matches in Cargo.lock. "
            f"Pinned advisory database: {db}; revision: {expected_db}. "
            f"Scanner argv: {json.dumps(scanner)}; test argv: {json.dumps(tests)}. "
            "These instructions are scope guidance, not a sandbox."
        )
        output({"candidate_directory": str(candidate_dir), "prompt": prompt})
        return

    if role == "snapshot":
        if git("rev-parse", "HEAD", cwd=candidate_dir) != base:
            raise RuntimeError("agent changed candidate HEAD; reconcile it before snapshot")
        git("add", "--all", cwd=candidate_dir)
        tree = git("write-tree", cwd=candidate_dir)
        commit = git("-c", "user.name=Newton", "-c", "user.email=newton@localhost",
                     "commit-tree", tree, "-p", base, "-m", "chore: retain optimization candidate", cwd=candidate_dir)
        git("update-ref", f"refs/newton/candidates/{request['candidate_id']}", commit)
        output({"candidate": {"id": request["candidate_id"], "artifact_id": commit,
                "base_artifact_id": base, "created_under_revision": request["requirements_revision"]}})
        return

    if role != "grade":
        raise RuntimeError(f"unsupported adapter role: {role}")
    if request["stage"] == "baseline":
        candidate = accepted["candidate"] if accepted else {
            "id": request["candidate_id"], "artifact_id": original,
            "base_artifact_id": original, "created_under_revision": request["requirements_revision"]}
    else:
        candidate = request["candidate"]
    evaluation_id = str(uuid.uuid4())
    evaluation_dir = artifacts / f"evaluation-{evaluation_id}"
    git("worktree", "add", "--detach", str(evaluation_dir), candidate["artifact_id"])
    audit = run([*scanner, "--json", "--no-fetch", "--no-yanked", "--db", str(db),
                 "--file", str(evaluation_dir / "Cargo.lock")], cwd=artifacts, allowed=(0, 1))
    report = json.loads(audit.stdout)
    vulnerabilities = report["vulnerabilities"]
    count = vulnerabilities["count"]
    if type(count) is not int or count < 0 or not isinstance(vulnerabilities.get("list"), list) or count != len(vulnerabilities["list"]):
        raise RuntimeError("cargo-audit returned an invalid vulnerabilities count/list")
    if (audit.returncode == 0) != (count == 0):
        raise RuntimeError("cargo-audit status disagrees with reported vulnerabilities")
    evidence = artifacts / f"audit-{evaluation_id}.json"
    evidence.write_text(audit.stdout)
    test_result = run(tests, cwd=evaluation_dir, allowed=tuple(range(256)))
    test_log = artifacts / f"tests-{evaluation_id}.txt"
    test_log.write_text(test_result.stdout + test_result.stderr)
    if git("rev-parse", "HEAD", cwd=evaluation_dir) != candidate["artifact_id"] or git("status", "--porcelain", "--untracked-files=no", cwd=evaluation_dir):
        raise RuntimeError("evaluation changed tracked candidate state; evidence is invalid")
    if git("rev-parse", "HEAD", cwd=db) != expected_db or git("status", "--porcelain", "--untracked-files=no", cwd=db):
        raise RuntimeError("advisory inputs changed during evaluation")
    if git("rev-parse", "HEAD") != original or git("status", "--porcelain", "--untracked-files=no"):
        raise RuntimeError("evaluation changed the original project; reconcile before accepting evidence")
    changed = git("diff", "--name-only", candidate["base_artifact_id"], candidate["artifact_id"]).splitlines()
    dependency_only = all(path == "Cargo.lock" or Path(path).name == "Cargo.toml" for path in changed)
    check = lambda passed, refs: {"evaluator": "cargo-audit", "status": "satisfied" if passed else "violated", "evidence": refs}
    evaluation = {
        "id": evaluation_id, "run_id": request["run_id"], "cycle": request["cycle"],
        "candidate_id": candidate["id"], "artifact_id": candidate["artifact_id"],
        "base_artifact_id": candidate["base_artifact_id"],
        "requirements_revision": request["requirements_revision"],
        "evaluator_revisions": {key: value["revision"] for key, value in request["requirements"]["evaluators"].items()},
        "measurements": {"vulnerabilities": {"status": "produced", "measurement": {
            "kind": "numeric", "unit": "affected_package_advisories", "direction": "minimize"}, "samples": [count]}},
        "constraints": {"tests_pass": check(test_result.returncode == 0, [str(test_log), str(evidence), f"{db}@{expected_db}"]),
                        "dependency_files_only": check(dependency_only, changed)},
        "completion_checks": {},
    }
    output({"candidate": candidate, "evaluation": evaluation})


if __name__ == "__main__":
    try:
        main(*sys.argv[1:])
        print("NEWTON_HELPER_COMPLETED", flush=True)
    except Exception as error:
        print(f"software-security: {error}", file=sys.stderr)
        sys.exit(1)
