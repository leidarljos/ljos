#!/bin/sh
# A clean range passes. A wrong author, a wrong committer, and a
# trailer each fail. A committer date that differs from the author
# date passes. A foreign commit that sits before the range is not
# judged. A merge GitHub commits and signs for Rohit passes, and 1dd43134
# passes with GitHub's key. A GitHub commit that is not a merge, is
# unsigned, is signed by another key, or was edited after signing fails.
# A key that says it is GitHub but is not pinned fails, and so does a
# subkey of an unpinned primary key and a pin that names a subkey. A key
# the key file lacks and an expired pinned key fail with a message that
# names the commit, the key and what to update.
# A signed merge with another committer fails. A GitHub merge that brings
# a foreign commit or a trailer fails, and so does a GitHub squash and an
# agent or bot author or committer. scripts/identity-history.sh checks every commit
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
export GNUPGHOME="$tmp/gnupg"
trap 'gpgconf --kill all >/dev/null 2>&1 || :; rm -rf "$tmp"' EXIT
repo=$tmp/repo
git init -q -b main "$repo"
git -C "$repo" config user.name "Rohit Goswami"
git -C "$repo" config user.email "rgoswami@ieee.org"
git -C "$repo" config commit.gpgsign false
git -C "$repo" config tag.gpgsign false
git -C "$repo" config gpg.format openpgp
git -C "$repo" config gpg.program gpg
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

# The shipped key is GitHub's web-flow key.
webflow_fpr=968479A1AFF927E37D1A566BB5690EEEBB952194
mkdir -m 700 "$GNUPGHOME"
shipped_fpr=$(gpg --batch --quiet --with-colons --import-options show-only \
  --import "$root/scripts/github-web-flow.asc" 2>/dev/null | awk -F: '$1 == "fpr" { print $10 }')
if [ "$shipped_fpr" != "$webflow_fpr" ]; then
  echo "identity-gate-test: scripts/github-web-flow.asc is not $webflow_fpr: $shipped_fpr" >&2
  exit 1
fi
if ! grep -qx "webflow_fprs=\"$webflow_fpr\"" "$gate"; then
  echo "identity-gate-test: identity-gate.sh does not pin $webflow_fpr" >&2
  exit 1
fi

# Make a throwaway key and print its primary fingerprint, or with ssb
# as the second argument its last subkey's.
new_key() {
  gpg --batch --quiet --passphrase '' --quick-gen-key "$@" 2>/dev/null
}
fpr_of() {
  gpg --batch --quiet --with-colons --list-secret-keys "=$1" 2>/dev/null |
    awk -F: -v kind="${2:-sec}" '$1 == "sec" || $1 == "ssb" { k = $1 }
      $1 == "fpr" && k == kind { f = $10 } END { print f }'
}

# A copy of the scripts that pins the fingerprints in $2 and holds the
# keys named after it in its key file.
fake_scripts() {
  dir=$1
  pins=$2
  shift 2
  mkdir -p "$dir/scripts"
  sed "s/^webflow_fprs=.*/webflow_fprs=\"$pins\"/" "$gate" >"$dir/scripts/identity-gate.sh"
  chmod +x "$dir/scripts/identity-gate.sh"
  cp "$root/scripts/identity-history.sh" "$dir/scripts/"
  gpg --batch --armor --export "$@" >"$dir/scripts/github-web-flow.asc"
}

# GitHub's key cannot sign here. A throwaway key signs the merges, and a
# copy of the scripts that pins only that key judges them.
new_key "Test Signer <signer@example.com>" ed25519 sign never
signer=$(fpr_of "Test Signer <signer@example.com>")
fake=$tmp/fake
fake_scripts "$fake" "$signer" "$signer"
fake_gate=$fake/scripts/identity-gate.sh
fake_hist=$fake/scripts/identity-history.sh
github="GIT_COMMITTER_NAME=GitHub GIT_COMMITTER_EMAIL=noreply@github.com"
seen="Rohit Goswami <rgoswami@ieee.org> | GitHub <noreply@github.com>"

# A merge onto base of one Rohit commit, signed by $key (the throwaway
# key unless set) and committed with the identity the trailing
# NAME=value arguments give.
key=
gpg_program=
signed_merge() {
  branch=$1
  msg=$2
  shift 2
  git checkout -q -B side "$base"
  git commit --allow-empty -q -m "fix: pull request work"
  side=$(git rev-parse HEAD)
  git checkout -q -B "$branch" "$base"
  env "$@" git ${gpg_program:+-c gpg.program="$gpg_program"} \
    merge -q --no-ff --no-edit -S"${key:-$signer}" -m "$msg" "$side"
}

