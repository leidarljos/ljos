#!/bin/sh
# A clean range passes. A wrong author, a wrong committer and a trailer
# each fail. A foreign commit that sits before the range is not judged,
# including one that is already on another remote-tracking branch.
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
gate=$root/scripts/identity-gate.sh

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
repo=$tmp/repo
git init -q -b main "$repo"
git -C "$repo" config user.name "Rohit Goswami"
git -C "$repo" config user.email "rgoswami@ieee.org"
git -C "$repo" config commit.gpgsign false
git -C "$repo" config core.hooksPath /dev/null

export GIT_AUTHOR_NAME="Rohit Goswami"
export GIT_AUTHOR_EMAIL="rgoswami@ieee.org"
export GIT_COMMITTER_NAME="Rohit Goswami"
export GIT_COMMITTER_EMAIL="rgoswami@ieee.org"

cd "$repo"
git commit --allow-empty -q -m "base"
base=$(git rev-parse HEAD)

git commit --allow-empty -q -m "clean work"
"$gate" "$base..HEAD"

tip=$(git rev-parse HEAD)
"$gate" "$tip..$tip"

# The foreign commit is the parent of the range, so it stays out.
git checkout -q -b later "$base"
GIT_AUTHOR_NAME="Ada Lovelace" \
  GIT_AUTHOR_EMAIL="ada@example.com" \
  git commit --allow-empty -q -m "older foreign"
old=$(git rev-parse HEAD)
git commit --allow-empty -q -m "after"
"$gate" "$old..HEAD"

must_fail() {
  desc=$1
  shift
  if "$@" >/dev/null 2>&1; then
    echo "identity-gate-test: $desc was accepted" >&2
    exit 1
  fi
}

git checkout -q -b author "$base"
GIT_AUTHOR_NAME="Ada Lovelace" \
  GIT_AUTHOR_EMAIL="ada@example.com" \
  git commit --allow-empty -q -m "foreign author"
must_fail "wrong author" "$gate" "$base..HEAD"

git checkout -q -b committer "$base"
GIT_COMMITTER_NAME="Ada Lovelace" \
  GIT_COMMITTER_EMAIL="ada@example.com" \
  git commit --allow-empty -q -m "foreign committer"
must_fail "wrong committer" "$gate" "$base..HEAD"

fail_msg() {
  desc=$1
  shift
  git checkout -q -B msg "$base"
  git commit --allow-empty -q "$@"
  must_fail "$desc" "$gate" "$base..HEAD"
}

fail_msg "Co-authored-by trailer" \
  -m "note" -m "Co-authored-by: Ada Lovelace <ada@example.com>"
fail_msg "Signed-off-by trailer" \
  -m "note" -m "Signed-off-by: Rohit Goswami <rgoswami@ieee.org>"
fail_msg "generated line" \
  -m "note" -m "Generated with a tool"
fail_msg "agent account line" \
  -m "note" -m "Cursor Agent wrote this"
fail_msg "account token" \
  -m "note" -m "cursoragent"

# New branch: history already on another remote-tracking branch is out.
git checkout -q -B develop "$base"
GIT_AUTHOR_NAME="Ada Lovelace" \
  GIT_AUTHOR_EMAIL="ada@example.com" \
  git commit --allow-empty -q -m "already on develop"
git commit --allow-empty -q -m "develop tip"
devtip=$(git rev-parse HEAD)
git checkout -q -B fresh "$devtip"
git commit --allow-empty -q -m "new branch work"
git update-ref refs/remotes/origin/develop "$devtip"
git update-ref refs/remotes/origin/fresh HEAD
# origin/HEAD follows this branch. It must not hide the new commits.
git symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/fresh
"$gate" HEAD --not \
  --exclude="origin/fresh" \
  --exclude="origin/HEAD" \
  --remotes=origin

git commit --allow-empty -q -m "note" \
  -m "Co-authored-by: Ada Lovelace <ada@example.com>"
git update-ref refs/remotes/origin/fresh HEAD
must_fail "new branch trailer" "$gate" HEAD --not \
  --exclude="origin/fresh" \
  --exclude="origin/HEAD" \
  --remotes=origin

must_fail "missing range" "$gate"

echo "identity-gate-test: ok"
