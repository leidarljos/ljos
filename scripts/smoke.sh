#!/usr/bin/env bash
# The tutorial's four loops on scratch stores, as a check: memory,
# agreement, work and handover, then a persona panel's settle. Every store
# is under one temporary directory; nothing touches the seat that runs it.
# Needs the seat binaries on PATH and a pack writer it may start.
set -euo pipefail
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

ljos doctor | grep -q '^ok	pack' || fail "the pack does not answer"
ljos remember "The lexical default is BM25+. It beat BM25 by two points on turns." | grep -q '"kind": "lesson"' || fail remember
ljos prefer "CombMNZ over RRF for fusing two ballots." | grep -q '"kind": "preference"' || fail prefer
ljos search fuse | grep -q CombMNZ || fail search
before=$(ljos search fuse --as-of 2000-01-01) || fail "an as-of search was refused"
echo "$before" | grep -q CombMNZ && fail "an as-of read before the write found it"
ljos due | grep -q 'scheduled' || fail due
# A rewrite of the same claim closes the earlier one on arrival; the seat
# says so, and a consolidation finds nothing left to close.
ljos remember "The lexical default is BM25L. It beat BM25 by two points on turns." 2>&1 | grep -q 'revises 1 earlier' || fail "a rewrite did not close the earlier claim"
ljos consolidate | grep -q '^0 of ' || fail "consolidate found pairs a write should have closed"

id=$(vissue create -p demo "Ship the fuse change?" -q | tail -1)
VISSUE_AGENT=alice ljos vote "$id" --for ship >/dev/null
VISSUE_AGENT=bob ljos vote "$id" --for ship >/dev/null
VISSUE_AGENT=carol ljos vote "$id" --for hold >/dev/null
ljos consensus "$id" | grep -q '"engine": "degroot-fj"' || fail consensus
ljos learn "$id" --outcome hold | grep -q 'weighs' || fail learn

ljos sitting "$id" --assignee you | grep -q '^gen=' || fail sitting
echo 'fn main() {}' > patch.rs
acc=$(deedar create file --name "the fuse patch" --path patch.rs --agent you | grep -o 'deed-[a-z0-9-]*' | head -1)
ljos deed "$id" --add "$acc" >/dev/null
ljos finish "$id" --lesson "The fuse patch shipped as one file. Nothing else moved." --outcome hold | grep -q 'completed the session node' || fail finish
ljos sitting "$id" --assignee you | grep -q 'reopened' || fail "reopen on a second sitting"
ljos sitting "$id" --assignee you | grep -q 'the sitting resumes' || fail "a third sitting on a held node did not resume"
ljos timeline "$id" | grep -q 'tracker	created' || fail timeline
LJOS_SEAT=you ljos release "$id" | grep -q '^gen=' || fail "release under LJOS_SEAT"

ljos persona reviewer --anchor 0.2 --view "Reads for what breaks in production." --about docs >/dev/null
ljos persona reader --anchor 0.8 --view "Reads as a first-time user." --about docs >/dev/null
id2=$(vissue create -p demo "Publish the docs site now?" -q | tail -1)
ljos vote "$id2" --for hold --as reviewer >/dev/null
ljos vote "$id2" --for ship --as reader >/dev/null
ljos predict "$id2" --expect ship --as reviewer >/dev/null
ljos predict "$id2" --expect '{"ship":0.6,"hold":0.4}' --as reader >/dev/null
ljos trust reader reviewer 0.9 --about docs >/dev/null
ljos consensus "$id2" | grep -q '"predictors": 2' || fail "surprisingly popular"
ljos brief reviewer "$id2" | grep -q 'You are reviewer' || fail brief
ljos calibrate -p demo | grep -q weighs || fail calibrate

ljos rule '*--force*' --verdict deny --why "Never force push." >/dev/null
echo '{"hook_event_name":"PreToolUse","tool_input":{"command":"git push --force"}}' | ljos hook | grep -q '"permissionDecision":"deny"' || fail "rule through the hook"
ljos policy -- git push --force | grep -q '^deny:' || fail "rule through policy"

ljos handover --out "$root/bag" --issue "$id" | grep -q 'manifest-sha256.txt.sig' || fail "signed handover"
ljos receive "$root/bag" | grep -q 'atoms enclosed' || fail receive
echo "smoke: every loop ran"
