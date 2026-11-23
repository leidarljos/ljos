#!/bin/sh
# A clean range passes. A wrong author, a wrong committer, and a
# trailer each fail. A committer date that differs from the author
# date passes. A foreign commit that sits before the range is not
# judged. A merge or squash GitHub commits for Rohit passes; one that
# brings a foreign commit or a trailer fails, and so does an agent or bot
# author or committer. scripts/identity-history.sh checks every commit
# reachable from one commit and skips none. No exclusion list exists. A
# bad commit fails for a branch, a tag and a pull request. A later clean
# commit still fails when a [skip ci] bad commit is reachable, also after
# a commit that names it in a list. Deleting and recreating branches
# does not change the result. A release merge passes. One bad commit on
# main and develop fails on both.
set -eu

root=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
gate=$root/scripts/identity-gate.sh

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
repo=$tmp/repo
git init -q -b main "$repo"
git -C "$repo" config user.name "Rohit Goswami"
git -C "$repo" config user.email "rgoswami@ieee.org"
git -C "$repo" config commit.gpgsign false
git -C "$repo" config tag.gpgsign false
git -C "$repo" config advice.detachedHead false
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

git checkout -q -b other-github "$base"
GIT_COMMITTER_NAME="GitHub" \
  GIT_COMMITTER_EMAIL="other@github.com" \
  git commit --allow-empty -q -m "non-webflow github committer"
must_fail "other github committer" "$gate" "$base..HEAD"

git checkout -q -b webflow "$base"
git commit --allow-empty -q -m "pr head"
GIT_COMMITTER_NAME="GitHub" \
  GIT_COMMITTER_EMAIL="noreply@github.com" \
  git commit --allow-empty -q -m "Merge pull request #9 from leidarljos/fix"
"$gate" "$base..HEAD"

git checkout -q -b webflow-wrong-committer-name "$base"
GIT_COMMITTER_NAME="Someone" \
  GIT_COMMITTER_EMAIL="noreply@github.com" \
  git commit --allow-empty -q -m "Merge pull request #11 from leidarljos/fix"
must_fail "noreply github committer without exact GitHub name" "$gate" "$base..HEAD"

git checkout -q -b webflow-foreign-author "$base"
GIT_AUTHOR_NAME="Ada Lovelace" \
  GIT_AUTHOR_EMAIL="ada@example.com" \
  GIT_COMMITTER_NAME="GitHub" \
  GIT_COMMITTER_EMAIL="noreply@github.com" \
  git commit --allow-empty -q -m "webflow merge for foreign author"
must_fail "webflow committer with non-Rohit author" "$gate" "$base..HEAD"

git checkout -q -b webflow-trailer "$base"
GIT_COMMITTER_NAME="GitHub" \
  GIT_COMMITTER_EMAIL="noreply@github.com" \
  git commit --allow-empty -q -m "Merge pull request #10" \
  -m "Co-authored-by: Ada Lovelace <ada@example.com>"
must_fail "webflow committer with trailer" "$gate" "$base..HEAD"

# The main push of 1dd43134: main is behind, and the push brings a
# two-parent merge GitHub made of a pull request branch.
git checkout -q -b behind-main "$base"
git commit --allow-empty -q -m "release tip"
main_before=$(git rev-parse HEAD)
git checkout -q -b pr-branch "$base"
git commit --allow-empty -q -m "fix: pull request work"
pr_tip=$(git rev-parse HEAD)
git checkout -q behind-main
GIT_COMMITTER_NAME="GitHub" \
  GIT_COMMITTER_EMAIL="noreply@github.com" \
  git merge -q --no-ff --no-edit \
  -m "Merge pull request #9 from leidarljos/fix/pr-work" "$pr_tip"
"$gate" "$main_before..HEAD"

git checkout -q -b pr-foreign "$base"
GIT_AUTHOR_NAME="Ada Lovelace" \
  GIT_AUTHOR_EMAIL="ada@example.com" \
  git commit --allow-empty -q -m "foreign work in the pull request"
foreign_tip=$(git rev-parse HEAD)
git checkout -q -B behind-main "$main_before"
GIT_COMMITTER_NAME="GitHub" \
  GIT_COMMITTER_EMAIL="noreply@github.com" \
  git merge -q --no-ff --no-edit \
  -m "Merge pull request #12 from leidarljos/fix/foreign" "$foreign_tip"
