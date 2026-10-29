#!/usr/bin/env bash
# Install ljos into an empty HOME, run it as two seats, take it off again,
# and check that HOME is back to what the install left.
#
#   scripts/fresh-install-check.sh                      # cargo build from this tree
#   scripts/fresh-install-check.sh --cargo-target DIR   # ljos and ljos-mcp already built in DIR
#   scripts/fresh-install-check.sh --tarball FILE       # a release tarball, ljos-vX-TARGET.tar.gz
#
# The other programs (packset, vissue, deedar, claimdag, ljos-policyd,
# ljos-consensus) come from scripts/install.sh and the network. With
# --companions DIR they come from DIR instead, so the run works offline.
#
# Steps, in order: install, onboard, seat add, file, claim, vote,
# consensus, note, recall, digest, offboard, clean HOME. A step that fails
# does not stop the run. The last lines list every step that failed, and
# the exit status is 1 when the list is not empty.
#
# Options:
#   --tarball FILE      install ljos and ljos-mcp from a release tarball
#   --cargo-target DIR  install ljos and ljos-mcp from a cargo target dir
#   --companions DIR    take the other programs from DIR, not the network
#   --root DIR          where the run's HOME and logs go (default:
#                       target/fresh-install under this tree). It must not
#                       be under /tmp: ljos switches its guard and memory
#                       off when HOME is a temporary directory
#   --port N            the pack writer's port, set as PACKSET_URL. By
#                       default the writer starts on its own port, 8761,
#                       and the run stops if something already holds it
#   --seed-configs      before the install snapshot, put a user file in
#                       each of three runner configs, so a step can check
#                       that offboard gives them back as they were
#   --offboard-args S   arguments for `ljos offboard`, in place of the
#                       ones read from its --help
#   --keep              keep the run directory when every step passed
#   --no-detach         run in this process tree. Without it the checks run
#                       from a detached process, so a coding agent that is
#                       running this script does not become the seats'
#                       runner and make two seats one voice
#   --help              this text
#
# Environment: FRESH_INSTALL_ROOT is --root. The checks run with a clean
# environment: PATH, HOME, the git identity and the seat names only.
set -u

SELF=$(CDPATH= cd -- "$(dirname "$0")" && pwd)/$(basename "$0")
REPO=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)

say() { printf '%s\n' "$*"; }
die() { printf 'fresh-install-check: %s\n' "$*" >&2; exit 2; }

