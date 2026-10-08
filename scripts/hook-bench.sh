#!/usr/bin/env bash
# Prompt-hook latency on the seat this shell points at: each prompt three
# times, each under a fresh session (a repeated session id hits the hook's
# duplicate check and skips the search), with every answer kept so two
# builds can be compared byte for byte.
#
#   scripts/hook-bench.sh OUT_DIR [PROMPT_FILE]
#   cmp -r BEFORE_DIR AFTER_DIR   # minus times.txt: the answers
#
# PROMPT_FILE holds one prompt a line; absent, eight that mix prompts a
# young pack holds nothing standing on with two that name a toolchain rule
# (`ljos prefer "Run cargo test with rustc 1.88 or newer; rustc 1.83 lacks
# edition2024."` makes those two inject it).
set -u
out=${1:?usage: hook-bench.sh OUT_DIR [PROMPT_FILE]}
mkdir -p "$out"
: > "$out/times.txt"
if [ -n "${2:-}" ]; then
  mapfile -t prompts < "$2"
else
  prompts=(
    "what is the capital of france"
    "refactor the parser to use iterators"
    "write a haiku about autumn"
    "how many tests does the vissue crate have"
    "summarise yesterday's standup notes"
    "please rename the variable foo to bar"
    "why does cargo test fail with edition2024 on rustc 1.83"
    "should I run cargo test with an older rustc"
  )
fi
i=0
for p in "${prompts[@]}"; do
  i=$((i + 1))
  for rep in 1 2 3; do
    sid="bench-$(date +%s%N)-$i-$rep"
    json=$(python3 -c 'import json,sys; print(json.dumps({"hook_event_name":"UserPromptSubmit","session_id":sys.argv[1],"prompt":sys.argv[2]}))' "$sid" "$p")
    t0=$(date +%s%N)
    printf '%s' "$json" | ljos hook > "$out/p$i-r$rep.out" 2>/dev/null
    t1=$(date +%s%N)
    echo "$i $rep $(( (t1 - t0) / 1000000 ))" >> "$out/times.txt"
  done
done
python3 - "$out" <<'EOF'
import collections, statistics, sys
out = sys.argv[1]
by = collections.defaultdict(list)
for line in open(f"{out}/times.txt"):
    i, rep, ms = map(int, line.split())
    by[i].append(ms)
for i in sorted(by):
    print(f"prompt {i}: median {statistics.median(by[i])} ms  {by[i]}")
every = [m for v in by.values() for m in v]
print(f"all: median {statistics.median(every)} ms, mean {statistics.mean(every):.0f} ms")
EOF
