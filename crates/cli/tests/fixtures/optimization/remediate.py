"""Deterministic agent boundary; records cwd through its actual file mutation."""
from pathlib import Path
import subprocess

lock = Path("Cargo.lock")
lock.write_text(lock.read_text().replace("vulnerable", "fixed"))
manifest = Path("Cargo.toml")
manifest.write_text(
    manifest.read_text() + "\n[dependencies]\nfixture-safe-dependency = \"1\"\n"
)
subprocess.run(["git", "add", "Cargo.toml", "Cargo.lock"], check=True)
subprocess.run([
    "git", "-c", "user.name=Fixture Agent", "-c",
    "user.email=fixture-agent@example.invalid", "commit", "-m",
    "fix: remediate dependency",
], check=True)
print("fixture remediation completed")