must_fail "webflow merge of a foreign commit" "$gate" "$main_before..HEAD"

fail_id() {
  desc=$1
  shift
  git checkout -q -B id "$base"
  env "$@" git commit --allow-empty -q -m "identity case"
  must_fail "$desc" "$gate" "$base..HEAD"
}

fail_id "Cursor committer" \
  GIT_COMMITTER_NAME="Cursor Agent" GIT_COMMITTER_EMAIL="cursoragent@cursor.com"
fail_id "Cursor author with webflow committer" \
  GIT_AUTHOR_NAME="Cursor Agent" GIT_AUTHOR_EMAIL="cursoragent@cursor.com" \
  GIT_COMMITTER_NAME="GitHub" GIT_COMMITTER_EMAIL="noreply@github.com"
fail_id "actions bot committer" \
  GIT_COMMITTER_NAME="github-actions[bot]" \
  GIT_COMMITTER_EMAIL="41898282+github-actions[bot]@users.noreply.github.com"
fail_id "dependabot author with webflow committer" \
  GIT_AUTHOR_NAME="dependabot[bot]" \
  GIT_AUTHOR_EMAIL="49699333+dependabot[bot]@users.noreply.github.com" \
  GIT_COMMITTER_NAME="GitHub" GIT_COMMITTER_EMAIL="noreply@github.com"
fail_id "webflow author" \
  GIT_AUTHOR_NAME="GitHub" GIT_AUTHOR_EMAIL="noreply@github.com"
fail_id "webflow author and committer" \
  GIT_AUTHOR_NAME="GitHub" GIT_AUTHOR_EMAIL="noreply@github.com" \
  GIT_COMMITTER_NAME="GitHub" GIT_COMMITTER_EMAIL="noreply@github.com"
fail_id "lower case webflow name" \
  GIT_COMMITTER_NAME="github" GIT_COMMITTER_EMAIL="noreply@github.com"

git checkout -q -B squash "$base"
GIT_COMMITTER_NAME="GitHub" \
  GIT_COMMITTER_EMAIL="noreply@github.com" \
  git commit --allow-empty -q -m "fix: squashed work (#13)" \
  -m "Co-authored-by: Cursor Agent <cursoragent@cursor.com>"
must_fail "webflow squash with a Cursor trailer" "$gate" "$base..HEAD"

# Dates are not judged. A later committer instant still passes.
authored=2026-11-02T20:17:15-06:00
committed=2026-11-03T01:03:03-06:00
git checkout -q -b redated "$base"
GIT_AUTHOR_DATE=$authored GIT_COMMITTER_DATE=$committed \
  git commit --allow-empty -q -m "committed later than authored"
