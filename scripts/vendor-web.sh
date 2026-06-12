#!/usr/bin/env bash
# Regenerate the embedded Newton UI bundle served by `newton serve`.
#
# The UI lives in a SEPARATE repo (gonewton/newton-ui). Its production build is a
# single self-contained `index.html` (vite-plugin-singlefile inlines all JS/CSS).
# We vendor a gzip-compressed copy into this repo and `include_bytes!` it from
# crates/core/src/api/mod.rs, so a single `newton` binary serves the whole UI with
# no runtime dependency on the UI repo.
#
# Run this whenever the UI changes. CI does not rebuild it automatically.
#
# Usage:
#   scripts/vendor-web.sh [path-to-newton-ui]
#
# Defaults to ../newton-ui relative to this repo root.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ui_repo="${1:-${repo_root}/../newton-ui}"
out="${repo_root}/crates/core/assets/web/index.html.gz"

if [[ ! -d "${ui_repo}" ]]; then
  echo "error: newton-ui repo not found at ${ui_repo}" >&2
  echo "       pass its path explicitly: scripts/vendor-web.sh /path/to/newton-ui" >&2
  exit 1
fi

echo "==> Building @newton/web in ${ui_repo}"
( cd "${ui_repo}" && pnpm --filter @newton/web build )

dist="${ui_repo}/apps/web/dist/index.html"
if [[ ! -f "${dist}" ]]; then
  echo "error: expected single-file build at ${dist} (is vite-plugin-singlefile enabled?)" >&2
  exit 1
fi

# Guard against a multi-file build sneaking in: a self-contained bundle has no
# external src=/href= asset references.
if grep -oE '(src|href)="[^"]+"' "${dist}" | grep -vE '"(data:|#|/)"?' | grep -qE '\.(js|css)"'; then
  echo "error: build is not self-contained (external js/css refs found in ${dist})" >&2
  exit 1
fi

mkdir -p "$(dirname "${out}")"
gzip -9 -c "${dist}" > "${out}"

raw=$(wc -c < "${dist}")
gz=$(wc -c < "${out}")
echo "==> Vendored ${out}"
echo "    raw ${raw} bytes -> gzip ${gz} bytes"
