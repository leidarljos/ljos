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
- voter ids are `<identity>/<seat-or-run>`.

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

- question id shape: `<type>-<8 hex>`
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

Ballot id is:

- canonical JSON over the ballot payload without the `id` field
- sha256 digest
- first 16 hex characters

## Canonical JSON and signatures

Canonical JSON is the typed entry re-serialised: keys sorted, no
insignificant whitespace. Record check also canonicalises the unsigned
payload from the file (without `signature`) and requires it to match the
typed form, so an extra field is unsigned and refused.
Key order and whitespace may differ.

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

`ljos record check` verifies schema rules, signature validity, ballot-id
matching, and key pinning:

- every voter id is pinned to one key, including a question's `opened_by`
  and a seat
- a run is `<identity>/run-<id>`; `run/<id>` has no identity and is refused
- the tracker pin file wins when it already names that voter
- with no stored row for that voter, the first key seen in this check is the
  pin for this check only
- ingesting pins durably is what makes that pin persist across checks
- a later entry from the same voter under another key is rejected

## Tally rule

Tally is pure over ballot entries:

- a ballot named by another entry's `replaces` never counts
- of the rest, the latest `cast_at` per `(question, voter)` counts
- a latest stance of `withdrawn` contributes no vote
- two unreplaced ballots from one voter on one question in the same
  hour are an error. `cast_at` does not break that tie, and neither
  does file order