redated=$(git rev-parse HEAD)
epochs=$(git log -1 --format='%at %ct' "$redated")
authored_s=${epochs% *}
committed_s=${epochs#* }
if [ "$authored_s" = "$committed_s" ]; then
  echo "identity-gate-test: date fixture did not differ: $epochs" >&2
  exit 1
fi
"$gate" "$base..HEAD"

# A commit already pushed with unequal dates sits below the range.
git commit --allow-empty -q -m "after the redated commit"
"$gate" "$redated..HEAD"

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
fail_msg "Made-with line" \
  -m "note" -m "Made-with: a tool"
fail_msg "made with cursor line" \
  -m "note" -m "made with cursor"
fail_msg "by line with no blank line" \
  -m "$(printf 'note\nCo-authored-by: Ada Lovelace <ada@example.com>')"
fail_msg "account line" \
  -m "note" -m "Seat alfa wrote this"
fail_msg "account token" \
  -m "note" -m "seatalfa"
fail_msg "account token mid line" \
  -m "note" -m "see seatalfa in the note"
fail_msg "account words mid line" \
  -m "note" -m "see a seat alfa in the note"
fail_msg "account assignment" \
  -m "note" -m "CURSOR_AGENT=1"
fail_msg "indented account assignment" \
  -m "$(printf 'note\n  CURSOR_AGENT=1')"
fail_msg "account colon" \
  -m "note" -m "CURSOR_AGENT: 1"
fail_msg "by line with a space before the colon" \
  -m "note" -m "Co-authored-by : x"
fail_msg "indented by line" \
  -m "$(printf 'note\n  Co-authored-by: x')"
fail_msg "made-with with a space before the colon" \
  -m "note" -m "Made-with : x"
fail_msg "indented made-with line" \
  -m "$(printf 'note\n Made-with: x')"
fail_msg "tab before a by line" \
  -m "$(printf 'note\n'; printf '\t'; printf 'Co-authored-by: A\n')"
fail_msg "tab before the by colon" \
  -m "$(printf 'note\nCo-authored-by'; printf '\t'; printf ': A\n')"
fail_msg "tab before an account assignment" \
  -m "$(printf 'note\n'; printf '\t'; printf 'CURSOR_AGENT=1\n')"
fail_msg "tab before the account equals" \
  -m "$(printf 'note\nCURSOR_AGENT'; printf '\t'; printf '=1\n')"

git checkout -q -B plain "$base"
git commit --allow-empty -q -m "note" -m "Tests: cargo test"
"$gate" "$base..HEAD"
git checkout -q -B plain "$base"
git commit --allow-empty -q -m "note" -m "Note: kept"
"$gate" "$base..HEAD"
git checkout -q -B plain "$base"
git commit --allow-empty -q -m "The Signed-off-by line is gone."
"$gate" "$base..HEAD"
git checkout -q -B plain "$base"
git commit --allow-empty -q -m "see CURSOR_AGENT in the prose"
"$gate" "$base..HEAD"
git checkout -q -B plain "$base"
git commit --allow-empty -q -m "The figures are generated with matplotlib."
"$gate" "$base..HEAD"
git checkout -q -B plain "$base"
git commit --allow-empty -q -m "the plot was made with cursor tools"
"$gate" "$base..HEAD"
git checkout -q -B plain "$base"
git commit --allow-empty -q -m "note" -m "Tests: ran cargo test"
"$gate" "$base..HEAD"
git checkout -q -B plain "$base"
git commit --allow-empty -q -m "Ask the author by mail."
"$gate" "$base..HEAD"
git checkout -q -B plain "$base"
git commit --allow-empty -q -m "Signed-off-by"
"$gate" "$base..HEAD"

must_fail_text() {
  desc=$1
  needle=$2
  shift 2
  if noise=$("$@" 2>&1); then
    echo "identity-gate-test: $desc was accepted" >&2
    exit 1
  fi
  case "$noise" in
    *"$needle"*) ;;
    *)
      echo "identity-gate-test: $desc failed another way: $noise" >&2
      exit 1
      ;;
  esac
}

missing_before=1111111111111111111111111111111111111111

# An unknown revision fails closed. It does not pick another set of commits.
git checkout -q -B plain "$base"
git commit --allow-empty -q -m "clean work after a missing before"
must_fail_text "unknown revision" "git rev-list failed" \
  "$gate" "$missing_before..HEAD"

hist=$root/scripts/identity-history.sh
zero=0000000000000000000000000000000000000000

# develop carries clean work. A release merge of that develop into
# main passes. A later bad commit fails wherever it is reachable.
git checkout -q -B develop "$base"
git commit --allow-empty -q -m "clean develop"
clean=$(git rev-parse HEAD)
"$hist" "$clean"

git checkout -q -B main "$base"
git merge -q --no-ff --no-edit -m "chore: release 0.45.3" "$clean"
release=$(git rev-parse HEAD)
"$hist" "$release"

git checkout -q -B develop "$clean"
GIT_AUTHOR_NAME="Ada Lovelace" \
  GIT_AUTHOR_EMAIL="ada@example.com" \
  git commit --allow-empty -q -m "bad work [skip ci]"
skip=$(git rev-parse HEAD)
git commit --allow-empty -q -m "later clean develop"
later=$(git rev-parse HEAD)
must_fail_text "later push after skipped bad commit" "wrong identity: $skip" \
  "$hist" "$later"
# The old before..sha range would have started after the skipped commit.
BEFORE=$skip must_fail_text "before does not hide the skipped commit" \
  "wrong identity: $skip" "$hist" "$later"

must_fail_text "branch push" "wrong identity: $skip" \
  "$hist" "$later"
git tag -a v9.9.9 -m v9.9.9 "$later"
must_fail_text "tag" "wrong identity: $skip" \
  "$hist" "$(git rev-parse v9.9.9)"
git checkout -q --detach "$base"
GIT_COMMITTER_NAME="GitHub" \
  GIT_COMMITTER_EMAIL="noreply@github.com" \
  git merge -q --no-ff --no-edit \
  -m "Merge pull request #50 from example/topic" "$later"
