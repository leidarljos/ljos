#!/usr/bin/env bash
# leiðarljós suite end-to-end: three seats with their own HOMEs on one scratch
# tracker, claim graph, deed store and pack workspace. No human steps.
# REAL=1 lets grok-4.7 (cursor-agent) cast the panel ballots as real seats.
set -uo pipefail
unset LJOS_SEAT VISSUE_AGENT LJOS_PANE_TOOL
for k in $(env | sed -n 's/^\([A-Za-z0-9_]*_\(SESSION\|THREAD\|CONVERSATION\)_ID\)=.*/\1/p'); do unset "$k"; done
BIN=${SUITE_BIN:-}; [ -n "$BIN" ] && export PATH="$BIN:$PATH"
root=$(mktemp -d "${SUITE_DIR:-${TMPDIR:-/tmp}}/suite-e2e.XXXX"); echo "root $root"
export VISSUE_ROOT="$root/tracker" DEEDAR_URL="file://$root/deeds" CLAIMDAG_DIR="$root/claims"
export PACKSET_WORKSPACE="suite:$(basename "$root")" LJOS_TRACKER_GIT=commit XDG_RUNTIME_DIR="$root/run"
export DEEDAR_HOST_SIGNING_KEY="$root/host.key"; head -c 32 /dev/urandom > "$DEEDAR_HOST_SIGNING_KEY"
mkdir -p "$VISSUE_ROOT" "$root/run" "$root/repo"
git -C "$VISSUE_ROOT" init -q; git -C "$VISSUE_ROOT" config user.email seat@example.invalid; git -C "$VISSUE_ROOT" config user.name seat
git -C "$VISSUE_ROOT" config core.hooksPath /dev/null
git -C "$root/repo" init -q; git -C "$root/repo" config user.email seat@example.invalid; git -C "$root/repo" config user.name seat
printf '# demo\n' > "$root/repo/README.md"; git -C "$root/repo" add README.md; git -C "$root/repo" -c core.hooksPath=/dev/null commit -qm init
SEATS="alice bob carol"
CA=${CA:-$(command -v cursor-agent || true)}
for s in $SEATS; do mkdir -p "$root/home-$s"; for rc in .bashrc .profile .zshenv; do printf 'export PATH="%s:/home/box/.cargo/bin:$PATH"\n' "${BIN:-/home/box/.cargo/bin}" > "$root/home-$s/$rc"; done; printf '[url "https://github.com/"]\n\tinsteadOf = git@github.com:\n' > "$root/home-$s/.gitconfig"; done
for s in $SEATS; do head -c 32 /dev/urandom > "$root/home-$s/seat.key"; done
as() { local s=$1; shift; HOME="$root/home-$s" DEEDAR_HOST_SIGNING_KEY="$root/home-$s/seat.key" LJOS_SEAT=$s VISSUE_AGENT=$s SEAT_SESSION_ID="sess-$s-0001" "$@"; }
cd "$root/repo"
packset ensure >/dev/null 2>&1 || true
P=0; F=0; TABLE=()
ok()  { P=$((P+1)); TABLE+=("pass	$1"); echo "PASS $1"; }
bad() { F=$((F+1)); TABLE+=("FAIL	$1	${2:-}"); echo "FAIL $1 :: ${2:-}"; }
chk() { local name=$1; shift; if "$@"; then ok "$name"; else bad "$name" "$LAST"; fi; }
LAST=""
run() { LAST=$("$@" 2>&1); local rc=$?; echo "\$ $*" >> "$root/trace.log"; echo "$LAST" >> "$root/trace.log"; return $rc; }
has() { printf '%s' "$LAST" | grep -q -- "$1"; }
state() { vissue show "$1" --json 2>/dev/null | jq -r .state; }

# 1. vissues filed by seats
run as alice vissue create -p demo "Add a CHANGES line for the demo" -q; T1=$(echo "$LAST" | tail -1)
chk "alice files a task vissue" test -n "$T1"
run as alice ljos file --top -p demo -t decision "Ship v1 now or hold?" --body "Options: ship, hold"; D1=$(echo "$LAST" | grep -oE '^[a-z]+-[a-z0-9]{4}' | head -1)
chk "alice files a decision vissue" test -n "$D1"

# 2. claim, pickup, ownership, review
run as bob ljos claim "$T1"; chk "bob claims $T1" has "gen="
GEN=$(echo "$LAST" | sed -n 's/.*gen=\([0-9]*\).*/\1/p' | tail -1)
run as carol ljos claim "$T1"; rc=$?
chk "claimdag refuses carol a node bob holds" test $rc -ne 0
run vissue claims --json; chk "tracker names bob as holder" has '"bob'
run as alice ljos claims; chk "ljos claims lists the hold" has "$T1"
echo "- CHANGES: demo line" > CHANGES.md; git add CHANGES.md; git -c core.hooksPath=/dev/null commit -qm "changes line"
run as bob deedar create file --name "changes line" --path CHANGES.md --agent bob; ACC=$(echo "$LAST" | grep -o 'deed-[a-z0-9-]*' | head -1)
chk "bob records a deed" test -n "$ACC"
run as bob ljos deed "$T1" --add "$ACC"; chk "bob cites the deed on $T1" test $? -eq 0
run as bob ljos evidence "$ACC"; chk "deedar: evidence intact" has " ok"
run as carol ljos file --parent "$T1" -t task "review: CHANGES line"; R1=$(echo "$LAST" | grep -oE '^[a-z]+-[a-z0-9]{4}' | head -1)
chk "carol files a review child" test -n "$R1"
run as carol vissue vote "$T1" --for pass --used "$ACC"; chk "carol's review ballot" test $? -eq 0

