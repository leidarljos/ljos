#!/bin/sh
# Judge the commits selected by the git rev-list arguments.
# Exit 1 when an author or committer is not Rohit Goswami
# <rgoswami@ieee.org>, or a message line is a by-trailer, a made-with
# line, a generated line, or a machine account mark. A Tests or note
# paragraph is not that line.
# Exit 2 when no range was given. Commits outside the arguments
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
pat='^[[:blank:]]*(([A-Za-z0-9-]+-by|made-with)[[:blank:]]*:|cursor[_-]?agent[A-Za-z0-9_]*[[:blank:]]*[=:]|generated with|made with cursor)|cursor agent|cursoragent'
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
