#!/bin/sh
set -eu
# Copy configuration into a disposable writable home; Pi also writes sessions
# and model caches. Never mount the operator's live agent directory writable.
mkdir -p "$HOME/.pi/agent"
for file in models.json auth.json settings.json; do
    if [ -f "/pi-config/$file" ]; then
        cp "/pi-config/$file" "$HOME/.pi/agent/$file"
    fi
done
export PI_CODING_AGENT_DIR="$HOME/.pi/agent"
export CARGO_HOME=/tmp/newton-cargo
mkdir -p "$CARGO_HOME"
exec python3 /opt/newton/test-optimize-live.py "$@"
