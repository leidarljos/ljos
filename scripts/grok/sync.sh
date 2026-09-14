#!/bin/sh
# Install the Grok hook files. The script and `ljos hook` are exec'd
# each event, so they go live without /hooks r. JSON matchers do not:
# print when that reload is actually required. Stale ljos-mcp (deleted
# inode) is SIGTERM'd so Grok respawns the binary on PATH.
set -eu
HOME="${HOME:?}"
HERE=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
DEST="$HOME/.grok/hooks"
mkdir -p "$DEST"

install -m 755 "$HERE/ljos-inject.sh" "$DEST/ljos-inject.sh"

want=$(python3 -c 'import os,pathlib,sys
p=pathlib.Path(sys.argv[1])
print(p.read_text().replace("${HOME}", os.path.expanduser("~")))
' "$HERE/ljos.json")
got=""
if [ -f "$DEST/ljos.json" ]; then
  got=$(cat "$DEST/ljos.json")
fi
json_changed=0
if [ "$want" != "$got" ]; then
  printf '%s' "$want" > "$DEST/ljos.json"
  json_changed=1
fi

echo "inject.sh  $DEST/ljos-inject.sh  (live on next event, no reload)"
if [ "$json_changed" -eq 1 ]; then
  echo "ljos.json  $DEST/ljos.json  changed: /hooks then r  (matchers only)"
else
  echo "ljos.json  unchanged  (no /hooks r)"
fi

# Grok holds ljos-mcp until the process dies. A replaced binary is
# (deleted) in /proc. Kill those so the next sitting gets PATH.
n=0
for p in $(pgrep -x ljos-mcp 2>/dev/null || true); do
  exe=$(readlink "/proc/$p/exe" 2>/dev/null || true)
  case "$exe" in
    *'(deleted)'*)
      kill -TERM "$p" 2>/dev/null || true
      n=$((n + 1))
      ;;
  esac
done
if [ "$n" -gt 0 ]; then
  echo "ljos-mcp   sent TERM to $n stale process(es); Grok respawns"
else
  echo "ljos-mcp   no deleted-inode processes"
fi
