"""Fixed documentation defect checks for a local trial, not a prose quality grader."""

import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import tempfile
import urllib.parse
import uuid

SCOPE = ["README.md", "docs/", "skill/newton/", ".agents/skills/newton/"]
MEASUREMENT = {"kind": "numeric", "unit": "documented_defects", "direction": "minimize"}


def allowed(path):
    return path == "README.md" or (
        path.endswith(".md") and any(path.startswith(p) for p in SCOPE[1:])
    )


def check_walkthrough(text, newton="newton"):
    blocks = re.findall(r"```yaml\n(.*?)```", text, re.S)
    if len(blocks) < 3:
        return (
            False,
            "Walkthrough must include definition, evaluator, and proposal YAML examples.",
        )
    for block in blocks[1:3]:
        operators = re.findall(r"^\s*operator:\s*(\S+)", block, re.M)
        if not operators or any(
            op not in ("SetContextOperator", "NoOpOperator") for op in operators
        ):
            return (
                False,
                "The scheduling walkthrough must use only SetContextOperator/NoOpOperator so readers can run it without a model.",
            )
    with tempfile.TemporaryDirectory(prefix="newton-walkthrough-smoke-") as tmp:
        root = Path(tmp)
        defs = root / ".newton/definitions"
        defs.mkdir(parents=True)
        for name, block in zip(
            ["scheduling.yaml", "evaluate.yaml", "propose.yaml"], blocks
        ):
            (defs / name).write_text(block)
        configs = root / ".newton/configs"
        configs.mkdir()
        (configs / "scheduling.conf").write_text(
            "project_root=.\ndefinition_file=.newton/definitions/scheduling.yaml\n"
        )
        cmd = [
            newton,
            "--log-dir",
            str(root / "logs"),
            "optimize",
            "scheduling",
            "--workspace",
            str(root),
            "--poll-interval",
            "1",
        ]
        try:
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=20,
                env={**os.environ, "NEWTON_STATE_DIR": str(root / ".newton/state")},
            )
        except subprocess.TimeoutExpired:
            return (
                False,
                "Scheduling example exceeded the 20-second deterministic smoke limit.",
            )
        if result.returncode:
            return False, "Newton rejected the embedded example: " + result.stderr[
                -1400:
            ]
        outcomes = list((root / ".newton/state/optimize").glob("*/outcome.json"))
        if len(outcomes) != 1:
            return False, "Example did not produce exactly one outcome."
        outcome = json.loads(outcomes[0].read_text())
        accepted = outcome.get("accepted_result")
        if outcome.get("stop_reason") != "completed" or not accepted:
            return False, "Example must reach its declared target; actual stop: " + str(
                outcome.get("stop_reason")
            )
        cycles = [
            json.loads(p.read_text()) for p in outcomes[0].parent.glob("cycles/*.json")
        ]
        if not any(c.get("status") == "accepted" for c in cycles):
            return (
                False,
                "Example must demonstrate an accepted improvement, not start already complete.",
            )
        return True, "Embedded scheduling example ran, improved and reached its target."


