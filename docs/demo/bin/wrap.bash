# Sourced by the recorded shell. Output breaks on spaces.
# Two columns of prompt leave the longest typed line inside the window.
PS1='\[\e[38;2;90;86;224m\]> \[\e[0m\]'
shopt -s checkwinsize
size=$(stty size 2>/dev/null) || size=""
if [ -n "$size" ]; then
  LINES=${size%% *}
  COLUMNS=${size##* }
  export LINES COLUMNS
fi
printf '\033[?7l'
exec > >(python3 -u "$LJOS_DEMO_DIR/wrap.py") 2>&1

# Ctrl-G, sent from a hidden step, reads the ticket just filed and
# lets a second seat vote. One shell refuses a second ballot, so
# ljosbot's vote is started by `seat`, which is not a shell. The
# typed line stays `ljos vote`.
_ljos_demo_arm() {
  id=$(awk '/:ID:/{i=$2} END{print i}' ~/tracker/Software/demo/issues.org)
  ljos() {
    if [ "$LJOS_SEAT" = ljosbot ]; then
      seat ljos "$@"
    else
      command ljos "$@"
    fi
  }
}
bind -x '"\C-g":_ljos_demo_arm'