# shellcheck disable=SC2086
signed_merge webflow "Merge pull request #9 from leidarljos/fix" $github
"$fake_gate" "$base..HEAD"
must_fail "GitHub merge signed by a key that is not GitHub's" "$gate" "$base..HEAD"

git checkout -q -B webflow-unsigned "$base"
# shellcheck disable=SC2086
env $github git merge -q --no-ff --no-edit \
  -m "Merge pull request #9 from leidarljos/fix" side
must_fail "unsigned GitHub merge" "$fake_gate" "$base..HEAD"

git checkout -q -B webflow-single "$base"
# shellcheck disable=SC2086
env $github git commit --allow-empty -q -S"$signer" \
  -m "Merge pull request #9 from leidarljos/fix"
must_fail "signed non-merge commit with the GitHub committer" "$fake_gate" "$base..HEAD"

signed_merge merge-foreign-committer "Merge pull request #14 from leidarljos/fix" \
  GIT_COMMITTER_NAME="Ada Lovelace" GIT_COMMITTER_EMAIL=ada@example.com
must_fail "signed merge with a non-GitHub committer" "$fake_gate" "$base..HEAD"

signed_merge webflow-wrong-committer-name "Merge pull request #11 from leidarljos/fix" \
  GIT_COMMITTER_NAME=Someone GIT_COMMITTER_EMAIL=noreply@github.com
must_fail "noreply github committer without exact GitHub name" "$fake_gate" "$base..HEAD"

# shellcheck disable=SC2086
signed_merge webflow-foreign-author "webflow merge for foreign author" $github \
  GIT_AUTHOR_NAME="Ada Lovelace" GIT_AUTHOR_EMAIL=ada@example.com
must_fail "webflow committer with non-Rohit author" "$fake_gate" "$base..HEAD"

# shellcheck disable=SC2086
signed_merge webflow-trailer \
  "$(printf 'Merge pull request #10\n\nCo-authored-by: Ada Lovelace <ada@example.com>')" \
  $github
must_fail "webflow committer with trailer" "$fake_gate" "$base..HEAD"

# A key whose user ID claims to be GitHub, in the key file next to the
# pinned key, signs a merge. The VALIDSIG primary key is not pinned.
new_key "GitHub <noreply@github.com>" ed25519 sign never
claimer=$(fpr_of "GitHub <noreply@github.com>")
fake_scripts "$tmp/claim" "$signer" "$signer" "$claimer"
key=$claimer
# shellcheck disable=SC2086
signed_merge claim "Merge pull request #9 from leidarljos/fix" $github
key=
hash=$(git rev-parse HEAD)
must_fail_text "merge signed by a key that claims to be GitHub" \
  "wrong identity: $hash $seen is signed by key $claimer with primary key $claimer, which is not a pinned web-flow key ($signer)" \
  "$tmp/claim/scripts/identity-gate.sh" "$base..HEAD"
must_fail_text "merge signed by a key that claims to be GitHub, real key" \
  "is signed by key $claimer, which scripts/github-web-flow.asc does not hold" \
  "$gate" "$base..HEAD"

# A signing subkey under a primary key. The pin is a primary key
# fingerprint, so the subkey's own fingerprint does not pass, and a
# subkey of a primary key that is not pinned does not pass.
new_key "Sub Signer <sub@example.com>" ed25519 cert never
sub_primary=$(fpr_of "Sub Signer <sub@example.com>")
gpg --batch --quiet --passphrase '' --quick-add-key "$sub_primary" ed25519 sign never 2>/dev/null
sub=$(fpr_of "Sub Signer <sub@example.com>" ssb)
key="$sub!"
# shellcheck disable=SC2086
signed_merge subkey "Merge pull request #9 from leidarljos/fix" $github
key=
hash=$(git rev-parse HEAD)
fake_scripts "$tmp/sub-pin" "$sub" "$sub_primary"
must_fail_text "pin naming the signing subkey" \
  "it holds no primary key $sub" \
  "$tmp/sub-pin/scripts/identity-gate.sh" "$base..HEAD"
fake_scripts "$tmp/sub-other" "$signer" "$signer" "$sub_primary"
must_fail_text "subkey of a primary key that is not pinned" \
  "wrong identity: $hash $seen is signed by key $sub with primary key $sub_primary, which is not a pinned web-flow key ($signer)" \
  "$tmp/sub-other/scripts/identity-gate.sh" "$base..HEAD"
fake_scripts "$tmp/sub-ok" "$sub_primary" "$sub_primary"
"$tmp/sub-ok/scripts/identity-gate.sh" "$base..HEAD"

