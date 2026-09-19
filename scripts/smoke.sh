#!/usr/bin/env bash
# The tutorial's four loops on scratch stores, as a check: memory,
# agreement, work and handover, then a persona panel's settle. Every store
# is under one temporary directory; nothing touches the seat that runs it.
# Needs the seat binaries on PATH and a pack writer it may start.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
export VISSUE_ROOT="$root/tracker" DEEDAR_URL="file://$root/deeds" CLAIMDAG_DIR="$root/claims"
export PACKSET_WORKSPACE="smoke:$(basename "$root")" VISSUE_AGENT=you
export DEEDAR_HOST_SIGNING_KEY="$root/host.key"
head -c 32 /dev/urandom > "$DEEDAR_HOST_SIGNING_KEY"
mkdir -p "$VISSUE_ROOT" && git -C "$VISSUE_ROOT" init -q
cd "$root"
packset ensure >/dev/null
fail() { echo "smoke: $1" >&2; exit 1; }

doc=$(ljos doctor 2>&1 || true)
echo "$doc" | grep -q '^ok	pack' || { echo "$doc"; fail "the pack does not answer"; }
ljos remember "The lexical default is BM25+. It beat BM25 by two points on turns." | grep -q lesson || fail remember
ljos prefer "CombMNZ over RRF for fusing two ballots." | grep -q preference || fail prefer
ljos search fuse | grep -q CombMNZ || fail search
before=$(ljos search fuse --as-of 2000-01-01) || fail "an as-of search was refused"
echo "$before" | grep -q CombMNZ && fail "an as-of read before the write found it"
ljos due | grep -q 'scheduled' || fail due
ljos habit smoke-score 0.5 --unit acc --every 7d --source first | grep -q '	habit	' || fail "a first reading"
ljos habit smoke-score 0.6 --unit acc --every 7d --source second 2>&1 | grep -q 'was 0.5 acc' || fail "a second reading did not name the first"
ljos habit | grep -q '^smoke-score	0.6 acc	+0.1 since 0.5' || fail "the habit list"
ljos habit smoke-score | grep -c '^smoke-score' | grep -q '^1$' || fail "one live reading a habit"
# A rewrite of the same claim closes the earlier one on arrival; the seat
# says so, and a consolidation finds nothing left to close.
ljos remember "The lexical default is BM25L. It beat BM25 by two points on turns." 2>&1 | grep -q 'revises 1 earlier' || fail "a rewrite did not close the earlier claim"
ljos consolidate | grep -q '^0 of ' || fail "consolidate found pairs a write should have closed"
if command -v landscape >/dev/null; then
  ljos conflicts -n 2 | grep -qE 'passes between single memories|fewer than two memories' || fail conflicts
else
  out=$(ljos conflicts 2>&1 || true)
  echo "$out" | grep -q 'not on PATH' || fail "conflicts without the habitat should say so"
fi

id=$(vissue create -p demo "Ship the fuse change?" -q | tail -1)
VISSUE_AGENT=alice ljos vote "$id" --for ship >/dev/null
VISSUE_AGENT=bob ljos vote "$id" --for ship >/dev/null
VISSUE_AGENT=carol ljos vote "$id" --for hold >/dev/null
ljos consensus "$id" | grep -q '"engine": "degroot-fj"' || fail consensus
learned=$(ljos learn "$id" --outcome hold 2>&1 || true)
echo "$learned" | grep -q 'weighs' || { echo "$learned"; ljos seat; vissue vote "$id" 2>&1 | head -8; fail learn; }

sit=$(ljos sitting "$id" --assignee you)
echo "$sit" | grep -q 'gen=' || fail sitting
gen=$(printf '%s\n' "$sit" | sed -n 's/.*gen=\([0-9][0-9]*\).*/\1/p' | tail -1)
[ -n "$gen" ] || fail "sitting printed no gen"
echo 'fn main() {}' > patch.rs
acc=$(deedar create file --name "the fuse patch" --path patch.rs --agent you | grep -o 'deed-[a-z0-9-]*' | head -1)
ljos deed "$id" --add "$acc" >/dev/null
ljos finish "$id" --gen "$gen" --assignee you --lesson "The fuse patch shipped as one file. Nothing else moved." --outcome hold | grep -q 'completed the session node' || fail finish
vissue show "$id" | grep -q 'State:    DONE' || fail "finish with status done did not close the ticket"
ljos sitting "$id" --assignee you | grep -q 'reopened' || fail "reopen on a second sitting"
ljos sitting "$id" --assignee you | grep -q 'the sitting resumes' || fail "a third sitting on a held node did not resume"
ljos timeline "$id" | grep -q 'tracker	created' || fail timeline
LJOS_SEAT=you ljos release "$id" | grep -q '^gen=' || fail "release under LJOS_SEAT"
ljos seat | grep -q '^seat	' || fail "seat prints who is sitting"
LJOS_SEAT=alice ljos seat | grep -q '^seat	alice$' || fail "LJOS_SEAT names the seat"

