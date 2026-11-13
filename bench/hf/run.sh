#!/bin/sh
# Fetch the cleaned LongMemEval_S file pinned by longmemeval_s_recall and
# run that scorer. BM25+ on one CPU. No encoder and no model download.
#
# From a checkout of https://github.com/leidarljos/ljos :
#
#   sh bench/hf/run.sh
#
# Writes bench/hf/out/summary.json and bench/hf/out/per_question.csv.
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
cd "$root"
src=scripts/longmemeval_s_recall/src/main.rs
if [ ! -f "$src" ]; then
  echo "run.sh: run this from a checkout of https://github.com/leidarljos/ljos" >&2
  exit 1
fi

repo=$(sed -n 's/^const DATA_REPO: &str = "\([^"]*\)";$/\1/p' "$src")
rev=$(sed -n 's/^const DATA_REV: &str = "\([^"]*\)";$/\1/p' "$src")
file=$(sed -n 's/^const DATA_FILE: &str = "\([^"]*\)";$/\1/p' "$src")
pin=$(sed -n 's/^const DATA_SHA256: &str = "\([^"]*\)";$/\1/p' "$src")
if [ -z "$repo" ] || [ -z "$rev" ] || [ -z "$file" ] || [ -z "$pin" ]; then
  echo "run.sh: the scorer is missing a dataset pin" >&2
  exit 1
fi

url="https://huggingface.co/datasets/${repo}/resolve/${rev}/${file}"
scratch=${LJOS_LME_SCRATCH:-${TMPDIR:-/tmp}}
export LJOS_LME_SCRATCH="$scratch"
dest="$scratch/ljos-longmemeval-s/$file"

file_sha() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

if [ ! -f "$dest" ] || [ "$(file_sha "$dest")" != "$pin" ]; then
  echo "fetching ${repo} ${rev}" >&2
  mkdir -p "$(dirname "$dest")"
  partial="$dest.partial"
  if ! command -v curl >/dev/null 2>&1; then
    echo "run.sh: curl is required to fetch the dataset" >&2
    exit 1
  fi
  curl -fsSL --retry 3 --retry-delay 2 -o "$partial" "$url"
  got=$(file_sha "$partial")
  if [ "$got" != "$pin" ]; then
    rm -f "$partial"
    echo "run.sh: sha256 ${got} does not match the pin" >&2
    exit 1
  fi
  mv "$partial" "$dest"
  echo "dataset sha256 matches the pin" >&2
else
  echo "dataset sha256 matches the pin" >&2
fi

out=bench/hf/out
mkdir -p "$out"
echo "scoring ${repo} ${rev}" >&2
cargo run --release --locked -p longmemeval_s_recall -- --out "$out"
