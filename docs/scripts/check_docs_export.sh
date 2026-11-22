#!/usr/bin/env bash
# Export org → rst like the docs workflow, then refuse rst Sphinx cannot build.
# Uses a detached worktree so export does not rewrite the caller's checkout.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

required="${CHECK_DOCS_EXPORT_REQUIRED:-0}"
missing=()
command -v pandoc >/dev/null 2>&1 || missing+=(pandoc)
command -v sphinx-build >/dev/null 2>&1 || missing+=(sphinx-build)
if ((${#missing[@]} > 0)); then
  if [[ "$required" == 1 ]]; then
    echo "check_docs_export: need ${missing[*]} on PATH (set CHECK_DOCS_EXPORT_REQUIRED=0 to skip)" >&2
    exit 1
  fi
  echo "check_docs_export: skip (${missing[*]} not on PATH)" >&2
  exit 0
fi

head_ref="$(git rev-parse HEAD)"
if [[ -n "${DOCS_EXPORT_COMPARE_BASE:-}" ]]; then
  compare_base="$DOCS_EXPORT_COMPARE_BASE"
  if ! git rev-parse --verify "${compare_base}^{commit}" >/dev/null 2>&1; then
    echo "check_docs_export: DOCS_EXPORT_COMPARE_BASE $compare_base is missing; fetch it and retry" >&2
    exit 1
  fi
else
  if [[ "$(git rev-parse --is-shallow-repository)" == true ]]; then
    if ! git fetch --quiet --no-tags --unshallow origin; then
      echo "check_docs_export: could not fetch the shallow checkout history from origin" >&2
      exit 1
    fi
  fi
  if ! git rev-parse --verify origin/develop >/dev/null 2>&1; then
    if ! git fetch --quiet --no-tags origin develop:refs/remotes/origin/develop; then
      echo "check_docs_export: could not fetch origin/develop; set DOCS_EXPORT_COMPARE_BASE" >&2
      exit 1
    fi
  fi
  if ! compare_base="$(git merge-base HEAD origin/develop)"; then
    echo "check_docs_export: HEAD and origin/develop have no common commit" >&2
    exit 1
  fi
fi
if [[ "${1:-}" == "--resolve-base" ]]; then
  printf '%s\n' "$compare_base"
  exit 0
elif (($# > 0)); then
  echo "check_docs_export: unknown argument $1" >&2
  exit 1
fi

wt="$(mktemp -d)"
base_wt="$(mktemp -d)"
cleanup() {
  git -C "$ROOT" worktree remove --force "$wt" 2>/dev/null || rm -rf "$wt"
  git -C "$ROOT" worktree remove --force "$base_wt" 2>/dev/null || rm -rf "$base_wt"
}
trap cleanup EXIT

git -C "$ROOT" worktree add --detach "$wt" HEAD >/dev/null
git -C "$ROOT" worktree add --detach "$base_wt" "$compare_base" >/dev/null
PYTHONPATH=docs/scripts python3 -m unittest docs/scripts/test_check_export_sync.py docs/scripts/test_gen_samples.py
cd "$wt"

export PYTHONDONTWRITEBYTECODE=1
python3 -m pip install --disable-pip-version-check -q -r docs/requirements.txt
export VISSUE_DOC_EXPORTER=pandoc
bash docs/scripts/export_org_to_rst.sh
(
  cd "$base_wt"
  export PYTHONDONTWRITEBYTECODE=1
  export VISSUE_DOC_EXPORTER=pandoc
  bash docs/scripts/export_org_to_rst.sh
)
python3 "$ROOT/docs/scripts/check_export_sync.py" \
  --root "$ROOT" \
  --base-ref "$compare_base" \
  --head-ref "$head_ref" \
  --base-export "$base_wt/docs/source" \
  --head-export "$wt/docs/source"
rm -rf docs/build
sphinx-build -b html -n -W docs/source docs/build >/dev/null
echo "check_docs_export: RST comparison, pandoc export and sphinx -W ok"