# A harness seat occupies per issue: two sittings must both take, not
# busy-and-release the first. Named `you` is a shared name.
id_a=$(vissue create -p demo "First harness ticket" -q | tail -1)
id_b=$(vissue create -p demo "Second harness ticket" -q | tail -1)
sit_a=$(ljos sitting "$id_a" --assignee you)
echo "$sit_a" | grep -q 'gen=' || fail "first harness sitting"
sit_b=$(ljos sitting "$id_b" --assignee you)
echo "$sit_b" | grep -q 'assignee busy' && fail "second harness sitting unseated the first"
echo "$sit_b" | grep -q 'gen=' || fail "second harness sitting"

ljos persona reviewer --anchor 0.2 --view "Reads for what breaks in production." --about docs >/dev/null
ljos persona reader --anchor 0.8 --view "Reads as a first-time user." --about docs >/dev/null
ljos personas | grep -q '^reviewer .*anchor 0.20 .*about docs' || fail "personas roster"
id2=$(vissue create -p demo "Publish the docs site now?" -q | tail -1)
ljos vote "$id2" --for hold --as reviewer >/dev/null
ljos vote "$id2" --for ship --as reader >/dev/null
ljos predict "$id2" --expect ship --as reviewer >/dev/null
ljos predict "$id2" --expect '{"ship":0.6,"hold":0.4}' --as reader >/dev/null
ljos trust reader reviewer 0.9 --about docs >/dev/null
ljos consensus "$id2" | grep -q '"predictors": 2' || fail "surprisingly popular"
ljos brief reviewer "$id2" | grep -q 'You are reviewer' || fail brief
cal=$(ljos calibrate -p demo 2>&1 || true)
echo "$cal" | grep -q weighs || { echo "$cal"; fail calibrate; }

ljos rule '*--force*' --verdict deny --why "Never force push." >/dev/null
echo '{"hook_event_name":"PreToolUse","tool_input":{"command":"git push --force"}}' | ljos hook | grep -q '"permissionDecision":"deny"' || fail "rule through the hook"
# A tool call must not search the pack. The inject script exits empty.
out=$(printf '%s' '{"hook_event_name":"PreToolUse","toolName":"read_file"}' | "$here/grok/ljos-inject.sh" || true)
[ -z "$out" ] || fail "inject searched on PreToolUse"
ljos policy -- git push --force | grep -q '^deny:' || fail "rule through policy"

ljos handover --out "$root/bag" --issue "$id" | grep -q 'manifest-sha256.txt.sig' || fail "signed handover"
got=$(ljos receive "$root/bag")
echo "$got" | grep -q 'atoms enclosed' || fail receive
echo "$got" | grep -q 'signed by ' || fail "receive did not name the accepted key"
# Unsigned: drop the sig, import must refuse before POST.
cp -a "$root/bag" "$root/unsigned"
rtrash "$root/unsigned/manifest-sha256.txt.sig" 2>/dev/null || rm -f "$root/unsigned/manifest-sha256.txt.sig"
if ljos receive "$root/unsigned" --import >/dev/null 2>&1; then fail "an unsigned bag was imported"; fi
# Signed import lands atoms; doctor still answers.
got=$(ljos receive "$root/bag" --import 2>&1 || true)
echo "$got" | grep -q 'atoms imported' || { echo "$got"; fail "signed import"; }
ljos doctor | grep -q '^ok	pack' || fail "doctor after receive"
# A bag altered after sealing is refused: one byte of one atom changed,
# and the receipt must fail on the manifest, not count the atoms.
tampered=$(ls "$root/bag/data/atoms"/* | head -1)
printf 'x' | dd of="$tampered" bs=1 seek=3 conv=notrunc status=none
if ljos receive "$root/bag" >/dev/null 2>&1; then fail "a tampered bag was received"; fi
echo "smoke: every loop ran"