# GitHub rotates its key: a merge signed by a key the key file does not
# hold names the commit, the key and what to update.
new_key "Next Signer <next@example.com>" ed25519 sign never
next=$(fpr_of "Next Signer <next@example.com>")
key=$next
# shellcheck disable=SC2086
signed_merge rotated "Merge pull request #9 from leidarljos/fix" $github
key=
hash=$(git rev-parse HEAD)
must_fail_text "merge signed by a rotated key" \
  "wrong identity: $hash $seen is signed by key $next, which scripts/github-web-flow.asc does not hold; the pinned web-flow keys are $signer. If GitHub changed its web-flow key, check the key at https://github.com/web-flow.gpg, put it in scripts/github-web-flow.asc and its primary fingerprint in webflow_fprs in scripts/identity-gate.sh." \
  "$fake_gate" "$base..HEAD"

# A pinned key that expired after it signed fails and says so.
printf '#!/bin/sh\nexec gpg --faked-system-time 20200101T120000 "$@"\n' >"$tmp/gpg-2020"
chmod +x "$tmp/gpg-2020"
gpg --faked-system-time 20200101T000000 --batch --quiet --passphrase '' \
  --quick-gen-key "Old Signer <old@example.com>" ed25519 sign 1d 2>/dev/null
old_key=$(fpr_of "Old Signer <old@example.com>")
gpg_program=$tmp/gpg-2020
key=$old_key
# shellcheck disable=SC2086
signed_merge expired "Merge pull request #9 from leidarljos/fix" $github
key=
gpg_program=
hash=$(git rev-parse HEAD)
fake_scripts "$tmp/expired" "$old_key" "$old_key"
must_fail_text "merge signed by an expired web-flow key" \
  "wrong identity: $hash $seen is signed by web-flow key $old_key, which has expired. If GitHub changed its web-flow key" \
  "$tmp/expired/scripts/identity-gate.sh" "$base..HEAD"

# A key file that lacks a pinned key fails before any commit is judged.
fake_scripts "$tmp/unheld" "$signer $next" "$signer"
# shellcheck disable=SC2086
signed_merge webflow "Merge pull request #9 from leidarljos/fix" $github
must_fail_text "pin missing from the key file" \
  "cannot load the GitHub web-flow key $tmp/unheld/scripts/github-web-flow.asc: it holds no primary key $next" \
  "$tmp/unheld/scripts/identity-gate.sh" "$base..HEAD"

# 1dd43134 is the merge github.com wrote for pull request #9. It passes
# with GitHub's key. The same commit with its message edited keeps the
# signature and fails.
real=$tmp/real
git clone -q --shared --no-checkout "$root" "$real"
webflow_pr=1dd43134fc86a51ebea9459285eefc9b04dfa20a
(
  cd "$real"
  "$gate" -1 "$webflow_pr"
  forged=$(git cat-file commit "$webflow_pr" |
    sed 's/^Merge pull request #9 /Merge pull request #90 /' |
    git hash-object -t commit -w --stdin)
  must_fail "1dd43134 with an edited message" "$gate" -1 "$forged"
)

# The main push of 1dd43134: main is behind, and the push brings a
# two-parent merge GitHub made of a pull request branch.
git checkout -q -b behind-main "$base"
git commit --allow-empty -q -m "release tip"
main_before=$(git rev-parse HEAD)
git checkout -q -b pr-branch "$base"
git commit --allow-empty -q -m "fix: pull request work"
pr_tip=$(git rev-parse HEAD)
git checkout -q behind-main
# shellcheck disable=SC2086
env $github git merge -q --no-ff --no-edit -S"$signer" \
  -m "Merge pull request #9 from leidarljos/fix/pr-work" "$pr_tip"
"$fake_gate" "$main_before..HEAD"

git checkout -q -b pr-foreign "$base"
GIT_AUTHOR_NAME="Ada Lovelace" \
  GIT_AUTHOR_EMAIL="ada@example.com" \
  git commit --allow-empty -q -m "foreign work in the pull request"
foreign_tip=$(git rev-parse HEAD)
git checkout -q -B behind-main "$main_before"
# shellcheck disable=SC2086
env $github git merge -q --no-ff --no-edit -S"$signer" \
  -m "Merge pull request #12 from leidarljos/fix/foreign" "$foreign_tip"
must_fail "webflow merge of a foreign commit" "$fake_gate" "$main_before..HEAD"

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
# shellcheck disable=SC2086
env $github git merge -q --no-ff --no-edit -S"$signer" \
  -m "Merge pull request #51 from example/topic" "$clean"
"$fake_hist" HEAD

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
