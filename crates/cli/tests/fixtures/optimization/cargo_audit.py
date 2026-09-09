"""Deterministic scanner boundary, not evidence of real RustSec coverage."""
import json
from pathlib import Path
import sys

if "--version" in sys.argv:
    print("cargo-audit fixture 1")
else:
    if "--no-yanked" in sys.argv:
        print("obsolete cargo-audit argument: --no-yanked", file=sys.stderr)
        sys.exit(2)
    lock = Path(sys.argv[sys.argv.index("--file") + 1]).read_text()
    vulnerable = "vulnerable" in lock
    print(json.dumps({"vulnerabilities": {
        "count": int(vulnerable), "list": [{"advisory": "fixture"}] if vulnerable else []}}))
    sys.exit(int(vulnerable))
