"""Deterministic agent boundary; records cwd through its actual file mutation."""
from pathlib import Path

lock = Path("Cargo.lock")
lock.write_text(lock.read_text().replace("vulnerable", "fixed"))
print("fixture remediation completed")
