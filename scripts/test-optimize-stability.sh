#!/usr/bin/env bash
# Three fresh sequential trials against one immutable image/fixture/model.
set -euo pipefail
if [[ $# != 1 ]]; then
  echo "usage: NEWTON_LIVE_MODEL=provider/model $0 NEW_EVIDENCE_DIRECTORY" >&2
  exit 2
fi
: "${NEWTON_LIVE_MODEL:?set exact Pi provider/model}"
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
evidence=$(realpath -m "$1")
mkdir "$evidence"
image=${NEWTON_OPTIMIZE_LIVE_IMAGE:-newton-optimize-live:local}
image_id=$(docker image inspect --format '{{.Id}}' "$image")
export NEWTON_OPTIMIZE_LIVE_IMAGE=$image_id NEWTON_LIVE_SKIP_BUILD=1
fixture_hash=$(find "$repo_root/scripts/fixtures/optimization-docs" -type f ! -path '*/__pycache__/*' -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -d ' ' -f 1)
python3 - "$evidence/manifest.json" "$image_id" "$fixture_hash" "$NEWTON_LIVE_MODEL" <<'PY'
import json,sys
from pathlib import Path
Path(sys.argv[1]).write_text(json.dumps(dict(image=sys.argv[2], fixture_sha256=sys.argv[3], model=sys.argv[4]),indent=2)+'\n')
PY
for number in 1 2 3; do
  current_hash=$(find "$repo_root/scripts/fixtures/optimization-docs" -type f ! -path '*/__pycache__/*' -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -d ' ' -f 1)
  [[ "$current_hash" == "$fixture_hash" ]] || { echo 'Fixture changed: restart the series.' >&2; exit 1; }
  workspace="$evidence/repository-$number"
  python3 "$repo_root/scripts/prepare-optimize-docs-trial.py" "$workspace" --model "$NEWTON_LIVE_MODEL"
  "$repo_root/scripts/test-optimize-live-docker.sh" "$workspace" trial "$evidence/run-$number" \
    --full-loop --expected-model "$NEWTON_LIVE_MODEL" --route local-gateway \
    --pi-models-file /tmp/newton-home/.pi/agent/models.json
  python3 - "$evidence/run-$number/trial/report.json" "$workspace" <<'PY'
import json,subprocess,sys
from pathlib import Path
report=json.loads(Path(sys.argv[1]).read_text())
assert report['status']=='passed'
assert report['outcome']['stop_reason'] in ('completed','no_progress','resource_limit','no_actionable_work')
assert report['original_head']==report['final_head']
assert not subprocess.check_output(['git','-C',sys.argv[2],'status','--porcelain','--untracked-files=no']).strip()
commit=report['outcome']['accepted_result']['candidate']['artifact_id']
subprocess.run(['git','-C',sys.argv[2],'branch','trial/accepted',commit],check=True)
print('PASS',sys.argv[2],commit)
PY
done
echo "Three consecutive trials passed: $evidence"
