#!/bin/sh
# Check every commit reachable from one commit. No commit is skipped.
# A git error or a shallow repository fails the check.
# The previous tip, remote tips, and event order are not read.
# Usage: identity-history.sh <commit>
set -eu

root=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
gate=$root/scripts/identity-gate.sh

die() {
  echo "identity-gate: $1" >&2
  exit 1
}

[ "$#" -eq 1 ] || die "usage: identity-history.sh <commit>"
[ -n "$1" ] || die "not a commit:"

commit=$(git rev-parse --verify --quiet --end-of-options "$1^{commit}") ||
  die "not a commit: $1"
shallow=$(git rev-parse --is-shallow-repository) || die "git rev-parse failed"
[ "$shallow" = false ] || die "shallow repository"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git rev-list "$commit" >"$work/all" || die "git rev-list failed"
"$gate" --stdin <"$work/all"