must_fail_text "pull request" "wrong identity: $skip" \
  "$hist" HEAD

git tag -a v0.1.0 -m v0.1.0 "$clean"
"$hist" "$(git rev-parse v0.1.0)"
git checkout -q --detach "$base"
GIT_COMMITTER_NAME="GitHub" \
  GIT_COMMITTER_EMAIL="noreply@github.com" \
  git merge -q --no-ff --no-edit \
  -m "Merge pull request #51 from example/topic" "$clean"
"$hist" HEAD

# One commit pushed to main and develop. Both tips are that commit, and
# both checks still fail. Remote tips are not consulted.
git checkout -q --detach "$base"
GIT_AUTHOR_NAME="Ada Lovelace" \
  GIT_AUTHOR_EMAIL="ada@example.com" \
  git commit --allow-empty -q -m "pushed to main and develop"
both=$(git rev-parse HEAD)
git update-ref refs/heads/main "$both"
git update-ref refs/heads/develop "$both"
git update-ref refs/remotes/origin/main "$both"
git update-ref refs/remotes/origin/develop "$both"
must_fail_text "bad commit on main" "wrong identity: $both" \
  "$hist" "$(git rev-parse refs/heads/main)"
must_fail_text "bad commit on develop" "wrong identity: $both" \
  "$hist" "$(git rev-parse refs/heads/develop)"

# Deleting and recreating the branches leaves both results unchanged.
git checkout -q --detach
git update-ref -d refs/heads/main
git update-ref -d refs/heads/develop
git update-ref refs/heads/main "$both"
git update-ref refs/heads/develop "$both"
must_fail_text "re-run main" "wrong identity: $both" \
  "$hist" "$(git rev-parse refs/heads/main)"
must_fail_text "re-run develop" "wrong identity: $both" \
  "$hist" "$(git rev-parse refs/heads/develop)"
git update-ref -d refs/heads/main
git update-ref -d refs/heads/develop
git update-ref refs/heads/main "$release"
git update-ref refs/heads/develop "$clean"
"$hist" "$(git rev-parse refs/heads/main)"
"$hist" "$(git rev-parse refs/heads/develop)"
git update-ref -d refs/heads/main
git update-ref -d refs/heads/develop
git update-ref refs/heads/main "$release"
git update-ref refs/heads/develop "$clean"
"$hist" "$(git rev-parse refs/heads/main)"
"$hist" "$(git rev-parse refs/heads/develop)"

# No exclusion list exists, and no argument or file in the checked tree
# skips a bad commit. A later Rohit commit that adds a list naming the
# bad commit still fails.
if [ -e "$root/scripts/identity-legacy.txt" ]; then
  echo "identity-gate-test: an exclusion list exists" >&2
  exit 1
fi
echo "$skip" >"$tmp/skip.txt"
must_fail_text "second argument" "usage:" "$hist" "$later" "$tmp/skip.txt"
git checkout -q -B listed "$later"
mkdir -p scripts
cp "$gate" "$hist" scripts/
echo "$skip" >scripts/identity-legacy.txt
git add scripts
git commit -q -m "list the bad commit"
must_fail_text "bad commit named in the tree" "wrong identity: $skip" \
  scripts/identity-history.sh HEAD
"$hist" "$base"
must_fail_text "bad commit is never skipped" "wrong identity: $skip" \
  "$hist" "$later"

must_fail_text "unknown commit" "not a commit:" \
  "$hist" "$missing_before"
must_fail_text "ref deletion" "not a commit:" \
  "$hist" "$zero"
must_fail_text "history usage" "usage:" "$hist"

git clone -q --depth 1 --branch develop "file://$repo" "$tmp/shallow"
if (cd "$tmp/shallow" && "$hist" HEAD) >"$tmp/shallow-out" 2>&1; then
  echo "identity-gate-test: shallow clone was accepted" >&2
  exit 1
fi
shallow_noise=$(cat "$tmp/shallow-out")
case "$shallow_noise" in
  *"shallow repository"*) ;;
  *)
    echo "identity-gate-test: shallow clone failed another way: $shallow_noise" >&2
    exit 1
    ;;
esac

# The checkout this suite lives in passes.
(cd "$root" && "$hist" HEAD)

must_fail "missing range" "$gate"

echo "identity-gate-test: ok"