# 3. mail between seats
run as bob ljos send carol "review $T1 please, deed $ACC" --issue "$T1" --new-seat; chk "bob mails carol" test $? -eq 0
run as carol ljos inbox; chk "carol's inbox has bob's mail" has "review $T1"
MID=$(echo "$LAST" | grep -oE '[0-9a-f]{8,}' | head -1)
run as carol ljos reply "$MID" "passed, ballot cast"; chk "carol replies" test $? -eq 0
run as bob ljos inbox; chk "bob hears the reply" has "passed, ballot cast"
run as alice ljos inbox; chk "alice did not get bob's private mail" bash -c '! printf "%s" "$0" | grep -q "review"' "$LAST"

# 4. panel vote reaching consensus (real seats when REAL=1)
for s in $SEATS; do
  if [ "${REAL:-0}" = 1 ]; then
    as $s bash -c "cd '$root/repo' && timeout 240 '$CA' -p --model grok-4.7 --force --output-format text \"You are seat $s on a team. The tracker issue $D1 asks: ship v1 now or hold? The repo has one CHANGES line, reviewed and passed, with no open bugs. Decide, then run exactly one shell command: ljos vote $D1 --for ship --used none   (or --for hold). Then reply with one word.\"" >> "$root/real-$s.log" 2>&1
  else
    as $s ljos vote "$D1" --for ship --used none >/dev/null 2>&1
  fi
done
run vissue vote "$D1" --json; N=$(echo "$LAST" | jq length 2>/dev/null || echo 0)
chk "three seats cast on $D1" test "$N" -ge 3
chk "each ballot on $D1 is its own seat's" bash -c 'test "$(printf "%s" "$0" | jq -r "[.[].agent]|sort|join(\",\")")" = "alice,bob,carol"' "$LAST"
run vissue consensus "$D1" --gate --quorum 3; chk "D1 settles with quorum 3" test $? -eq 0
run ljos consensus "$D1"; chk "ljos consensus names a leader on D1" bash -c '! printf "%s" "$0" | grep -q "tie"' "$LAST"

run as alice ljos consensus "$D1"; chk "signed seat ballots stand at the push gate" has "decides ship for a push by alice"
chk "each seat's key is pinned on the tracker" test "$(wc -l < "$VISSUE_ROOT/.ljos-seat-keys")" -eq 3

# 4b. forged and unsigned ballots do not count
run as alice vissue create -p demo "Signed: enable the cache?" -q; D6=$(echo "$LAST"|tail -1)
as alice ljos vote "$D6" --for on --used none >/dev/null 2>&1
head -c 32 /dev/urandom > "$root/mallory.key"
run env HOME="$root/home-bob" LJOS_SEAT=bob VISSUE_AGENT=bob DEEDAR_HOST_SIGNING_KEY="$root/mallory.key" ljos vote "$D6" --for on --used none
chk "a ballot under bob's name with another key is refused a signature" has "pinned to key"
run as carol ljos consensus "$D6"; chk "the gate rejects the mismatched ballot" has "bob's ballot is unsigned\|bob's ballot is signed by a key other"
as bob ljos vote "$D6" --for on --used none >/dev/null 2>&1
run as carol ljos consensus "$D6"; chk "bob's own key makes it stand" has "decides on for a push by carol"
VISSUE_AGENT=carol vissue vote "$D6" --for on --used none >/dev/null 2>&1
run as alice ljos consensus "$D6"; chk "a ballot cast around ljos, unsigned, is rejected" has "carol's ballot is unsigned"
run as alice ljos consensus "$D6" --json; chk "push_cite stands false with the refusal in JSON" bash -c 'printf "%s" "$0" | jq -e ".push_cite.stands == false" >/dev/null' "$LAST"

# 5. tie must not settle
run as alice vissue create -p demo "Tie: rename the crate?" -q; D2=$(echo "$LAST"|tail -1)
as alice ljos vote "$D2" --for yes --used none >/dev/null 2>&1; as bob ljos vote "$D2" --for no --used none >/dev/null 2>&1
run vissue consensus "$D2" --gate; chk "a 1-1 tie fails the gate" test $? -ne 0
run ljos consensus "$D2"; chk "ljos consensus says tie, not settled" has "tie"
chk "ljos consensus does not call a tie settled" bash -c '! printf "%s" "$0" | grep -qiE "^push gate.*(pass|settled)|settled on"' "$LAST"
run ljos consensus "$D2" --json; chk "consensus json: settled false on a tie" bash -c 'printf "%s" "$0" | jq -e "(.settle.settled // false) == false and (.push_cite.settled // .push_cite.ok // false) != true" >/dev/null' "$LAST"

