#!/usr/bin/env bash
[ -z "${DEMO_CLEAN:-}" ] && exec env -i DEMO_CLEAN=1 TERM=xterm-256color PATH=/usr/bin:/bin bash "$0" "$@"
# Fresh HOME. Each command is typed, then run; nothing here prints a canned result.
set -u
export HOME="$(mktemp -d)"
export PATH="$HOME/.local/bin:/usr/bin:/bin"
export PS1='\[\033[33m\]\w $\[\033[0m\] '
cd "$HOME"

# Expand PS1 in this shell. A child shell drops it, and \w is ~ for this HOME.
show_prompt() {
  printf '%s' "${PS1@P}"
}

type_run() {
  local cmd="$1"
  show_prompt
  local i
  for ((i = 0; i < ${#cmd}; i++)); do
    printf '%s' "${cmd:i:1}"
    sleep 0.035
  done
  sleep 0.5
  printf '\n'
  eval "$cmd"
  sleep "${2:-2.2}"
}

clear
type_run 'curl -fsSLO https://github.com/leidarljos/ljos/releases/download/v0.31.0/install.sh && sh install.sh --version 0.31.0'
# A HOME under /tmp is not a seat, so ljos does not start the writer.
# Start it once; doctor, prefer and the hook then have a pack.
packset ensure >/dev/null 2>&1 || printf 'packset ensure failed\n'
type_run 'ljos doctor'
type_run 'ljos prefer "Tag a release with git push origin TAG; --follow-tags leaves v-tags behind."'
type_run 'echo "how do I push the v0.32.0 tag?" | ljos hook --prompt | jq -r .hookSpecificOutput.additionalContext'
type_run 'ljos policy -- git push --force origin main' 3
