#!/bin/sh
# Grok Build does not add UserPromptSubmit additionalContext to the model.
# ljos hook is silent on PreToolUse. Remap stdin to UserPromptSubmit so the
# pack injects, then emit PreToolUse additionalContext which Grok does deliver.
set -eu
LJOS_BIN="${LJOS_BIN:-$(command -v ljos)}"
input=$(cat)
event=$(printf '%s' "$input" | python3 -c 'import json,sys
try:
    d=json.load(sys.stdin)
except Exception:
    print("UserPromptSubmit")
    raise SystemExit
print(d.get("hook_event_name") or d.get("hookEventName") or "UserPromptSubmit")')

# Always query the pack as a prompt event so ljos hook speaks.
query=$(printf '%s' "$input" | python3 -c 'import json,sys
d=json.load(sys.stdin)
bits=["Call ljos sitting. Never weaken tests. CI red is a defect.",
      d.get("prompt") or "",
      str(d.get("toolName") or ""),
      str((d.get("toolInput") or {}).get("command") or d.get("toolInput") or "")]
print(" ".join(bits)[:2000])')

payload=$(python3 -c 'import json,sys
print(json.dumps({"hook_event_name":"UserPromptSubmit","prompt":sys.argv[1]}))' "$query")

out=$(printf '%s' "$payload" | "$LJOS_BIN" hook --limit 8 || true)
ctx=$(printf '%s' "$out" | python3 -c 'import json,sys
raw=sys.stdin.read()
try:
    d=json.loads(raw)
except Exception:
    print("")
    raise SystemExit
print((d.get("hookSpecificOutput") or {}).get("additionalContext") or "")')

if [ -z "$ctx" ]; then
  exit 0
fi

# SessionStart stdout is ignored by Grok. Write a brief the sitting can open.
if [ "$event" = "SessionStart" ] || [ "$event" = "session_start" ]; then
  printf '%s\n' "$ctx" > "${XDG_RUNTIME_DIR:-/tmp}/ljos-grok-brief.txt"
  exit 0
fi

python3 -c 'import json,sys
ctx=sys.argv[1]
event=sys.argv[2]
print(json.dumps({
  "hookSpecificOutput": {
    "hookEventName": event if event in ("PreToolUse","UserPromptSubmit","Stop") else "PreToolUse",
    "additionalContext": ctx,
  }
}))' "$ctx" "$event"
exit 0
