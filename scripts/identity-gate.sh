#!/bin/sh
# Judge the commits selected by the git rev-list arguments.
# Exit 1 when an author or committer is not Rohit Goswami
# <rgoswami@ieee.org>, or a message has a refused trailer or generated
# text. Exit 2 when no range was given. Commits outside the arguments
# are not read.
# Usage: identity-gate.sh <git rev-list arguments>
set -eu

if [ "$#" -eq 0 ]; then
  echo "identity-gate: usage: identity-gate.sh <range>" >&2
  exit 2
fi

if ! hashes=$(git rev-list "$@"); then
  echo "identity-gate: git rev-list failed" >&2
  exit 1
fi

want="Rohit Goswami <rgoswami@ieee.org>"
pat='^(co-authored-by|signed-off-by)[ \t]*:|generated with|cursor agent|cursoragent'
status=0
for hash in $hashes; do
  id=$(git log -1 --format='%an <%ae> | %cn <%ce>' "$hash") || {
    echo "identity-gate: cannot read $hash" >&2
    exit 1
  }
  case "$id" in
    "$want | $want") ;;
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