# 6. persona panel: a held minority view auto-files a dissent vissue
run as alice vissue create -p demo "Dissent: drop the legacy flag?" -q; D3=$(echo "$LAST"|tail -1)
as carol ljos persona keeper --anchor 0 --view "Never breaks a user's flag." --about compat >/dev/null 2>&1
as alice ljos vote "$D3" --for drop --used none >/dev/null 2>&1; as bob ljos vote "$D3" --for drop --used none >/dev/null 2>&1
as carol ljos vote "$D3" --for keep --used none --as keeper >/dev/null 2>&1
run as alice ljos consensus "$D3"; chk "ljos consensus reports the dissent it filed" has "dissent: filed"
chk "a 'Dissent holds keep' child exists" bash -c 'vissue children "$0" | grep -q "Dissent holds keep"' "$D3"
run as alice ljos consensus "$D3"; chk "a second reading files no duplicate" bash -c 'test "$(vissue children "$0" | grep -c "Dissent holds")" = 1' "$D3"
run as alice ljos consensus "$D2" --no-file; chk "a tie files no dissent child" bash -c 'test "$(vissue children "$0" | grep -c "Dissent")" = 0' "$D2"

# 7. quorum timeout
run as alice vissue create -p demo "Quorum: adopt the new linter?" -q; D4=$(echo "$LAST"|tail -1)
as alice ljos vote "$D4" --for adopt --used none >/dev/null 2>&1
run vissue consensus "$D4" --gate --quorum 3; chk "1 of 3 ballots fails the quorum" test $? -ne 0
run vissue consensus "$D4" --gate --quorum 3 --deadline 2000-01-01T00:00:00Z; rc=$?
chk "a passed deadline without quorum times out (exit 3, noted)" bash -c 'test "$0" -eq 3 && vissue show "$1" | grep -q "quorum timed out"' "$rc" "$D4"

run as alice ljos consensus "$D4" --quorum 3; rc=$?; chk "ljos consensus: short of quorum exits 1 and says so" bash -c 'test "$0" -eq 1 && printf "%s" "$1" | grep -q "quorum: short"' "$rc" "$LAST"
run as alice ljos consensus "$D4" --quorum 3 --deadline 2000-01-01T00:00:00Z; rc=$?; chk "ljos consensus: lapsed quorum exits 3" bash -c 'test "$0" -eq 3 && printf "%s" "$1" | grep -q "quorum: lapsed"' "$rc" "$LAST"
chk "the lapse is noted once" bash -c 'test "$(vissue show "$0" | grep -c "quorum timed out")" = 1' "$D4"
run as alice ljos consensus "$D1" --quorum 3; chk "ljos consensus: quorum met reads on" test $? -eq 0

# 7b. reap enforces a quorum deadline with no one asking
run as alice ljos file --top -p demo -t decision "Deadline: switch CI runner?" --body "Options: switch, stay" --quorum 3 --deadline 2000-01-01T00:00:00Z; D5=$(echo "$LAST" | grep -oE '^[a-z]+-[a-z0-9]{4}' | head -1)
chk "ljos file records the quorum and deadline" bash -c 'vissue show "$0" | grep -q "quorum: 3 by"' "$D5"
as bob ljos vote "$D5" --for switch --used none >/dev/null 2>&1
run as carol ljos reap; chk "a reap dry run names the due vote" has "would check $D5"
run as carol ljos reap --apply; chk "reap --apply lapses the short vote" has "lapsed $D5"
chk "the lapse is on the tracker once" bash -c 'test "$(vissue show "$0" | grep -c "quorum timed out")" = 1' "$D5"
run as carol ljos reap --apply; chk "a second reap does not lapse it again" bash -c '! printf "%s" "$0" | grep -q "lapsed $1"' "$LAST" "$D5"

# 8. finish
run as bob ljos finish "$T1" --gen "$GEN" --assignee bob --lesson "The CHANGES line shipped in one commit. Review passed first time." --close
chk "bob finishes $T1" has "completed"
chk "$T1 is DONE in the tracker" test "$(state "$T1")" = DONE
run vissue claims --json; chk "no claim left on $T1" bash -c '! printf "%s" "$0" | grep -q "$1"' "$LAST" "$T1"
run as alice ljos search "CHANGES line"; chk "the lesson is recallable by another seat" has "CHANGES"
run git -C "$VISSUE_ROOT" log --oneline; chk "tracker has commits for the work" test "$(echo "$LAST" | wc -l)" -ge 5
run vissue check; chk "tracker corpus validates" test $? -eq 0

echo; echo "== table ($P pass, $F fail)"; printf '%s\n' "${TABLE[@]}" | tee "$root/table.tsv"
packset forget "$PACKSET_WORKSPACE" >/dev/null 2>&1 || true
[ "$F" -eq 0 ]
