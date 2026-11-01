#!/usr/bin/env bash
# Two ljos-mcp connections, two seats. One process is one session: a seat
# argument does not mint a second ballot, and the session that filed a
# design candidate cannot vote on that panel.
set -uo pipefail
unset LJOS_SEAT VISSUE_AGENT LJOS_PANE_TOOL LJOS_HARNESS_SESSION LJOS_PROCESS_NONCE LJOS_PANEL_CHILD LJOS_MEMBER_BIN LJOS_PANEL_PERSONAS LJOS_OFF
if [ "${PACKSET_URL:-}" = off ]; then unset PACKSET_URL; fi
for k in $(env | sed -n 's/^\([A-Za-z0-9_]*_\(SESSION\|THREAD\|CONVERSATION\)_ID\)=.*/\1/p'); do unset "$k"; done

BIN=${SUITE_BIN:-}
if [ -z "$BIN" ]; then
  echo "mcp-seats-e2e: set SUITE_BIN to a directory with ljos, ljos-mcp, vissue, packset and claimdag" >&2
  exit 1
fi
export PATH="$BIN:$PATH"
command -v ljos-mcp >/dev/null && command -v ljos >/dev/null && command -v vissue >/dev/null && command -v claimdag >/dev/null

root=$(mktemp -d "${TMPDIR:-/tmp}/mcp-seats.XXXX")
echo "root $root"
export VISSUE_ROOT="$root/tracker" CLAIMDAG_DIR="$root/claims" LJOS_TRACKER_GIT=commit
export LJOS_PROJECT=demo XDG_RUNTIME_DIR="$root/run" XDG_STATE_HOME="$root/state"
export XDG_CONFIG_HOME="$root/config" XDG_DATA_HOME="$root/data"
mkdir -p "$VISSUE_ROOT" "$root/run" "$root/repo" "$root/state" "$root/home-alice" "$root/home-bob"
git -C "$VISSUE_ROOT" init -q
git -C "$VISSUE_ROOT" config user.email seat@example.invalid
git -C "$VISSUE_ROOT" config user.name seat
git -C "$VISSUE_ROOT" config core.hooksPath /dev/null
git -C "$root/repo" init -q
git -C "$root/repo" config user.email seat@example.invalid
git -C "$root/repo" config user.name seat
git -C "$root/repo" config core.hooksPath /dev/null
printf 'seed\n' > "$root/repo/README.md"
git -C "$root/repo" add README.md
git -C "$root/repo" -c core.hooksPath=/dev/null commit -qm seed
printf 'seats = ["alice", "bob", "carol"]\n' > "$VISSUE_ROOT/vissue.toml"

# The loopback writer already on 8761 belongs to another home, and its
# token is not this scratch's. A seat's HOME is also not the writer's
# home, so the client would send the wrong bearer and get 401. Start a
# writer whose home and port are this run's, and point every process at it.
export PACKSET_HOME="$root/pack"
mkdir -p "$PACKSET_HOME"
if [ -z "${PACKSET_URL:-}" ]; then
  PACKSET_PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
  export PACKSET_PORT
  packset ensure >"$root/packset.url" 2>"$root/packset.err" || true
  url=$(sed -n 's/^PACKSET_URL=//p' "$root/packset.url" | head -1)
  if [ -n "$url" ]; then export PACKSET_URL="$url"; fi
fi
export PACKSET_WORKSPACE="mcp-seats:$(basename "$root")"
echo "pack ${PACKSET_URL:-unset} home $PACKSET_HOME"

export MCP_ROOT="$root"
python3 - << 'PY'
import json, os, re, subprocess, sys

root = os.environ["MCP_ROOT"]
bin_mcp = "ljos-mcp"
env_base = os.environ.copy()
for key in list(env_base):
    if key.endswith("_SESSION_ID") or key.endswith("_THREAD_ID") or key.endswith("_CONVERSATION_ID"):
        env_base.pop(key, None)
env_base.pop("LJOS_SEAT", None)
env_base.pop("LJOS_HARNESS_SESSION", None)
env_base.pop("LJOS_PROCESS_NONCE", None)

fails = []

def check(name, ok, detail=""):
    if ok:
        print(f"PASS {name}")
    else:
        print(f"FAIL {name} :: {detail}")
        fails.append(name)

class Seat:
    def __init__(self, name, session):
        self.name = name
        self.session = session
        self.n = 0
        env = env_base.copy()
        env["HOME"] = os.path.join(root, f"home-{name}")
        env["LJOS_HARNESS_SESSION"] = session
        env["LJOS_PROCESS_NONCE"] = f"nonce-{name}-0001"
        self.proc = subprocess.Popen(
            [bin_mcp],
            cwd=os.path.join(root, "repo"),
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.call("initialize", {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "mcp-seats", "version": "0"},
        }, note=False)
        self.notify("notifications/initialized", {})

    def send(self, msg):
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()

    def notify(self, method, params):
        self.send({"jsonrpc": "2.0", "method": method, "params": params})

    def call(self, method, params, note=True):
        self.n += 1
        ident = self.n
        self.send({"jsonrpc": "2.0", "id": ident, "method": method, "params": params})
        while True:
            line = self.proc.stdout.readline()
            if line == "":
                err = self.proc.stderr.read()
                raise SystemExit(f"{self.name} closed on {method}\n{err}")
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            if msg.get("id") == ident:
                return msg

    def tool(self, name, args):
        body = dict(args)
        body["seat"] = self.name
        msg = self.call("tools/call", {"name": name, "arguments": body})
        result = msg.get("result") or {}
        texts = []
        for block in result.get("content") or []:
            if isinstance(block, dict) and block.get("type") == "text":
                texts.append(block.get("text") or "")
        text = "\n".join(texts)
        err = msg.get("error")
        bad = bool(err) or result.get("isError") is True
        return bad, text if text else json.dumps(msg)

    def close(self):
        try:
            self.proc.stdin.close()
        except Exception:
            pass
        try:
            self.proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            self.proc.kill()

