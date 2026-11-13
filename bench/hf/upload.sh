#!/bin/sh
# Publish the benchmark files as a public Hugging Face dataset.
#
#   REPO_ID=rgoswami/ljos-longmemeval-s HF_TOKEN=... sh bench/hf/upload.sh
#
# The ljos README links that dataset id. REPO_ID picks the dataset.
# HF_TOKEN is a write token. This script does not print it.
# It creates the dataset when it is missing.
#
# Uploads bench/hf/out/, this directory's run.sh, and card.md as README.md.
set -eu
set +x

if [ -z "${REPO_ID:-}" ] || [ -z "${HF_TOKEN:-}" ]; then
  echo "upload.sh: set REPO_ID and HF_TOKEN" >&2
  exit 2
fi

here=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
if [ ! -f "$here/out/summary.json" ] || [ ! -f "$here/out/per_question.csv" ]; then
  echo "upload.sh: out/summary.json and out/per_question.csv are missing" >&2
  exit 1
fi

export BENCH_HF_DIR="$here"
python3 - "$here" <<'PY'
import os
import sys

root = sys.argv[1]
token = os.environ.get("HF_TOKEN", "")
repo = os.environ.get("REPO_ID", "")
if not token or not repo:
    sys.stderr.write("upload.sh: set REPO_ID and HF_TOKEN\n")
    sys.exit(2)

try:
    from huggingface_hub import HfApi
except ImportError:
    sys.stderr.write("upload.sh: python package huggingface_hub is not installed\n")
    sys.exit(1)

api = HfApi(token=token)
try:
    api.create_repo(repo_id=repo, repo_type="dataset", private=False, exist_ok=True)
    api.upload_file(
        path_or_fileobj=os.path.join(root, "card.md"),
        path_in_repo="README.md",
        repo_id=repo,
        repo_type="dataset",
    )
    api.upload_file(
        path_or_fileobj=os.path.join(root, "run.sh"),
        path_in_repo="run.sh",
        repo_id=repo,
        repo_type="dataset",
    )
    api.upload_folder(
        folder_path=os.path.join(root, "out"),
        path_in_repo="out",
        repo_id=repo,
        repo_type="dataset",
    )
except Exception as exc:
    text = str(exc).replace(token, "[token]")
    sys.stderr.write(f"upload.sh: {text}\n")
    sys.exit(1)
sys.stdout.write(f"upload.sh: published {repo}\n")
PY
