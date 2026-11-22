#!/usr/bin/env bash
# Proof that a prompt which never names ljos leaves nothing on the
# tracker. No hook files a ticket, takes a claim, cites a deed or
# finishes a claim; the seat calls those itself. One session per
# harness. The payloads are the ones each harness actually posts to
# `ljos hook`. The fake vissue and deedar log every call, so a hook that
# wrote would show up in those logs.
# shellcheck disable=SC2317 # each drive_ function is called as drive_$name
set -euo pipefail

root=$(mktemp -d "${TMPDIR:-/tmp}/ljos-book.XXXX")
echo "root $root"
tracker="$root/tracker"
repo="$root/repo"
bin="$root/bin"
run="$root/run"
mkdir -p "$tracker" "$repo" "$bin" "$run"

here=$(cd "$(dirname "$0")" && pwd)
LJOS=${LJOS:-}
if [ -z "$LJOS" ]; then
  if [ -x "$here/../target/debug/ljos" ]; then
    LJOS="$here/../target/debug/ljos"
  else
    LJOS=$(command -v ljos)
  fi
fi

git -C "$tracker" init -q
git -C "$tracker" config user.email seat@example.invalid
git -C "$tracker" config user.name seat
git -C "$tracker" config core.hooksPath /dev/null
git -C "$repo" init -q
git -C "$repo" config user.email seat@example.invalid
git -C "$repo" config user.name seat
git -C "$repo" config core.hooksPath /dev/null
printf 'seed\n' > "$repo/README.md"
git -C "$repo" add README.md
git -C "$repo" commit -q -m seed

cat > "$bin/vissue" << 'PY'
#!/usr/bin/env python3
import os, sys, pathlib
root = pathlib.Path(os.environ["VISSUE_ROOT"])
args = sys.argv[1:]
with (root / "vissue.log").open("a") as fh:
    fh.write(" ".join(args) + "\n")
cmd = args[0] if args else ""
if cmd in ("claims", "children", "list", "ready"):
    print("[]")
elif cmd == "show":
    print("{}")
PY
cat > "$bin/deedar" << 'PY'
#!/usr/bin/env python3
import os, sys, pathlib
root = pathlib.Path(os.environ["VISSUE_ROOT"])
with (root / "deedar.log").open("a") as fh:
    fh.write(" ".join(sys.argv[1:]) + "\n")
PY
chmod 755 "$bin/vissue" "$bin/deedar"

export PATH="$bin:$PATH"
export VISSUE_ROOT="$tracker" VISSUE_NO_ROUTE=1 LJOS_TRACKER_GIT=commit
export LJOS_PROJECT=probe XDG_RUNTIME_DIR="$run" PACKSET_URL=off LJOS_HOOKD=off
unset LJOS_OFF LJOS_SESSION_ID || true

fail=0
say() { printf '%s\n' "$*"; }
need() {
  local name=$1
  shift
  if "$@"; then
    say "PASS $name"
  else
    say "FAIL $name"
    fail=1
  fi
}

# One harness session: prompt, edit, commit, stop, end.
# $1 session id, $2 prompt (must not name the seat binary), $3 event.
claude_shape() {
  local session=$1 prompt=$2 event=$3
  shift 3
  python3 - "$session" "$prompt" "$event" "$repo" "$@" << 'PY'
import json, sys
session, prompt, event, cwd = sys.argv[1:5]
extra = json.loads(sys.argv[5]) if len(sys.argv) > 5 else {}
body = {"session_id": session, "cwd": cwd, "hook_event_name": event, "prompt": prompt}
body.update(extra)
if event != "UserPromptSubmit":
    body.pop("prompt", None)
print(json.dumps(body))
PY
}

run_hook() {
  local payload=$1
  shift
  printf '%s' "$payload" | "$LJOS" hook "$@" >/dev/null
}

drive_claude() {
  local session=$1 prompt=$2
  run_hook "$(claude_shape "$session" "$prompt" UserPromptSubmit)"
  run_hook "$(claude_shape "$session" "$prompt" SessionStart)"
  run_hook "$(claude_shape "$session" "$prompt" PreToolUse '{"tool_name":"Edit","tool_input":{"file_path":"README.md"}}')"
  run_hook "$(claude_shape "$session" "$prompt" PreToolUse '{"tool_name":"Bash","tool_input":{"command":"git commit -m \"readme\""}}')"
  run_hook "$(claude_shape "$session" "$prompt" PostToolUse '{"tool_name":"Bash","tool_input":{"command":"git commit -m \"readme\""}}')"
  run_hook "$(claude_shape "$session" "$prompt" Stop)"
  run_hook "$(claude_shape "$session" "$prompt" SessionEnd)"
}

drive_codex() {
  local session=$1 prompt=$2
  local turn="${session}-turn"
  codex_body() {
    local event=$1 extra=$2
    python3 - "$session" "$turn" "$prompt" "$event" "$repo" "$extra" << 'PY'
import json, sys
session, turn, prompt, event, cwd, extra = sys.argv[1:7]
body = {"session_id": session, "turn_id": turn, "cwd": cwd, "hook_event_name": event}
if event == "UserPromptSubmit":
    body["prompt"] = prompt
body.update(json.loads(extra))
print(json.dumps(body))
PY
  }
  run_hook "$(codex_body UserPromptSubmit '{}')"
  run_hook "$(codex_body SessionStart '{}')"
  run_hook "$(codex_body PreToolUse '{"tool_name":"apply_patch","tool_input":{"file_path":"README.md"}}')"
  run_hook "$(codex_body PreToolUse '{"tool_name":"shell","tool_input":{"command":"git -C . commit -m \"readme\""}}')"
  run_hook "$(codex_body PostToolUse '{"tool_name":"shell","tool_input":{"command":"git -C . commit -m \"readme\""},"tool_response":{"exit_code":0}}')"
  run_hook "$(codex_body Stop '{}')"
  run_hook "$(codex_body SessionEnd '{}')"
}

