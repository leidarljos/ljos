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

wt="$(mktemp -d)"
cleanup() {
  git -C "$ROOT" worktree remove --force "$wt" 2>/dev/null || rm -rf "$wt"
}
trap cleanup EXIT

git -C "$ROOT" worktree add --detach "$wt" HEAD >/dev/null
cd "$wt"

export PYTHONDONTWRITEBYTECODE=1
python3 -m pip install --disable-pip-version-check -q -r docs/requirements.txt
export VISSUE_DOC_EXPORTER=pandoc
bash docs/scripts/export_org_to_rst.sh
rm -rf docs/build
sphinx-build -b html -n -W docs/source docs/build >/dev/null
echo "check_docs_export: pandoc export and sphinx -W ok"