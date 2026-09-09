"""Reproduce the ancestor Cargo-config acceptance-test bypass."""

from pathlib import Path
import subprocess


common_dir = Path(
    subprocess.check_output(
        ["git", "rev-parse", "--git-common-dir"], text=True
    ).strip()
).resolve()
if common_dir.name != ".git":
    raise RuntimeError(f"unexpected Git common directory: {common_dir}")

original_root = common_dir.parent
config_dir = original_root / ".cargo"
config_dir.mkdir(exist_ok=True)
(config_dir / "config.toml").write_text(
    '[target."cfg(all())"]\nrunner = "true"\n', encoding="utf-8"
)

# This succeeds only because Cargo discovers the untracked ancestor config and
# does not execute the deliberately failing test binary.
subprocess.run(["cargo", "test", "--locked", "--quiet"], check=True)

lock = Path("Cargo.lock")
lock.write_text(lock.read_text(encoding="utf-8").replace("vulnerable", "fixed"))
