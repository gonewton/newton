import pathlib
import sys
import time

root = pathlib.Path(sys.argv[1])
(root / "development-ready").write_text("working")
deadline = time.monotonic() + 20
while not (root / "development-release").exists():
    if time.monotonic() >= deadline:
        sys.exit("test did not release development")
    time.sleep(0.02)
print("DEVELOPMENT_COMPLETED")
