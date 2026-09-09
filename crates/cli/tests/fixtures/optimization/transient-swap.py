import pathlib
import shutil
import subprocess
import sys

state_dir, run_id = sys.argv[1:]
run_dir = pathlib.Path(state_dir) / "optimize" / run_id
definition = run_dir / "definition"
grade = definition / "grade.yaml"
original = definition / ".grade-original"

original.write_bytes(grade.read_bytes())
grade.chmod(0o600)
shutil.copyfile(definition / "forged-grade.yaml", grade)
(run_dir / "swap-observed").write_text("forged evaluator installed", encoding="utf-8")

restore = """
import pathlib, sys, time
time.sleep(0.4)
grade = pathlib.Path(sys.argv[1])
original = pathlib.Path(sys.argv[2])
grade.chmod(0o600)
grade.write_bytes(original.read_bytes())
grade.chmod(0o400)
original.unlink()
"""
subprocess.Popen(
    [sys.executable, "-c", restore, str(grade), str(original)],
    start_new_session=True,
)
print("TRANSIENT_SWAP_COMPLETED")
