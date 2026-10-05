"""Exercise the trial adapter's public evaluator and proposal protocol."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

FIXTURE = Path(__file__).parent / "fixtures/optimization-docs"
SPEC = importlib.util.spec_from_file_location("docs_trial", FIXTURE / "adapter.py")
ADAPTER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ADAPTER)


class DocsTrialTests(unittest.TestCase):
    def test_nested_example_uses_its_own_state(self):
        seen = []

        def run(command, **kwargs):
            root = Path(command[command.index("--workspace") + 1])
            state = Path(kwargs["env"]["NEWTON_STATE_DIR"])
            self.assertEqual(state, root / ".newton/state")
            seen.append(state)
            run_dir = state / "optimize/example"
            (run_dir / "cycles").mkdir(parents=True)
            (run_dir / "outcome.json").write_text(
                json.dumps(
                    {"stop_reason": "completed", "accepted_result": {"candidate": {}}}
                )
            )
            (run_dir / "cycles/1.json").write_text('{"status":"accepted"}')
            return subprocess.CompletedProcess(command, 0, "", "")

        with tempfile.TemporaryDirectory() as parent:
            sentinel = Path(parent) / "sentinel"
            sentinel.write_text("unchanged")
            with (
                patch.dict(os.environ, {"NEWTON_STATE_DIR": parent}),
                patch.object(ADAPTER.subprocess, "run", side_effect=run),
            ):
                passed, detail = ADAPTER.check_walkthrough(
                    (FIXTURE / "content/docs/optimization-quickstart.md").read_text()
                )
            self.assertTrue(passed, detail)
            self.assertEqual(list(Path(parent).iterdir()), [sentinel])
            self.assertEqual(sentinel.read_text(), "unchanged")
            self.assertEqual(len(seen), 1)

    def test_proposal_preserves_recent_rejection_diagnostics(self):
        attempts = [
            {"cycle": n, "status": "rejected", "diagnostics": [f"failed check {n}"]}
            for n in range(1, 5)
        ]
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            subprocess.run(["git", "init", str(root)], check=True, capture_output=True)
            (root / "README.md").write_text("[Broken](missing.md)")
            subprocess.run(["git", "-C", str(root), "add", "."], check=True)
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(root),
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@localhost",
                    "commit",
                    "-m",
                    "fixture",
                ],
                check=True,
                capture_output=True,
            )
            request = root / "request.json"
            result = root / "result.json"
            request.write_text(
                json.dumps(
                    {
                        "workspace": str(root),
                        "run_id": "test",
                        "cycle": 5,
                        "candidate_id": "next",
                        "result_file": str(result),
                        "previous_attempts": attempts,
                    }
                )
            )
            ADAPTER.main("propose", str(request))
            proposal = json.loads(result.read_text())["metadata"]
            self.assertEqual(proposal["decision"], "execute")
            feedback = proposal["attempt"]["rejected_attempts"]
            self.assertEqual([a["cycle"] for a in feedback], [3, 4])
            self.assertEqual(feedback[-1]["diagnostics"], ["failed check 4"])