drive_opencode() {
  local session=$1 prompt=$2
  # What assets/opencode/ljos.ts posts: prompt, then SessionStart on the
  # first prompt, PreToolUse for an edit and for bash, PostToolUse, Stop
  # on session.idle, SessionEnd on session.deleted.
  drive_claude "$session" "$prompt"
}

drive_agy() {
  local session=$1 prompt=$2
  local transcript=$root/agy-$session.jsonl
  python3 - "$transcript" "$prompt" << 'PY'
import json, sys
path, prompt = sys.argv[1:]
open(path, "w").write(json.dumps({"role": "user", "text": prompt}) + "\n")
PY
  agy() {
    local event=$1 body=$2
    printf '%s' "$body" | "$LJOS" hook --event "$event" >/dev/null
  }
  agy PreInvocation "$(python3 - "$session" "$transcript" "$repo" << 'PY'
import json, sys
session, transcript, cwd = sys.argv[1:]
print(json.dumps({"conversationId": session, "invocationNum": 1, "transcriptPath": transcript, "cwd": cwd}))
PY
)"
  agy PreToolUse "$(python3 - "$session" "$repo" << 'PY'
import json, sys
session, cwd = sys.argv[1:]
print(json.dumps({"conversationId": session, "cwd": cwd, "toolCall": {"name": "edit", "args": {"TargetFile": "README.md"}}}))
PY
)"
  agy PreToolUse "$(python3 - "$session" "$repo" << 'PY'
import json, sys
session, cwd = sys.argv[1:]
print(json.dumps({"conversationId": session, "cwd": cwd, "toolCall": {"name": "bash", "args": {"CommandLine": "git commit -m \"readme\""}}}))
PY
)"
  agy PostToolUse "$(python3 - "$session" "$repo" << 'PY'
import json, sys
session, cwd = sys.argv[1:]
print(json.dumps({"conversationId": session, "cwd": cwd, "toolCall": {"name": "bash", "args": {"CommandLine": "git commit -m \"readme\""}}, "tool_response": {"exit_code": 0}}))
PY
)"
  agy Stop "$(python3 - "$session" "$repo" << 'PY'
import json, sys
print(json.dumps({"conversationId": sys.argv[1], "cwd": sys.argv[2], "executionNum": 1}))
PY
)"
}

drive_cursor() {
  local session=$1 prompt=$2
  cursor_body() {
    local event=$1 extra=$2
    python3 - "$session" "$prompt" "$event" "$repo" "$extra" << 'PY'
import json, sys
session, prompt, event, cwd, extra = sys.argv[1:6]
body = {"cursor_version": "1.0.0", "session_id": session, "cwd": cwd, "hook_event_name": event}
if event == "beforeSubmitPrompt":
    body["prompt"] = prompt
body.update(json.loads(extra))
print(json.dumps(body))
PY
  }
  run_hook "$(cursor_body sessionStart '{}')"
  run_hook "$(cursor_body beforeSubmitPrompt '{}')"
  run_hook "$(cursor_body preToolUse '{"tool_name":"Write","tool_input":{"file_path":"README.md"}}')"
  run_hook "$(cursor_body beforeShellExecution '{"command":"git commit -m \"readme\""}')"
  run_hook "$(cursor_body postToolUse '{"tool_name":"Shell","tool_input":{"command":"git commit -m \"readme\""},"tool_response":{"exit_code":0}}')"
  run_hook "$(cursor_body stop '{}')"
  run_hook "$(cursor_body sessionEnd '{}')"
}

prompts=(
  "claude|Add a readme line for the claude harness"
  "codex|Add a readme line for the codex harness"
  "opencode|Add a readme line for the opencode harness"
  "agy|Add a readme line for the agy harness"
  "cursor|Add a readme line for the cursor harness"
)

for row in "${prompts[@]}"; do
  name=${row%%|*}
  prompt=${row#*|}
  case "$prompt" in
    *ljos*) echo "prompt names the seat: $prompt" >&2; exit 1 ;;
  esac
  say "drive $name"
  "drive_$name" "book-$name" "$prompt"
done

touch "$tracker/vissue.log" "$tracker/deedar.log"
say "---- vissue calls ----"
cat "$tracker/vissue.log"
say "---- deedar calls ----"
cat "$tracker/deedar.log"

writes='^(create|q|claim|update|deed|note|append|close|cancel|release|refile)( |$)'
tracker_writes=$(grep -cE "$writes" "$tracker/vissue.log" || true)
deed_calls=$(grep -c . "$tracker/deedar.log" || true)
commits=$(git -C "$tracker" rev-list --all --count)
tickets=$(find "$tracker" -name issues.org | wc -l)
need "no hook wrote the tracker" test "$tracker_writes" -eq 0
need "no hook minted a deed" test "$deed_calls" -eq 0
need "the tracker has no commit" test "$commits" -eq 0
need "no hook filed a ticket" test "$tickets" -eq 0

say "root $root"
exit "$fail"
