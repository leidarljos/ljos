#!/usr/bin/env bash
# Measure ljos-mcp: how many tools tools/list returns, and the o200k_base
# token cost of the initialize instructions plus that result.
#
# Two modes: the default core set, and LJOS_MCP_TOOLS=all.
# The tokenizer is tiktoken's o200k_base. The text is the instructions
# string from the initialize result, and the tools/list result object as
# the server sent it. The two are counted separately and added, so a
# token does not merge across the boundary.
#
# Writes measurements/mcp-size.txt. With --check, refuses a file or a
# README count that disagrees with a fresh run.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
check=0
if [[ "${1:-}" == "--check" ]]; then
  check=1
elif [[ -n "${1:-}" ]]; then
  echo "usage: scripts/mcp-size.sh [--check]" >&2
  exit 2
fi

if [[ -n "${LJOS_MCP:-}" ]]; then
  bin=$LJOS_MCP
elif [[ -x target/debug/ljos-mcp ]]; then
  bin=$root/target/debug/ljos-mcp
elif [[ -x target/release/ljos-mcp ]]; then
  bin=$root/target/release/ljos-mcp
elif command -v ljos-mcp >/dev/null 2>&1; then
  bin=$(command -v ljos-mcp)
else
  echo "ljos-mcp is not built. cargo build --locked -p ljos --bin ljos-mcp" >&2
  exit 1
fi

python3 - "$bin" "$root" "$check" <<'PY'
import hashlib, json, re, subprocess, sys, tempfile
from pathlib import Path

bin_path, root, check = sys.argv[1], Path(sys.argv[2]), sys.argv[3] == "1"
out_path = root / "measurements" / "mcp-size.txt"

try:
    import tiktoken
except ImportError:
    sys.exit("tiktoken is not installed (o200k_base). pip install 'tiktoken==0.12.0'")
enc = tiktoken.get_encoding("o200k_base")

ONES = ["zero","one","two","three","four","five","six","seven","eight","nine",
        "ten","eleven","twelve","thirteen","fourteen","fifteen","sixteen",
        "seventeen","eighteen","nineteen"]
TENS = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"]

def words(n: int) -> str:
    if n < 20:
        return ONES[n]
    ten, one = divmod(n, 10)
    if one == 0:
        return TENS[ten]
    return f"{TENS[ten]}-{ONES[one]}"

def comma(n: int) -> str:
    return f"{n:,}"

def ask(tools: str | None) -> tuple[str, str, list[str]]:
    home = tempfile.mkdtemp(prefix="ljos-mcp-size-")
    env = {"PATH": "/usr/bin:/bin", "HOME": home}
    if tools is not None:
        env["LJOS_MCP_TOOLS"] = tools
    lines = [
        json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "mcp-size", "version": "0"}}}),
        json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ]
    proc = subprocess.Popen(
        [bin_path], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, cwd=home)
    try:
        stdout, stderr = proc.communicate(("\n".join(lines) + "\n").encode(), timeout=60)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.communicate()
        sys.exit("ljos-mcp did not answer tools/list within 60s")
    text = stdout.decode()
    err = stderr.decode(errors="replace")
    if "panicked" in err:
        sys.exit(f"ljos-mcp panicked:\n{err}")
    by_id = {}
    raw_by_id = {}
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if "id" in obj:
            by_id[obj["id"]] = obj
            raw_by_id[obj["id"]] = line
    if 1 not in by_id or 2 not in by_id:
        sys.exit(f"ljos-mcp gave no initialize or tools/list answer\nstdout:\n{text}\nstderr:\n{err}")
    init, listed = by_id[1], by_id[2]
    if "error" in init or "error" in listed:
        sys.exit(f"ljos-mcp answered with an error\n{init}\n{listed}")
    instructions = init["result"]["instructions"]
    if not isinstance(instructions, str) or not instructions:
        sys.exit("initialize result has no instructions")
    raw = raw_by_id[2]
    found = re.search(r'"result"\s*:', raw)
    if not found:
        sys.exit("tools/list answer has no result")
    result_json = raw[found.end():].rstrip()
    # The value runs to the end of the object apart from the closing brace
    # of the envelope. raw_decode takes the value and nothing after it.
    value, end = json.JSONDecoder().raw_decode(result_json)
    result_json = result_json[:end]
    tools = value.get("tools")
    if not isinstance(tools, list) or not tools:
        sys.exit("tools/list result has no tools")
    names = [tool["name"] for tool in tools]
    return instructions, result_json, names

def measure(mode: str, tools: str | None) -> dict:
    instructions, result_json, names = ask(tools)
    inst_tokens = len(enc.encode(instructions))
    tool_tokens = len(enc.encode(result_json))
    blob = instructions.encode() + b"\n" + result_json.encode()
    return {
        "mode": mode,
        "tools": len(names),
        "tokens": inst_tokens + tool_tokens,
        "instructions_tokens": inst_tokens,
        "tools_tokens": tool_tokens,
        "sha256": hashlib.sha256(blob).hexdigest(),
        "names": names,
    }

core = measure("core", None)
full = measure("all", "all")
rows = [
    "tokenizer: o200k_base",
    "text: initialize instructions, and the tools/list result object as the server sent it",
    "total: the two token counts added",
    f"core.tools: {core['tools']}",
    f"core.tokens: {core['tokens']}",
    f"core.instructions_tokens: {core['instructions_tokens']}",
    f"core.tools_tokens: {core['tools_tokens']}",
    f"core.sha256: {core['sha256']}",
    "core.names: " + " ".join(core["names"]),
    f"all.tools: {full['tools']}",
    f"all.tokens: {full['tokens']}",
    f"all.instructions_tokens: {full['instructions_tokens']}",
    f"all.tools_tokens: {full['tools_tokens']}",
    f"all.sha256: {full['sha256']}",
    "all.names: " + " ".join(full["names"]),
    "",
]
report = "\n".join(rows)

def norm(s: str) -> str:
    return re.sub(r"\s+", " ", s).strip()

readme = (root / "README.md").read_text()
mcp = readme.split("## MCP", 1)[1].split("\n## ", 1)[0]
flat = norm(mcp)
core_clause = f"lists {words(core['tools'])} core tools"
token_clause = (
    f"{comma(core['tokens'])} tokens for the core set against "
    f"{comma(full['tokens'])} for all {words(full['tools'])}"
)
missing = []
if core_clause not in flat:
    missing.append(core_clause)
if token_clause not in flat:
    missing.append(token_clause)
for name in core["names"]:
    short = name.removeprefix("ljos_")
    if f"`{short}`" not in mcp:
        missing.append(f"`{short}`")

if check:
    have = out_path.read_text() if out_path.is_file() else ""
    if have != report:
        sys.stderr.write("measurements/mcp-size.txt disagrees with a fresh tools/list count\n")
        sys.stderr.write(report)
        sys.exit(1)
    if missing:
        sys.stderr.write("README counts drift from the tools/list count:\n")
        sys.stderr.write("\n".join(missing) + "\n")
        sys.exit(1)
else:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(report)
    print(report, end="")
    if missing:
        print("README is missing:", file=sys.stderr)
        print("\n".join(missing), file=sys.stderr)
PY