def issue_of(text):
    m = re.search(r"\b([a-z]+-[a-z0-9]{4})\b", text)
    return m.group(1) if m else ""

alice = Seat("alice", "sess-alice-mcp")
bob = Seat("bob", "sess-bob-mcp")

bad, text = alice.tool("ljos_start", {
    "task": "Add a review line for the demo.",
    "repo": os.path.join(root, "repo"),
})
task = issue_of(text)
check("alice starts and claims", (not bad) and task and "claimed" in text, text)

bad, text = alice.tool("ljos_remember", {
    "text": "Quartz counters scratch when a knife cuts on them.",
    "transient": False,
})
check("alice writes a memory", not bad and "id" in text, text)

bad, text = alice.tool("ljos_note", {
    "issue": task,
    "text": "The line is one sentence.",
})
check("alice notes the issue", (not bad) and "noted" in text, text)

bad, text = alice.tool("ljos_finish", {
    "issue": task,
    "lesson": "The review line names the seat. The note is one sentence.",
})
check("alice finishes", (not bad) and "completed" in text, text)

bad, text = alice.tool("ljos_file", {
    "title": "Add the seat to the review line",
    "top": True,
    "project": "demo",
    "issue_kind": "task",
})
work = issue_of(text)
check("alice files a vissue", (not bad) and bool(work), text)

bad, text = bob.tool("ljos_start", {
    "issue": work,
    "task": "Pick up the review line.",
    "repo": os.path.join(root, "repo"),
})
check("bob picks up the filed issue", (not bad) and work in text and "claimed" in text, text)

bad, text = bob.tool("ljos_note", {
    "issue": work,
    "text": "review: the line names the seat and stays one sentence.",
})
check("bob reviews", (not bad) and "noted" in text, text)

bad, text = bob.tool("ljos_finish", {
    "issue": work,
    "lesson": "The review passed. The line names the seat.",
    "close": True,
})
check("bob closes", (not bad) and "closed the ticket" in text, text)

bad, text = alice.tool("ljos_file", {
    "title": "Ship the review line now or hold?",
    "top": True,
    "project": "demo",
    "issue_kind": "decision",
    "body": "Options: ship, hold",
})
panel = issue_of(text)
check("alice files the panel", (not bad) and bool(panel), text)

bad, text = alice.tool("ljos_file", {
    "title": "design: stamp the seat on the logbook",
    "parent": panel,
    "project": "demo",
    "issue_kind": "task",
})
cand = issue_of(text)
check("alice files the design candidate", (not bad) and "bound to session" in text and bool(cand), text)

bad, text = alice.tool("ljos_vote", {
    "issue": panel,
    "choice": "ship",
    "used": "none",
})
check("the author's vote is refused", bad and "authored candidate" in text and "Nothing was cast" in text, text)

bad, text = bob.tool("ljos_vote", {
    "issue": panel,
    "choice": "ship",
    "used": "none",
})
check("the other seat's vote is cast", (not bad) and "ship" in text, text)

bad, text = alice.tool("ljos_file", {
    "title": "Keep the line or drop it?",
    "top": True,
    "project": "demo",
    "issue_kind": "decision",
    "body": "Options: keep, drop",
})
second = issue_of(text)
check("alice files a second decision", (not bad) and bool(second), text)

bad, text = alice.tool("ljos_vote", {
    "issue": second,
    "choice": "keep",
    "used": "none",
})
check("alice casts one ballot", not bad and "keep" in text, text)

# Same connection, a different seat argument. The pin is the session.
alice.name = "carol"
bad, text = alice.tool("ljos_vote", {
    "issue": second,
    "choice": "drop",
    "used": "none",
})
check("one connection cannot cast as another seat", bad and "already cast" in text and "Nothing was cast" in text, text)

alice.close()
bob.close()

log = subprocess.run(
    ["git", "-C", os.environ["VISSUE_ROOT"], "log", "--format=%s"],
    check=False, text=True, capture_output=True,
)
subjects = log.stdout
check("tracker commits name alice", "by alice" in subjects, subjects)
check("tracker commits name bob", "by bob" in subjects, subjects)

usage = subprocess.run(
    ["ljos", "usage", "--since", "2026-10-16"],
    check=False, text=True, capture_output=True, env=env_base,
)
shown = usage.stdout
check("usage counts alice", "alice" in shown and not shown.startswith("no tracker"), shown + usage.stderr)
check("usage counts bob", "bob" in shown, shown + usage.stderr)

listed = subprocess.run(
    ["ljos", "memories", "list"],
    check=False, text=True, capture_output=True,
    env={**env_base, "LJOS_SEAT": "alice", "HOME": os.path.join(root, "home-alice")},
)
check(
    "the memory the MCP wrote is on the list",
    "Quartz counters scratch when a knife cuts on them." in listed.stdout,
    listed.stdout + listed.stderr,
)

print(f"root {root}")
sys.exit(1 if fails else 0)
PY
rc=$?
echo "root $root"
exit "$rc"