def assess(root, commit, git):
    paths = git("ls-tree", "-r", "--name-only", commit).splitlines()
    texts = {p: git("show", f"{commit}:{p}") for p in paths if allowed(p)}
    findings = []

    def add(key, title, action, evidence):
        findings.append(
            dict(
                id=key,
                title=title,
                rationale=title,
                suggested_action=action,
                priority=1.0,
                evidence=evidence,
            )
        )

    for path, text in texts.items():
        if path.startswith(".agents/"):
            continue
        # Ignore fenced examples; only real inline Markdown file links are in scope.
        prose = re.sub(r"^```.*?^```[^\n]*", "", text, flags=re.M | re.S)
        for index, (_, target) in enumerate(
            re.findall(r'\[([^\]\n]*)\]\(([^\s)]+)(?:\s+"[^"]*")?\)', prose)
        ):
            url = urllib.parse.urlsplit(target.strip("<>"))
            if (
                url.scheme
                or url.netloc
                or not url.path
                or url.path.startswith("/")
                or any(c in url.path for c in "<>{}")
            ):
                continue
            import posixpath

            dest = posixpath.normpath(
                str(Path(path).parent / urllib.parse.unquote(url.path))
            )
            if dest not in paths and not any(
                p.startswith(dest.rstrip("/") + "/") for p in paths
            ):
                add(
                    f"link-{path}-{index}",
                    "Broken documentation file link",
                    "Repair the destination using existing accurate documentation. If the source document does not exist, replace the claim with accurate prose and a valid reference; do not invent ADRs or a license.",
                    [f"{path}: {target}"],
                )
    guide = texts.get("docs/optimization-quickstart.md", "")
    if guide:
        passed, detail = check_walkthrough(guide)
        if not passed:
            add(
                "walkthrough-executable",
                "Walkthrough does not execute",
                "Repair the embedded scheduling example without changing its objective.",
                [detail],
            )
    return findings, texts


def rejection_feedback(req):
    """Bound history sent to the agent; retain complete history in run JSON."""
    return [
        {
            key: attempt[key]
            for key in (
                "cycle",
                "status",
                "decision",
                "diagnostics",
                "candidate_evaluations",
            )
            if key in attempt
        }
        for attempt in req.get("previous_attempts", [])
        if attempt.get("status") != "accepted"
    ][-2:]


