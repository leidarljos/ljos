#!/usr/bin/env bash
# A herd on scratch stores: four seats each open two sittings at the same
# moment, two seats contend for one issue, and every sitting is closed in
# parallel. Every store is under one temporary directory. Needs the seat
# binaries on PATH and a pack writer it may start.
set -euo pipefail
root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
export VISSUE_ROOT="$root/tracker" DEEDAR_URL="file://$root/deeds" CLAIMDAG_DIR="$root/claims"
export PACKSET_WORKSPACE="herd:$(basename "$root")" XDG_RUNTIME_DIR="$root/run"
mkdir -p "$VISSUE_ROOT" "$XDG_RUNTIME_DIR" && git -C "$VISSUE_ROOT" init -q
cd "$root"
packset ensure >/dev/null
fail() { echo "herd: $1" >&2; exit 1; }

seats=(brio acme vela orin)
declare -A issue
for s in "${seats[@]}"; do
  for k in 1 2; do
    issue[$s:$k]=$(vissue create -p herd "Ticket $k for $s" -q | tail -1)
  done
done

# Eight sittings at once, two per seat. Each must take its own node.
for s in "${seats[@]}"; do
  for k in 1 2; do
    ( LJOS_SEAT=$s ljos sitting "${issue[$s:$k]}" > "sit-$s-$k.out" 2>&1; echo $? > "sit-$s-$k.rc" ) &
  done
done
wait
for s in "${seats[@]}"; do
  for k in 1 2; do
    [ "$(cat "sit-$s-$k.rc")" = 0 ] || { cat "sit-$s-$k.out"; fail "sitting $s $k exited non-zero"; }
    grep -q 'gen=' "sit-$s-$k.out" || { cat "sit-$s-$k.out"; fail "sitting $s $k took no node"; }
    grep -q 'assignee busy\|held by another' "sit-$s-$k.out" && { cat "sit-$s-$k.out"; fail "sitting $s $k was refused"; }
  done
done
for s in "${seats[@]}"; do
  LJOS_SEAT=$s ljos seat | grep -q "^seat	$s$" || fail "seat $s does not name itself"
done
# Actors are hashed in the graph, so the count is what says eight seats
# hold eight nodes: one claimed node per sitting, none lost to a race.
claimed=$(claimdag list --json | grep -o '"status":"claimed"' | wc -l)
[ "$claimed" = 8 ] || { claimdag list --json | head -c 600; fail "$claimed claimed nodes, wanted 8"; }
echo "herd: eight sittings took eight nodes"

# Two seats on one issue at the same moment: one takes it, the other is told
# who holds it and does not unseat them.
shared=$(vissue create -p herd "One ticket two seats" -q | tail -1)
( LJOS_SEAT=brio ljos sitting "$shared" > share-brio.out 2>&1; echo $? > share-brio.rc ) &
( LJOS_SEAT=acme ljos sitting "$shared" > share-acme.out 2>&1; echo $? > share-acme.rc ) &
wait
took=0; told=0
for s in brio acme; do
  echo "--- $s exit $(cat share-$s.rc), $(wc -l < share-$s.out) lines"
  if [ "$(cat share-$s.rc)" = 0 ] && grep -q 'gen=' "share-$s.out" && ! grep -q 'held by' "share-$s.out"; then took=$((took+1)); fi
  if grep -q 'held by another' "share-$s.out"; then told=$((told+1)); fi
done
nodes=$(claimdag list --json --all | grep -o "\"summary\":\"$shared\"" | wc -l)
[ "$nodes" = 1 ] || { fail "$nodes claim-graph nodes for one ticket, wanted one"; }
[ "$took" = 1 ] || { for s in brio acme; do echo "=== $s"; cat "share-$s.out"; done; fail "$took seats took the shared ticket, wanted one"; }
[ "$told" = 1 ] || { for s in brio acme; do echo "=== $s"; cat "share-$s.out"; done; fail "the second seat was not told who holds it"; }
echo "herd: one ticket, two seats, one holder"

# Eight finishes at once, each with a lesson. Every ticket closes.
for s in "${seats[@]}"; do
  for k in 1 2; do
    gen=$(sed -n 's/.*gen=\([0-9][0-9]*\).*/\1/p' "sit-$s-$k.out" | tail -1)
    ( LJOS_SEAT=$s ljos finish "${issue[$s:$k]}" --gen "$gen" --lesson "Seat $s closed ticket $k. The herd held." > "fin-$s-$k.out" 2>&1; echo $? > "fin-$s-$k.rc" ) &
  done
done
wait
for s in "${seats[@]}"; do
  for k in 1 2; do
    [ "$(cat "fin-$s-$k.rc")" = 0 ] || { cat "fin-$s-$k.out"; fail "finish $s $k exited non-zero"; }
    vissue show "${issue[$s:$k]}" | grep -q 'State:    DONE' || fail "ticket $s $k did not close"
  done
done
echo "herd: eight finishes closed eight tickets"

# The same island fired by several closings tightens once: the first
# finish fires it, a second within the hour is held.
fired=$(grep -l 'fired the island' fin-*.out | wc -l)
held=$(grep -l 'fired within the hour' fin-*.out | wc -l)
echo "herd: $fired finishes fired an island, $held found it fired already"
echo "herd: every loop ran"