usage() { awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$SELF"; }

# ---------------------------------------------------------------- child --
# The child runs every step with the environment the parent wrote to
# RUN/config.sh. It is started by the parent below.
if [ "${1:-}" = --child ]; then
  RUN=$2
  # shellcheck disable=SC1091
  . "$RUN/config.sh"
  LOGS=$RUN/logs
  RESULTS=$RUN/results
  : > "$RESULTS"
  mkdir -p "$LOGS"
  export HOME PATH XDG_RUNTIME_DIR
  export GIT_AUTHOR_NAME=fresh GIT_AUTHOR_EMAIL=fresh@example.invalid
  export GIT_COMMITTER_NAME=fresh GIT_COMMITTER_EMAIL=fresh@example.invalid
  [ -z "$PORT" ] || export PACKSET_URL="http://127.0.0.1:$PORT"

  # A seat's commands come from its runner, through a shell of their own.
  # ljos reads the process tree and refuses a second ballot from one shell,
  # whatever session id the second one names. So each call goes through
  # awk, a program that is neither a shell nor a wrapper, and awk starts
  # the shell that starts ljos. That is the shape a runner gives it.
  as_seat() {
    seat=$1; shift
    SEAT_CMD=$(printf '%q ' "$@") \
      LJOS_SEAT=$seat VISSUE_AGENT=$seat LJOS_SESSION_ID=$seat-session-0001 \
      awk 'BEGIN { exit system(ENVIRON["SEAT_CMD"]) }'
  }
  alice() { as_seat alice "$@"; }
  bob() { as_seat bob "$@"; }

  N=0
  LAST=
  STEP_OK=1
  ACCEPT=
  trap 'touch "$RUN/DONE"' EXIT
  # step NAME CMD...: run CMD, keep its output in LOGS, record pass or fail.
  step() {
    name=$1; shift
    N=$((N + 1))
    LAST=$LOGS/$(printf '%02d' "$N")-$(printf '%s' "$name" | tr -c 'A-Za-z0-9\n' '-').log
    { printf '$ %s\n' "$*"; "$@"; } > "$LAST" 2>&1
    rc=$?
    if [ "$rc" != 0 ] && [ -n "$ACCEPT" ] && "$ACCEPT" "$LAST"; then rc=0; fi
    STEP_OK=1
    [ "$rc" = 0 ] || STEP_OK=0
    if [ "$rc" = 0 ]; then
      say "ok    $name"
      printf 'ok\t%s\n' "$name" >> "$RESULTS"
    else
      why=$(missing_command "$LAST")
      [ -n "$why" ] || why="exit $rc: $(last_line "$LAST")"
      say "FAIL  $name ($why)"
      printf 'fail\t%s\t%s\n' "$name" "$why" >> "$RESULTS"
    fi
    return "$rc"
  }
  # A runner whose own command is not installed here gets a `no` row and
  # exit 1 from onboard. That is the machine, not the install, so only a
  # `no` row with another reason fails the step.
  only_path_missing() {
    ! grep -P '^no\t' "$1" | grep -qv 'not on PATH'
  }
  onboard_step() {
    ACCEPT=only_path_missing
    step "$1" ljos onboard --harness "$2"
    ACCEPT=
  }
  # fail NAME WHY: record a failed check that is not a command.
  fail() {
    say "FAIL  $1 ($2)"
    printf 'fail\t%s\t%s\n' "$1" "$2" >> "$RESULTS"
  }
  pass() {
    say "ok    $1"
    printf 'ok\t%s\n' "$1" >> "$RESULTS"
  }
  # expect NAME FILE PATTERN: pass when the log holds the pattern. When the
  # step before it failed, there is nothing to read, so it is skipped.
  expect() {
    if [ "$STEP_OK" = 0 ]; then
      say "skip  $1 (the step before it failed)"
      printf 'skip\t%s\n' "$1" >> "$RESULTS"
    elif grep -Eq -- "$3" "$2" 2>/dev/null; then
      pass "$1"
    else
      fail "$1" "no match for: $3"
    fi
  }
  last_line() { grep -v '^\$ ' "$1" | grep -v '^[[:space:]]*$' | tail -n 1 | cut -c1-160; }
  # A clap error for a subcommand this build does not have.
  missing_command() {
    if grep -Eq "unrecognized subcommand|unexpected argument '(offboard|digest|add)'" "$1"; then
      printf 'not in this build: %s' "$(grep -E -m1 "unrecognized subcommand|unexpected argument" "$1" | cut -c1-80)"
    fi
  }
  # snapshot DIR OUT: every path under DIR with its type, then the hash of
  # each file, as OUT.paths and OUT.sums.
  snapshot() {
    ( cd "$1" && find . -mindepth 1 -printf '%y %p\n' | sort ) > "$2.paths" 2> /dev/null
    ( cd "$1" && find . -type f -exec sha256sum {} + | sort -k2 ) > "$2.sums" 2> /dev/null
  }
  # my_procs: processes whose command line names this HOME.
  my_procs() { pgrep -af -- "$HOME" | grep -v "pgrep" || true; }

  say "fresh-install-check: HOME=$HOME"
  say "source: $SOURCE_DESC"
  say

  # ------------------------------------------------------------ install
  mkdir -p "$HOME/.local/bin"
  if [ -n "$COMPANIONS" ]; then
    mkdir -p "$RUN/from"
    cp "$COMPANIONS"/* "$RUN/from/" 2>/dev/null
    cp "$STAGE"/ljos "$STAGE"/ljos-mcp "$RUN/from/"
    step install sh "$REPO/scripts/install.sh" --from-dir "$RUN/from" --no-embed --no-onboard --no-modify-path
  else
    step install sh "$REPO/scripts/install.sh" --no-embed --no-onboard --no-modify-path
    # install.sh fetches the latest release of every program; the ljos
    # under test goes over the one it fetched.
    cp "$STAGE/ljos" "$HOME/.local/bin/ljos.new" && mv -f "$HOME/.local/bin/ljos.new" "$HOME/.local/bin/ljos"
    cp "$STAGE/ljos-mcp" "$HOME/.local/bin/ljos-mcp.new" && mv -f "$HOME/.local/bin/ljos-mcp.new" "$HOME/.local/bin/ljos-mcp"
  fi
  for p in ljos ljos-mcp packset packsetd vissue vissue-mcp deedar claimdag ljos-policyd ljos-consensus; do
    [ -x "$HOME/.local/bin/$p" ] || fail "install: $p is in the bin dir" "missing"
  done
  step "version" ljos --version
  expect "version is the one under test" "$LAST" "ljos $WANT_VERSION\$"

  if [ "$SEED" = 1 ]; then
    mkdir -p "$HOME/.claude" "$HOME/.cursor" "$HOME/.codex"
    printf '{\n  "model": "keep-me"\n}\n' > "$HOME/.claude/settings.json"
    printf '{\n  "mcpServers": {\n    "other": {"command": "other-server"}\n  }\n}\n' > "$HOME/.cursor/mcp.json"
    printf 'model = "keep-me"\n' > "$HOME/.codex/config.toml"
  fi

  snapshot "$HOME" "$RUN/after-install"

  # ------------------------------------------------------------ onboard
  onboard_step "onboard (all runners found)" all
  onboard_step "onboard claude" claude
  onboard_step "onboard cursor" cursor
  onboard_step "onboard codex" codex
  step "doctor" ljos doctor
  snapshot "$HOME" "$RUN/after-onboard"
  if cmp -s "$RUN/after-install.paths" "$RUN/after-onboard.paths"; then
    fail "onboard wrote files" "HOME did not change"
  else
    pass "onboard wrote files"
  fi

  # --------------------------------------------------------------- seats
  step "seat add alice" alice ljos seat add alice
  step "seat add bob" bob ljos seat add bob
  step "seat names alice" alice ljos seat
  expect "alice is the seat alice" "$LAST" "^seat[[:space:]]+alice"
  step "seat names bob" bob ljos seat
  expect "bob is the seat bob" "$LAST" "^seat[[:space:]]+bob"
  if [ "$(alice ljos seat 2>/dev/null | awk '$1 == "holder" {print $2}')" = \
       "$(bob ljos seat 2>/dev/null | awk '$1 == "holder" {print $2}')" ]; then
    fail "the two seats have two holders" "both hold the same name"
  else
    pass "the two seats have two holders"
  fi

  # --------------------------------------------------------------- file
  step "file the decision" alice ljos file "Pick a fuse" -p demo -t decision --options "keep,drop"
  ID=$(grep -Eo '^[a-z0-9]+-[a-z0-9]+' "$LAST" | head -n 1)
  step "file a second ticket" alice ljos file "Check the fuse docs" -p demo -t task
  ID2=$(grep -Eo '^[a-z0-9]+-[a-z0-9]+' "$LAST" | head -n 1)
  if [ -z "$ID" ] || [ -z "$ID2" ]; then
    fail "file printed ticket ids" "no id in the output"
    ID=${ID:-missing-id}
    ID2=${ID2:-missing-id}
  else
    pass "file printed ticket ids"
  fi
  step "tracker shows the ticket" alice vissue show "$ID"

  # -------------------------------------------------------------- claim
  step "alice claims the decision" alice ljos claim "$ID" --assignee alice
  step "bob claims the second ticket" bob ljos claim "$ID2" --assignee bob
  step "claims lists both holders" alice ljos claims
  expect "claims names alice" "$LAST" "alice"
  expect "claims names bob" "$LAST" "bob"
  # A second claim on a held ticket must be refused and must name the holder.
  if bob ljos claim "$ID" --assignee bob > "$LOGS/refused-claim.log" 2>&1; then
    fail "a held ticket refuses a second claim" "bob's claim was accepted"
  else
    expect "a held ticket refuses a second claim" "$LOGS/refused-claim.log" "alice"
  fi

  # --------------------------------------------------------------- vote
  step "alice votes" alice ljos vote "$ID" --for keep --used none
  step "bob votes" bob ljos vote "$ID" --for keep --used none
  step "tally shows two ballots" alice ljos vote "$ID"
  expect "tally holds alice's ballot" "$LAST" "alice[[:space:]]+keep"
  expect "tally holds bob's ballot" "$LAST" "bob[[:space:]]+keep"

  # ---------------------------------------------------------- consensus
  step "consensus" alice ljos consensus "$ID" --no-file
  expect "consensus counts two voters" "$LAST" "2 voters"
  expect "consensus settles on keep" "$LAST" "keep 1\\.0"

  # --------------------------------------------------------------- note
  step "alice notes the decision" alice ljos note "$ID" "alice: the fuse patch is one file."
  step "bob notes the decision" bob ljos note "$ID" "bob: the docs match the patch."

  # ------------------------------------------------------------- recall
  step "recall" bob ljos recall "$ID"
  expect "recall carries alice's note" "$LAST" "the fuse patch is one file"
  expect "recall carries bob's note" "$LAST" "the docs match the patch"

  # ------------------------------------------------------------- digest
  step "digest" alice ljos digest
  expect "digest names both seats" "$LAST" "alice"
  expect "digest names the decision" "$LAST" "$ID"

  # ------------------------------------------------------------- finish
  step "alice finishes" alice ljos finish "$ID" --assignee alice --lesson "The fuse patch is one file." --outcome keep
  step "bob finishes" bob ljos finish "$ID2" --assignee bob --lesson "The docs match the patch."

  # ----------------------------------------------------------- offboard
  OFF_ARGS=$OFFBOARD_ARGS
  if [ -z "$OFF_ARGS" ]; then
    help=$(ljos offboard --help 2>&1 || true)
    for flag in --purge --yes; do
      case $help in *"$flag"*) OFF_ARGS="$OFF_ARGS $flag" ;; esac
    done
  fi
  # shellcheck disable=SC2086
  step "offboard" ljos offboard $OFF_ARGS

  # ----------------------------------------------------------- clean HOME
  sleep 1
  snapshot "$HOME" "$RUN/after-offboard"
  # A path the install left may be gone: offboard may remove the programs.
  # A path that is new, or a file whose content changed, is a leftover.
  # Only the top of each new tree is listed, with the count under it.
  awk 'NR == FNR { had[$2] = 1; next } !($2 in had) { print $2 }' \
    "$RUN/after-install.paths" "$RUN/after-offboard.paths" | sort > "$RUN/new-paths.txt"
  awk 'NR == FNR { sum[$2] = $1; next } ($2 in sum) && sum[$2] != $1 { print $2 }' \
    "$RUN/after-install.sums" "$RUN/after-offboard.sums" | sort > "$RUN/changed-files.txt"
  : > "$RUN/leftovers.txt"
  while IFS= read -r path; do
    parent=$(dirname "$path")
    if ! grep -qxF "$parent" "$RUN/new-paths.txt"; then
      n=$(grep -cE "^$(printf '%s' "$path" | sed 's/[][\.*^$/]/\\&/g')(/|$)" "$RUN/new-paths.txt")
      printf '%s (%s)\n' "$path" "$n" >> "$RUN/leftovers.txt"
    fi
  done < "$RUN/new-paths.txt"
  sed 's/^/changed: /' "$RUN/changed-files.txt" >> "$RUN/leftovers.txt"
  if [ -s "$RUN/leftovers.txt" ]; then
    total=$(( $(wc -l < "$RUN/new-paths.txt") + $(wc -l < "$RUN/changed-files.txt") ))
    fail "HOME is back to its state after the install" "$total new or changed paths"
  else
    pass "HOME is back to its state after the install"
  fi
  procs=$(my_procs)
  if [ -n "$procs" ]; then
    printf '%s\n' "$procs" > "$RUN/leftover-procs.txt"
    fail "no process of this HOME is running" "$(printf '%s\n' "$procs" | wc -l | tr -d ' ') left"
  else
    pass "no process of this HOME is running"
  fi
  if [ "$SEED" = 1 ]; then
    STEP_OK=1
    expect "claude settings are as the user left them" "$HOME/.claude/settings.json" '"model": "keep-me"'
    if grep -q 'ljos' "$HOME/.claude/settings.json" 2>/dev/null; then
      fail "claude settings hold no ljos entry" "ljos is still named there"
    else
      pass "claude settings hold no ljos entry"
    fi
    expect "cursor mcp keeps the user's server" "$HOME/.cursor/mcp.json" 'other-server'
    expect "codex config keeps the user's line" "$HOME/.codex/config.toml" 'keep-me'
  fi
  exit 0
fi

# --------------------------------------------------------------- parent --
TARBALL=
CARGO_TARGET=
COMPANIONS=
ROOT=${FRESH_INSTALL_ROOT:-$REPO/target/fresh-install}
PORT=
SEED=0
OFFBOARD_ARGS=
KEEP=0
DETACH=1

while [ $# -gt 0 ]; do
  case $1 in
    --tarball) [ $# -ge 2 ] || die "--tarball needs a file"; TARBALL=$2; shift 2 ;;
    --cargo-target) [ $# -ge 2 ] || die "--cargo-target needs a directory"; CARGO_TARGET=$2; shift 2 ;;
    --companions) [ $# -ge 2 ] || die "--companions needs a directory"; COMPANIONS=$2; shift 2 ;;
    --root) [ $# -ge 2 ] || die "--root needs a directory"; ROOT=$2; shift 2 ;;
    --port) [ $# -ge 2 ] || die "--port needs a number"; PORT=$2; shift 2 ;;
    --seed-configs) SEED=1; shift ;;
    --offboard-args) [ $# -ge 2 ] || die "--offboard-args needs a string"; OFFBOARD_ARGS=$2; shift 2 ;;
    --keep) KEEP=1; shift ;;
    --no-detach) DETACH=0; shift ;;
    --help|-h) usage; exit 0 ;;
    *) die "unknown option $1 (try --help)" ;;
  esac
done

[ -z "$TARBALL" ] || [ -z "$CARGO_TARGET" ] || die "give --tarball or --cargo-target, not both"
case $(uname -s) in Linux) ;; *) die "ljos builds and runs on Linux only today" ;; esac
for tool in find sha256sum pgrep tar awk; do
  command -v "$tool" > /dev/null 2>&1 || die "needs $tool"
done

mkdir -p "$ROOT" || die "cannot make $ROOT"
ROOT=$(CDPATH= cd -- "$ROOT" && pwd -P)
for t in /tmp /var/tmp /dev/shm "${TMPDIR:-/tmp}"; do
  case $ROOT in "$t"|"$t"/*) die "$ROOT is under $t; ljos stands down when HOME is a temporary directory. Pass --root outside it" ;; esac
done

port=${PORT:-8761}
if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2> /dev/null; then
  die "something already answers on 127.0.0.1:$port; stop it, or pass --port with a free port"
fi

RUN=$(mktemp -d "$ROOT/run.XXXXXX")
STAGE=$RUN/stage
mkdir -p "$STAGE" "$RUN/home" "$RUN/runtime"
chmod 700 "$RUN/runtime"

if [ -n "$TARBALL" ]; then
  [ -f "$TARBALL" ] || die "no such tarball: $TARBALL"
  mkdir -p "$RUN/unpack"
  tar -xzf "$TARBALL" -C "$RUN/unpack" || die "cannot unpack $TARBALL"
  for p in ljos ljos-mcp; do
    found=$(find "$RUN/unpack" -type f -name "$p" | head -n 1)
    [ -n "$found" ] || die "$TARBALL holds no $p"
    cp "$found" "$STAGE/$p"
  done
  SOURCE_DESC="release tarball $TARBALL"
elif [ -n "$CARGO_TARGET" ]; then
  for p in ljos ljos-mcp; do
    [ -x "$CARGO_TARGET/$p" ] || die "$CARGO_TARGET/$p is missing"
    cp "$CARGO_TARGET/$p" "$STAGE/$p"
  done
  SOURCE_DESC="cargo build in $CARGO_TARGET"
else
  say "building ljos from $REPO"
  (cd "$REPO" && cargo build --locked --release -p ljos) || die "cargo build failed"
  for p in ljos ljos-mcp; do
    cp "$REPO/target/release/$p" "$STAGE/$p"
  done
  SOURCE_DESC="cargo build of $REPO ($(git -C "$REPO" rev-parse --short HEAD 2> /dev/null || echo unknown))"
fi
chmod 755 "$STAGE/ljos" "$STAGE/ljos-mcp"
WANT_VERSION=$("$STAGE/ljos" --version | awk '{print $2}')
[ -n "$WANT_VERSION" ] || die "the ljos to install does not print a version"

if [ -n "$COMPANIONS" ]; then
  COMPANIONS=$(CDPATH= cd -- "$COMPANIONS" && pwd) || die "no such directory: $COMPANIONS"
fi

# Only the entries the child needs: the bin dir under test, git, and cargo
# for the source build install.sh falls back to where a program has no release.
CHILD_PATH=$RUN/home/.local/bin:$(dirname "$(command -v git)"):/usr/local/bin:/usr/bin:/bin
command -v cargo > /dev/null 2>&1 && CHILD_PATH=$CHILD_PATH:$(dirname "$(command -v cargo)")
{
  printf 'HOME=%q\n' "$RUN/home"
  printf 'PATH=%q\n' "$CHILD_PATH"
  printf 'XDG_RUNTIME_DIR=%q\n' "$RUN/runtime"
  printf 'REPO=%q\n' "$REPO"
  printf 'STAGE=%q\n' "$STAGE"
  printf 'COMPANIONS=%q\n' "$COMPANIONS"
  printf 'SOURCE_DESC=%q\n' "$SOURCE_DESC"
  printf 'WANT_VERSION=%q\n' "$WANT_VERSION"
  printf 'PORT=%q\n' "$PORT"
  printf 'SEED=%q\n' "$SEED"
  [ -z "${CARGO_HOME:-}" ] || printf 'export CARGO_HOME=%q\n' "$CARGO_HOME"
  [ -z "${RUSTUP_HOME:-}" ] || printf 'export RUSTUP_HOME=%q\n' "$RUSTUP_HOME"
  printf 'OFFBOARD_ARGS=%q\n' "$OFFBOARD_ARGS"
} > "$RUN/config.sh"

stop_writers() {
  pkill -f -- "$RUN/home" 2> /dev/null || true
}

if [ "$DETACH" = 1 ] && command -v setsid > /dev/null 2>&1; then
  setsid -f env -i bash "$SELF" --child "$RUN" > "$RUN/out.log" 2>&1 < /dev/null
  # The child writes DONE as its last act.
  tail -n +1 -F "$RUN/out.log" 2> /dev/null &
  tailer=$!
  i=0
  while [ ! -f "$RUN/DONE" ] && [ "$i" -lt 3600 ]; do
    sleep 1
    i=$((i + 1))
  done
  sleep 1
  kill "$tailer" 2> /dev/null
  wait "$tailer" 2> /dev/null
else
  env -i bash "$SELF" --child "$RUN" 2>&1 | tee "$RUN/out.log"
fi

stop_writers
say
say "Summary"
if [ ! -s "$RUN/results" ]; then
  say "  no step ran; see $RUN/out.log"
  exit 2
fi
passed=$(grep -c '^ok' "$RUN/results")
failed=$(grep -c '^fail' "$RUN/results")
skipped=$(grep -c '^skip' "$RUN/results")
say "  $passed passed, $failed failed, $skipped skipped (their step failed first)"
if [ "$failed" -gt 0 ]; then
  say
  say "Failed steps:"
  awk -F'\t' '$1 == "fail" { n++; printf "  %d. %s (%s)\n", n, $2, $3 }' "$RUN/results"
  if [ -s "$RUN/leftovers.txt" ]; then
    say
    say "Left in HOME after offboard (top of each tree, with the count of paths in it):"
    sed 's/^/  /' "$RUN/leftovers.txt" | head -n 40
  fi
  if [ -s "$RUN/leftover-procs.txt" ]; then
    say
    say "Processes still running for this HOME:"
    sed 's/^/  /' "$RUN/leftover-procs.txt"
  fi
  say
  say "Logs: $RUN/logs"
  exit 1
fi
if [ "$KEEP" = 1 ]; then
  say "Logs: $RUN/logs"
else
  rm -rf "$RUN"
fi
exit 0
