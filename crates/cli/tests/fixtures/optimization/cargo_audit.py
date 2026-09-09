"""Deterministic scanner boundary, not evidence of real RustSec coverage."""
import json
from pathlib import Path
import sys

if "--version" in sys.argv:
    print("cargo-audit fixture 1")
else:
    lock = Path(sys.argv[sys.argv.index("--file") + 1]).read_text()
    vulnerable = "vulnerable" in lock
    print(json.dumps({"vulnerabilities": {
        "count": int(vulnerable), "list": [{"advisory": "fixture"}] if vulnerable else []}}))
    sys.exit(int(vulnerable))
