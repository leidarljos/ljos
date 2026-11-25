#!/bin/sh
# Judge the commits selected by the git rev-list arguments, or the SHAs
# on stdin when the first argument is --stdin.
# Exit 1 when an author is not Rohit Goswami <rgoswami@ieee.org>, a
# committer is not that identity, or a message line is a by-trailer, a
# made-with line, a generated line, or a machine account mark. A Tests or
# note paragraph is not that line. The one other committer is GitHub
# <noreply@github.com> on a merge of two or more parents whose one
# signature gpg reports as GOODSIG and VALIDSIG with a primary key
# fingerprint in webflow_fprs, which is what github.com writes when a
# pull request is merged there. A GitHub commit that is not a merge, is
# unsigned, is signed by another key, or is signed by an expired or
# revoked key fails, and the message names the key it saw.
# Dates are not read. Exit 2 when no range was given. A git error fails
# the check. Commits outside the arguments are not read.
# Usage: identity-gate.sh <git rev-list arguments>
#        identity-gate.sh --stdin
set -eu

if [ "$#" -eq 0 ]; then
  echo "identity-gate: usage: identity-gate.sh <range>" >&2
  exit 2
fi

if [ "$1" = "--stdin" ]; then
  if [ "$#" -ne 1 ]; then
    echo "identity-gate: usage: identity-gate.sh <range>" >&2
    exit 2
  fi
  hashes=$(cat) || {
    echo "identity-gate: cannot read commits" >&2
    exit 1
  }
elif ! hashes=$(git rev-list "$@" 2>/dev/null); then
  echo "identity-gate: git rev-list failed" >&2
  exit 1
fi

root=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
webflow_key=$root/scripts/github-web-flow.asc
# Primary key fingerprints of GitHub's web-flow keys. A merge passes only
# when gpg's VALIDSIG line names one of these as the primary key.
# scripts/github-web-flow.asc must hold each of them.
webflow_fprs="968479A1AFF927E37D1A566BB5690EEEBB952194"
rotate="If GitHub changed its web-flow key, check the key at https://github.com/web-flow.gpg, put it in scripts/github-web-flow.asc and its primary fingerprint in webflow_fprs in scripts/identity-gate.sh."
gnupg=
trap '[ -z "$gnupg" ] || {
  GNUPGHOME=$gnupg gpgconf --kill all >/dev/null 2>&1 || :
  rm -rf "$gnupg"
}' EXIT

load_fail() {
  echo "identity-gate: cannot load the GitHub web-flow key $webflow_key${1:+: $1}" >&2
  exit 1
}

# The keyring is a temp GNUPGHOME that holds only the committed key file,
# so a caller's keyring or gpg.conf is never read.
load_webflow_key() {
  [ -z "$gnupg" ] || return 0
  gnupg=$(mktemp -d) || load_fail
  chmod 700 "$gnupg" || load_fail
  printf 'no-auto-key-retrieve\n' >"$gnupg/gpg.conf" || load_fail
  GNUPGHOME=$gnupg gpg --batch --no-tty --quiet --import "$webflow_key" \
    >/dev/null 2>&1 || load_fail
  held=$(GNUPGHOME=$gnupg gpg --batch --no-tty --with-colons --list-keys 2>/dev/null |
    awk -F: '$1 == "pub" { p = 1; next } $1 == "fpr" && p { print $10 } { p = 0 }') ||
    load_fail
  for fpr in $webflow_fprs; do
    printf '%s\n' "$held" | grep -qx "$fpr" || load_fail "it holds no primary key $fpr"
  done
}

# Print ok, or why the commit is not a merge GitHub's web-flow key
# signed. Only gpg's status lines are read; its exit code alone passes
# nothing.
webflow_merge() {
  parents=$(git log -1 --format='%P' "$1") || {
    echo "cannot read its parents"
    return
  }
  if [ "$(printf '%s\n' "$parents" | wc -w)" -lt 2 ]; then
    echo "is not a merge"
    return
  fi
  rc=0
  raw=$(GNUPGHOME=$gnupg git -c gpg.format=openpgp -c gpg.program=gpg \
    -c gpg.openpgp.program=gpg verify-commit --raw "$1" 2>&1 >/dev/null) || rc=$?
  printf '%s\n' "$raw" | awk -v pins="$webflow_fprs" -v rc="$rc" -v rotate="$rotate" '
    $1 != "[GNUPG:]" { next }
    { seen++ }
    $2 == "NEWSIG" { sigs++ }
    $2 == "GOODSIG" { good++ }
    $2 == "BADSIG" { bad++ }
    $2 == "EXPSIG" { expsig++ }
    $2 == "EXPKEYSIG" { expkey++ }
    $2 == "REVKEYSIG" { revkey++ }
    $2 == "ERRSIG" { err++; errkey = ($9 != "" && $9 != "-") ? $9 : $3; errrc = $8 }
    $2 == "VALIDSIG" { valid++; signer = $3; primary = $12 }
    END {
      if (!seen) { print "is an unsigned merge"; exit }
      if (sigs > 1 || valid > 1) { print "carries more than one signature"; exit }
      if (bad) { print "has a bad signature: it was changed after it was signed"; exit }
      if (err && errrc == 9) {
        print "is signed by key " errkey ", which scripts/github-web-flow.asc does not hold; the pinned web-flow keys are " pins ". " rotate
        exit
      }
      if (err) { print "has a signature gpg cannot check (key " errkey ", code " errrc ")"; exit }
      if (valid != 1 || length(primary) != 40 || primary ~ /[^0-9A-F]/) { print "has no VALIDSIG line with a primary key fingerprint"; exit }
      n = split(pins, pin, " ")
      pinned = 0
      for (i = 1; i <= n; i++) if (primary == pin[i]) pinned = 1
      if (!pinned) {
        print "is signed by key " signer " with primary key " primary ", which is not a pinned web-flow key (" pins "). " rotate
        exit
      }
      if (revkey) { print "is signed by web-flow key " primary ", which is revoked. " rotate; exit }
      if (expkey) { print "is signed by web-flow key " primary ", which has expired. " rotate; exit }
      if (expsig) { print "has an expired signature from web-flow key " primary; exit }
      if (good != 1 || rc != 0) { print "has no good signature from web-flow key " primary; exit }
      print "ok"
    }'
}

want="Rohit Goswami <rgoswami@ieee.org>"
webflow="GitHub <noreply@github.com>"
pat='^[[:blank:]]*(([A-Za-z0-9-]+-by|made-with)[[:blank:]]*:|cursor[_-]?agent[A-Za-z0-9_]*[[:blank:]]*[=:]|generated with|made with cursor)|seat alfa|seatalfa'
status=0
for hash in $hashes; do
  id=$(git log -1 --format='%an <%ae> | %cn <%ce>' "$hash") || {
    echo "identity-gate: cannot read $hash" >&2
    exit 1
  }
  case "$id" in
    "$want | $want") ;;
    "$want | $webflow")
      load_webflow_key
      why=$(webflow_merge "$hash")
      if [ "$why" != ok ]; then
        echo "identity-gate: wrong identity: $hash $id ${why:-was not checked}" >&2
        status=1
      fi
      ;;
    *)
      echo "identity-gate: wrong identity: $hash $id" >&2
      status=1
      ;;
  esac
  body=$(git log -1 --format='%B' "$hash") || {
    echo "identity-gate: cannot read $hash" >&2
    exit 1
  }
  if printf '%s\n' "$body" | grep -iE "$pat" >/dev/null; then
    echo "identity-gate: trailer or generated text: $hash" >&2
    status=1
  fi
done
exit "$status"
