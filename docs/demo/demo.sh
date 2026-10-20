#!/usr/bin/env bash
# The README recording: a fresh HOME, from the install line to the first
# recall. Each command is typed, then run; nothing prints a canned result,
# and nothing runs that the cast does not show.
#
#   asciinema rec --cols 100 --rows 30 --idle-time-limit 1.5 \
#     -c docs/demo/demo.sh docs/demo/install-to-recall.cast
#
# DEMO_ROOT is where the fresh HOME is made (default: the caller's HOME). It
# must not be a temporary directory: ljos treats a HOME under /tmp as a test
# sandbox and keeps its guard and memory writer off there.
# DEMO_INSTALL is the install line typed first (default: the README's). A
# recording made before a release names its local build here, such as
# `sh /opt/ljos-build/install.sh --from-dir /opt/ljos-build`, so the cast
# shows where the programs came from.
[ -z "${DEMO_CLEAN:-}" ] && exec env -i DEMO_CLEAN=1 TERM=xterm-256color PATH=/usr/bin:/bin \
  DEMO_ROOT="${DEMO_ROOT:-$HOME}" DEMO_INSTALL="${DEMO_INSTALL:-}" bash "$0" "$@"
set -u
export HOME="$(mktemp -d "$DEMO_ROOT/ljos-demo.XXXXXX")"
export PATH="$HOME/.local/bin:/usr/bin:/bin"
export PS1='\[\033[33m\]\w $\[\033[0m\] '
cd "$HOME"

install_line='curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh'
install_line=${DEMO_INSTALL:-$install_line}

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
type_run "$install_line"
type_run 'ljos doctor'
type_run 'ljos prefer "Push a tag with git push origin TAG; --follow-tags skips lightweight tags."'
type_run 'echo "how do I push the v0.32.0 tag?" | ljos hook --prompt --text'
type_run 'ljos policy -- git push --force origin main' 3
