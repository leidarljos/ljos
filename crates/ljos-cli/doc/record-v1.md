# ljos record schema v1 (questions and ballots)

Schema v1 covers only question and ballot entries in the plain layer.
Every entry is one JSON file.

- `schema` is always `record/v1`.
- `kind` is `question` or `ballot`.
- `host`, `session`, and `agent` fields are refused anywhere in the plain
  payload.
- `opened_at` and `cast_at` are UTC hours written
  `YYYY-MM-DDTHH:00:00Z`. Offsets, subseconds and other separators are
  refused, so one hour has one spelling and one ballot id.
- voter ids are `<identity>/<seat-or-run>`, each written one way (see
  [Voter ids](#voter-ids)): `alpha/seat-a`, not `Alpha/seat-a` or
  `alpha/Seat-a`.

## Voter ids

A voter id is `<identity>/<seat-or-run>`, spelled with ASCII letters,
digits, `-`, `_` and `.`. It has one written form:

- the identity is its slug: lower case, with each run of other
  characters one `-` and none at either end. `alpha`, not `Alpha`,
  `alpha.` or `alpha_`.
- the part after `/` is the seat it sits as, read the way the tracker
  reads a holder: its slug, less an old conversation tag (a pid in base
  36: a last part of one to six lower-case letters and digits with at
  least one of each, as in `inky-39u` or `inky-n9o`), with an old seat
  name mapped to the new one (`ljos-bot` is `ljosbot`). `alpha/inky`,
  not `alpha/Inky` or `alpha/inky-39u`. A last part of digits alone is
  a version or a number and stays: `alpha/gpt-5` is its own seat, apart
  from `alpha/gpt`.
- a run is `run-<id>` and a judge `judge-<model>`. Both are slugs and
  keep their ids whole, so `judge-model-1-5` keeps its version.
- `run/<id>` has no identity and is refused; a run is
  `<identity>/run-<id>`.

Signing and `ljos record check` refuse every other spelling and name the
form to use: `alpha/inky-39u` must be written `alpha/inky`. The signature
covers the voter, so a stored entry in another spelling has to be signed
again under that form.

## Question entry

```json
{
  "schema": "record/v1",
  "kind": "question",
  "id": "correct-3f9a1c2e",
  "type": "correct",
  "title": "is this head mergeable",
  "options": ["accept", "reject"],
  "anchors": ["sha:owner/repo@0123456789abcdef0123456789abcdef01234567"],
  "opened_by": "alpha/seat-a",
  "opened_at": "2026-10-21T09:00:00Z",
  "signature": {
    "key": "<64 hex>",
    "sig": "<128 hex>"
  }
}
```

- question id shape: `<type>-<8 lowercase hex>`. `correct-3F9A1C2E` is
  refused, so a question has one spelling and one tally key. Ballot
  `question` and `dup_of` follow the same rule.
- supported types: `correct`, `locate`, `change`, `design`, `lead`
- supported anchor prefixes: `sha:`, `sym:`, `ticket:`, `pr:`, `name:`

## Ballot entry

```json
{
  "schema": "record/v1",
  "kind": "ballot",
  "id": "bb6a93382fadce00",
  "question": "correct-3f9a1c2e",
  "stance": "accept",
  "confidence": 0.9,
  "voter": "alpha/seat-a",
  "commit": "0123456789abcdef0123456789abcdef01234567",
  "used": ["none"],
  "lineage": "model:synthetic",
  "cast_at": "2026-10-21T10:00:00Z",
  "signature": {
    "key": "<64 hex>",
    "sig": "<128 hex>"
  }
}
```

- `stance` is exactly `accept`, `reject` or `withdrawn`: lower case, with
  no spaces or invisible characters.

Ballot id is:

- canonical JSON over the ballot payload without the `id` field
- sha256 digest
- first 16 hex characters

## Canonical JSON and signatures

Canonical JSON is the typed entry re-serialised: keys sorted, no
insignificant whitespace. Record check also canonicalises the unsigned
payload from the file (without `signature`) and requires it to match the
typed form, so an extra field is unsigned and refused.
Key order and whitespace may differ. A duplicate key at any depth, such
as two `stance` fields or two `sig` fields under `signature`, is refused
rather than read last-wins.

Numbers use the typed spelling. `1e0`, `10E-1` and `1.0` are one value
and one ballot id, and a file may use the exponent spelling. An integer
`1` is not the canonical spelling of `1.0`, and the file is refused.

Strings are raw Unicode scalar values. `\u00e9` and `é` are the same
string. The record does not normalise: `é` and `e` plus a combining
acute are different strings and different ballot ids.

Entry signatures are ed25519 over:

1. the ASCII label `ljos-record-entry/v1`
2. newline (`\n`)
3. canonical JSON of the unsigned payload (the entry without `signature`)

Verification is strict. A small-order key or `R` is refused: the identity
point as a key, with `R` the identity and `s = 0`, would otherwise verify
any message.

A voter signs with its own key file,
`$XDG_DATA_HOME/ljos/seats/<name>/ballot.key`. `<name>` keeps ASCII
letters, digits, `-` and `.` and writes any other byte, and a leading
`.`, as `%HH`, so `a/b_c` and `a_b/c` never share a key. A voter id
in its one written form is lower case, so two never share a file on a
case-insensitive volume either.

## What `ljos record check` enforces

`ljos record check DIR` reads every `.json` file under `DIR` and fails on
the first problem. It checks the schema rules above, the ballot id, the
signature, key pinning, the `replaces` rule and the tally rule.

### Key pinning

The tracker pin file is `.ljos-seat-keys` at `$VISSUE_ROOT`, one
`<voter> <key>` row per line.

- with no pin file there are no stored pins. A pin file that cannot be
  read, including a symlink to a missing file, fails the check.
- two rows for one voter fail the check
- a row's voter is read as written and nothing is stripped. It must be
  printable ASCII, a row with `/` must be a voter id in its one written
  form, and a bare row spelled only with voter-id characters must
  already be a seat slug (`seat-a`, not `Seat-a` or `seat_a`). A BOM or
  zero-width space before a voter fails the check instead of leaving
  that pin unread.
- every voter id is pinned to one key, including a question's `opened_by`
- a pin file row for a voter id also pins the seat after its `/`:
  beside `inky <key1>`, a row `mallory/inky <key9>` fails the check
  rather than letting a second key vote as `inky`
- seat cross-check: a voter id with any other spelling of its seat is
  refused before this check (see [Voter ids](#voter-ids)), so the part
  after `/` is looked up as written. A voter id row pins the seat after
  its `/`, and each bare row pins the seat it reads as under the same
  rules: `inky` and `inky@39u` both pin `inky`, while `inky-10` pins
  `inky-10`; `ljos-bot` pins `ljosbot`; `judge:model-1.5` pins
  `judge-model-1-5`; and `run-101` pins `run-101`. A run or a judge
  keeps its id whole, so `run-inky` and `judge-inky` pin themselves, not
  `inky`. Rows that pin one seat must pin one key, whatever their
  spelling, up to hex case and a small-order part: `inky <key1>` beside
  `inky@39u <key2>` or `alpha/inky <key2>` fails the check, since one
  seat would vote under two keys. When rows pin the voter's seat, the
  entry must be signed under that key.
- with no pin file row for the voter id and none for its seat, the first
  key seen in this check is the pin for this check only
- ingesting pins durably is what makes that pin persist across checks
- a later entry from the same voter under another key is rejected

## Tally rule

Tally is pure over signed ballot entries and counts per question. A
voter is a signing key fingerprint: two key strings are one voter when
they differ only in hex case or in a small-order part.

- files that hold one ballot (one ballot id) under one voter are one
  ballot, whatever the hex case of their key and signature: a replayed
  copy neither counts twice nor makes the voter ambiguous
- `replaces` must name the id of a ballot in the same check, from the
  same voter (key fingerprint), on the same question. A missing target,
  or another voter's or another question's ballot, fails the check.
- a ballot named by another entry's `replaces` never counts
- with one seat under two keys on one question, the first key seen for
  that seat keeps counting and the later key for that seat is refused
- of what remains, the latest `cast_at` per `(question, voter)` counts
- a latest stance of `withdrawn` contributes no vote
- two unreplaced ballots from one voter on one question in that
  voter's latest hour are ambiguous and are dropped on that question
  only. `cast_at` does not break that tie, and neither does file
  order. A tie in an earlier hour is superseded by the later ballot
  like any older ballot: accept and reject at 10:00 and accept at
  12:00 count one accept

## What the check cannot see

The check reads one directory as it is now and keeps nothing from earlier
runs. It cannot tell that a file was deleted. A deleted withdrawal revives
the earlier ballot: with the withdrawal file gone, the ballot it replaced
is again the latest under its key and counts. Keep the record under version
control and review deletions there.
