#!/usr/bin/env bash
# Regenerate (or verify) the embedded Newton UI bundle served by `newton serve`.
#
# The UI lives in a SEPARATE repo (gonewton/newton-ui). Its production build is a
# single self-contained `index.html` (vite-plugin-singlefile inlines all JS/CSS).
# We vendor a gzip-compressed copy into this repo and `include_bytes!` it from
# crates/core/src/api/mod.rs, so a single `newton` binary serves the whole UI with
# no runtime dependency on the UI repo.
#
# Run this whenever the UI changes. CI does not rebuild it automatically (the UI
# repo is private); the `--check` mode below is wired into CI behind a token so
# drift is caught when the secret is configured. The vite build is deterministic,
# so `--check` compares the freshly built HTML against the committed bundle and
# is not flaky.
#
# Usage:
#   scripts/vendor-web.sh [--check] [path-to-newton-ui]
#
#   (default)  Build the UI and (re)write the vendored gzip bundle.
#   --check    Build the UI and FAIL if the committed bundle is stale, without
#              modifying it. Intended for CI and pre-PR verification.
#
# newton-ui path defaults to ../newton-ui relative to this repo root.
set -euo pipefail

check_only=false
ui_repo=""
for arg in "$@"; do
  case "$arg" in
    --check) check_only=true ;;
    -*) echo "error: unknown flag '$arg'" >&2; exit 2 ;;
    *) ui_repo="$arg" ;;
  esac
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ui_repo="${ui_repo:-${repo_root}/../newton-ui}"
out="${repo_root}/crates/core/assets/web/index.html.gz"

if [[ ! -d "${ui_repo}" ]]; then
  echo "error: newton-ui repo not found at ${ui_repo}" >&2
  echo "       pass its path explicitly: scripts/vendor-web.sh [--check] /path/to/newton-ui" >&2
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

if [[ "${check_only}" == true ]]; then
  # The vite build is deterministic, so the committed bundle must decompress to
  # exactly the freshly built HTML. Compare decompressed content (gzip framing
  # itself is irrelevant) so the check never flakes on compression metadata.
  if [[ ! -f "${out}" ]]; then
    echo "error: vendored bundle missing at ${out}; run scripts/vendor-web.sh" >&2
    exit 1
  fi
  if cmp -s <(gzip -dc "${out}") "${dist}"; then
    echo "==> OK: embedded UI bundle is up to date with newton-ui"
    exit 0
  fi
  echo "error: embedded UI bundle is STALE — newton-ui changed without re-vendoring." >&2
  echo "       Run: scripts/vendor-web.sh ${ui_repo}" >&2
  echo "       and commit crates/core/assets/web/index.html.gz" >&2
  exit 1
fi

mkdir -p "$(dirname "${out}")"
gzip -9 -c "${dist}" > "${out}"

raw=$(wc -c < "${dist}")
gz=$(wc -c < "${out}")
echo "==> Vendored ${out}"
echo "    raw ${raw} bytes -> gzip ${gz} bytes"
