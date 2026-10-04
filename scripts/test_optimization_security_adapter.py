"""Exercise the shipped evaluator role and immutable Git input contract."""

import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

TEMPLATE = Path(__file__).resolve().parents[1] / "resources/newton-template/newton/definitions/software-security"


class SecurityAdapterTests(unittest.TestCase):
    def test_shipped_role_evaluates_retained_artifact_after_revision(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            repo, db = root / "repo", root / "db"

            def git(path, *args):
                return subprocess.check_output(
                    ["git", "-C", str(path), *args], text=True, stderr=subprocess.PIPE
                ).strip()

            for path in (repo, db):
                path.mkdir()
                git(path, "init")
                git(path, "config", "user.name", "Test")
                git(path, "config", "user.email", "test@example.invalid")
            (repo / "Cargo.toml").write_text('[package]\nname="fixture"\nversion="0.1.0"\n')
            (repo / "Cargo.lock").write_text("version = 4\n")
            git(repo, "add", ".")
            git(repo, "commit", "-m", "baseline")
            original = git(repo, "rev-parse", "HEAD")
            (repo / "Cargo.lock").write_text("version = 4\n# retained dependency state\n")
            git(repo, "add", "Cargo.lock")
            git(repo, "commit", "-m", "retained")
            retained = git(repo, "rev-parse", "HEAD")
            git(repo, "checkout", "--detach", original)
            (db / "README").write_text("fixture advisory database")
            git(db, "add", ".")
            git(db, "commit", "-m", "database")
            candidate = {"id": "retained", "artifact_id": retained, "base_artifact_id": original, "created_under_revision": 1}
            request = {
                "workspace": str(repo), "run_id": "test-run", "cycle": 2,
                "candidate_id": "retained", "candidate": candidate,
                "accepted_result": None,
                "retained_result": {"candidate": candidate},
                "stage": "baseline", "requirements_revision": 2,
                "requirements": {"evaluators": {"cargo-audit": {"revision": "v2"}}},
                "remaining_seconds": 30, "result_file": str(root / "result.json"),
                "parameters": {name: {"kind": "literal", "value": value} for name, value in {
                    "advisory_db": str(db), "advisory_db_revision": git(db, "rev-parse", "HEAD"),
                    "scanner_command": [sys.executable, "-c", 'print(\'{"vulnerabilities":{"count":0,"list":[]}}\')'],
                    "test_command": [sys.executable, "-c", 'from pathlib import Path; assert "retained dependency state" in Path("Cargo.lock").read_text()'],
                }.items()},
            }
            request_path = root / "request.json"
            request_path.write_text(json.dumps(request))
            # Invoke the exact helper role authored in the shipped workflow.
            role = re.search(r"^          - (grade|evaluate)$", (TEMPLATE / "grade.yaml").read_text(), re.M).group(1)
            result = subprocess.run([sys.executable, str(TEMPLATE / "security.py"), role, str(request_path)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            output = json.loads((root / "result.json").read_text())["metadata"]
            self.assertEqual(output["candidate"], candidate)
            self.assertEqual(output["evaluation"]["constraints"]["tests_pass"]["status"], "satisfied")
            self.assertEqual(git(repo, "rev-parse", "HEAD"), original)
            self.assertEqual(git(repo, "worktree", "list", "--porcelain").count("worktree "), 1)


if __name__ == "__main__":
    unittest.main()
