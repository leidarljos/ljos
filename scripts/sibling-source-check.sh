#!/usr/bin/env bash
# Source sibling-tools.sh and check that packsetd and vissue resolve.
#
#   scripts/sibling-source-check.sh [DEST]
#
# DEST is the directory passed to the sourced script. With no DEST, the
# sourced script picks its cache directory. The check removes DEST from
# PATH first, so a hit is the directory the source added.
set -euo pipefail

here=$(CDPATH= cd -- "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)
dest=${1:-}

strip_dest() {
  local drop=$1 old=$2 part kept=
  old=${old#:}
  while [ -n "$old" ]; do
    part=${old%%:*}
    case "$old" in
      *:*) old=${old#*:} ;;
      *) old= ;;
    esac
    [ "$part" = "$drop" ] && continue
    if [ -z "$kept" ]; then
      kept=$part
    else
      kept=$kept:$part
    fi
  done
  printf '%s' "$kept"
}

if [ -n "$dest" ]; then
  mkdir -p "$dest"
  dest=$(CDPATH= cd -- "$dest" && pwd)
  PATH=$(strip_dest "$dest" "$PATH")
  export PATH
  # shellcheck source=scripts/sibling-tools.sh
  source "$here/sibling-tools.sh" "$dest"
else
  # shellcheck source=scripts/sibling-tools.sh
  source "$here/sibling-tools.sh"
  dest=${LJOS_SIBLING_DEST:-${XDG_CACHE_HOME:-$HOME/.cache}/ljos/sibling-bin}
  dest=$(CDPATH= cd -- "$dest" && pwd)
fi

pack=$(command -v packsetd)
vissue=$(command -v vissue)
[ "$pack" = "$dest/packsetd" ]
[ "$vissue" = "$dest/vissue" ]
packsetd --version >/dev/null
vissue --version >/dev/null
printf '%s\n' "sibling-source-check: packsetd and vissue resolve in $dest"
