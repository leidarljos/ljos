#!/usr/bin/env bash
# Proof that a prompt which never names ljos still leaves a claim, a deed
# and a finish on the tracker. One session per harness. The payloads are
# the ones each harness actually posts to `ljos hook`.
set -euo pipefail

root=$(mktemp -d "${TMPDIR:-/tmp}/ljos-book.XXXX")
echo "root $root"
tracker="$root/tracker"
repo="$root/repo"
bin="$root/bin"
run="$root/run"
mkdir -p "$tracker" "$repo" "$bin" "$run" "$repo"

LJOS=${LJOS:-}
if [ -z "$LJOS" ]; then
  if [ -x /workspace/target/debug/ljos ]; then
    LJOS=/workspace/target/debug/ljos
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
import os, sys, pathlib, re
root = pathlib.Path(os.environ["VISSUE_ROOT"])
log = root / "vissue.log"
args = sys.argv[1:]
with log.open("a") as fh:
    fh.write(" ".join(args) + "\n")
cmd, rest = args[0], args[1:]

def files():
    return list(root.rglob("issues.org"))

def rewrite(iid, state):
    pat = re.compile(r"(\* )[A-Z]+( \[#[A-Z]\] [^\n]*\n:PROPERTIES:\n:ID:\s+" + re.escape(iid) + r"\n)")
    for f in files():
        text = f.read_text()
        new, n = pat.subn(r"\1" + state + r"\2", text, count=1)
        if n:
            f.write_text(new)
            return
    sys.exit("no heading for " + iid)

if cmd == "create":
    proj, title, i = "agent", "agent work", 0
    while i < len(rest):
        if rest[i] == "-p" and i + 1 < len(rest):
            proj = rest[i + 1]
            i += 2
            continue
        if rest[i] == "-q":
            i += 1
            continue
        title = rest[i]
        i += 1
    seq = root / ".seq"
    n = int(seq.read_text()) + 1 if seq.exists() else 1
    seq.write_text(str(n))
    iid = f"{proj}-{n:04x}"
    dest = root / "Software" / proj
    dest.mkdir(parents=True, exist_ok=True)
    with (dest / "issues.org").open("a") as fh:
        fh.write(f"* TODO [#C] {title}\n:PROPERTIES:\n:ID:         {iid}\n:END:\n")
    print(iid)
elif cmd == "claim":
    rewrite(rest[0], "STARTED")
elif cmd == "update":
    state = rest[rest.index("-s") + 1]
    rewrite(rest[0], state)
elif cmd == "deed":
    iid, acc = rest[0], rest[rest.index("--add") + 1]
    needle = f":ID:         {iid}\n:END:\n"
    for f in files():
        text = f.read_text()
        if needle in text:
            f.write_text(text.replace(needle, needle + f"cited {acc}\n", 1))
            sys.exit(0)
    sys.exit("no issue " + iid)
elif cmd == "claims":
    print("[]")
elif cmd == "show":
    print("{}")
else:
    sys.exit("unknown " + cmd)
PY
cat > "$bin/deedar" << 'PY'
#!/usr/bin/env python3
import os, pathlib
root = pathlib.Path(os.environ["VISSUE_ROOT"])
seq = root / ".deedseq"
n = int(seq.read_text()) + 1 if seq.exists() else 1
seq.write_text(str(n))
print(f"id=deed-{n:04x}")
PY
chmod 755 "$bin/vissue" "$bin/deedar"

export PATH="$bin:$PATH"
export VISSUE_ROOT="$tracker" VISSUE_NO_ROUTE=1 LJOS_TRACKER_GIT=commit
export LJOS_PROJECT=probe XDG_RUNTIME_DIR="$run" PACKSET_URL=off
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
# $1 harness label, $2 session id, $3 prompt (must not name the seat binary).
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

log=$(git -C "$tracker" log --format='%s')
say "---- tracker log ----"
printf '%s\n' "$log"
say "---- issues ----"
grep '^\* ' "$tracker/Software/probe/issues.org" || true

claimed=$(printf '%s\n' "$log" | grep -c 'claimed' || true)
deeds=$(printf '%s\n' "$log" | grep -c 'cited a deed' || true)
finished=$(printf '%s\n' "$log" | grep -c 'finished' || true)
need "one claim per harness" test "$claimed" -eq 5
need "one deed per harness" test "$deeds" -eq 5
need "one finish per harness" test "$finished" -eq 5
for name in claude codex opencode agy cursor; do
  need "$name title is on the tracker" grep -q "Add a readme line for the $name harness" "$tracker/Software/probe/issues.org"
done

say "root $root"
exit "$fail"