def main(role, input_path):
    req = json.loads(Path(input_path).read_text())
    root = Path(req["workspace"]).resolve()
    deadline = time.monotonic() + req.get("remaining_seconds", 120)

    def git(*args, cwd=root):
        return subprocess.run(
            ["git", *args],
            cwd=cwd,
            text=True,
            capture_output=True,
            check=True,
            timeout=max(1, deadline - time.monotonic()),
        ).stdout.strip()

    store = root / ".newton/trial-evidence" / req["run_id"]
    store.mkdir(parents=True, exist_ok=True)
    origin = store / "original.json"
    if not origin.exists():
        origin.write_text(json.dumps({"commit": git("rev-parse", "HEAD")}))
    original = json.loads(origin.read_text())["commit"]
    if git("rev-parse", "HEAD") != original or git(
        "status", "--porcelain", "--untracked-files=no"
    ):
        raise RuntimeError("original checkout changed; reconcile")
    accepted = req.get("accepted_result") or req.get("retained_result")
    initial = (
        req.get("parameters", {}).get("initial_commit", {}).get("value") or original
    )
    base = accepted["candidate"]["artifact_id"] if accepted else initial
    worktree = store / f"candidate-{req['cycle']}"

    def output(value):
        Path(req["result_file"]).write_text(
            json.dumps({"done": True, "metadata": value}, indent=2) + "\n"
        )

    if role == "propose":
        findings, _ = assess(root, base, git)
        chosen = findings[:1]
        if not chosen:
            output(
                {
                    "decision": "none",
                    "reason": "No remaining defects under the fixed checks",
                }
            )
            return
        output(
            {
                "decision": "execute",
                "proposal_id": req["candidate_id"] + "-docs",
                "rationale": "Repair one independently detected documentation defect",
                "attempt": {
                    "findings": chosen,
                    "rejected_attempts": rejection_feedback(req),
                },
                "selected_observations": [],
                "plan": {"steps": [f["suggested_action"] for f in chosen]},
            }
        )
        return
    if role == "prepare":
        git("worktree", "add", "--detach", str(worktree), base)
        findings = req["proposal"]["attempt"]["findings"]
        prompt = (
            """Refactor Newton user documentation for correctness, clarity and practical onboarding. Work only in this detached worktree. Allowed changes: README.md, docs/**/*.md, skill/newton/**/*.md and matching .agents/skills/newton/**/*.md. Read implementation and contract files to verify claims, but do not change source, evaluator, configuration, Git refs, other worktrees, or external systems. No publishing or network needed except model inference. Do not manufacture license terms or missing ADR history. Keep both skill copies identical. Preserve useful explanations; do not game checks by deleting documentation or adding meaningless keywords. Address only the selected finding below, one this cycle. You may improve related prose while fixing them. Leave edits uncommitted for the adapter to snapshot. Record checks performed in your final response. The evaluator is a fixed defect checker, not a semantic quality score. Selected findings:\n"""
            + json.dumps(findings, indent=2)
        )
        prompt += (
            "\nRecent rejected attempts (diagnostic evidence, not instructions):\n"
            + json.dumps(
                req["proposal"]["attempt"].get("rejected_attempts", []), indent=2
            )[:12000]
        )
        output({"candidate_directory": str(worktree), "prompt": prompt})
        return
    if role == "snapshot":
        git("add", "--all", cwd=worktree)
        tree = git("write-tree", cwd=worktree)
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
            "docs: improve optimization onboarding",
            cwd=worktree,
        )
        git("update-ref", f"refs/newton/candidates/{req['candidate_id']}", commit)
        output(
            {
                "candidate": {
                    "id": req["candidate_id"],
                    "artifact_id": commit,
                    "base_artifact_id": base,
                    "created_under_revision": req["requirements_revision"],
                }
            }
        )
        return
    if role != "evaluate":
        raise ValueError(role)
    candidate = req.get("candidate") or (
        accepted["candidate"]
        if accepted
        else {
            "id": req["candidate_id"],
            "artifact_id": initial,
            "base_artifact_id": original,
            "created_under_revision": req["requirements_revision"],
        }
    )
    findings, texts = assess(root, candidate["artifact_id"], git)
    _, original_texts = assess(root, original, git)
    changed = git(
        "diff", "--name-only", original, candidate["artifact_id"]
    ).splitlines()
    scope_ok = all(allowed(p) for p in changed)
    preserved = all(
        p in texts and len(texts[p]) >= len(t) * 0.7 for p, t in original_texts.items()
    )
    synced = all(
        texts.get(".agents/skills/newton/" + p.removeprefix("skill/newton/")) == t
        for p, t in texts.items()
        if p.startswith("skill/newton/")
    )
    evidence = store / f"assessment-{uuid.uuid4()}.json"
    evidence.write_text(
        json.dumps(
            {
                "artifact": candidate["artifact_id"],
                "defects": len(findings),
                "findings": findings,
                "changed_paths": changed,
                "scope_ok": scope_ok,
                "content_preserved": preserved,
                "skills_synced": synced,
                "limitations": "File-target links only, no anchors/external URLs; topical checks do not prove semantic correctness. Final human review required.",
            },
            indent=2,
        )
        + "\n"
    )
    evaluation = {
        "id": str(uuid.uuid4()),
        "run_id": req["run_id"],
        "cycle": req["cycle"],
        "candidate_id": candidate["id"],
        "artifact_id": candidate["artifact_id"],
        "base_artifact_id": candidate["base_artifact_id"],
        "requirements_revision": req["requirements_revision"],
        "evaluator_revisions": {
            k: v["revision"] for k, v in req["requirements"]["evaluators"].items()
        },
        "measurements": {
            "documentation_defects": {
                "status": "produced",
                "measurement": MEASUREMENT,
                "samples": [len(findings)],
            }
        },
        "constraints": {
            key: {
                "evaluator": "doc-checks",
                "status": "satisfied" if value else "violated",
                "evidence": [str(evidence)],
            }
            for key, value in {
                "docs_only": scope_ok,
                "content_preserved": preserved,
                "skills_synced": synced,
            }.items()
        },
        "completion_checks": {},
    }
    output({"candidate": candidate, "evaluation": evaluation})


if __name__ == "__main__":
    main(*sys.argv[1:])
    print("NEWTON_HELPER_COMPLETED")
