# ljos record schema v1 (questions and ballots)

Schema v1 covers only question and ballot entries in the plain layer.
Every entry is one JSON file.

- `schema` is always `record/v1`.
- `kind` is `question` or `ballot`.
- `host`, `session`, and `agent` fields are refused anywhere in the plain
  payload.
- `opened_at` and `cast_at` are RFC 3339 timestamps at hour precision
  (minutes, seconds, and subseconds are zero).
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
  "id": "8d66a9c9385fd9d6",
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

Canonical JSON is recursive key sort with no insignificant whitespace.
Entry signatures are ed25519 over:

1. the ASCII label `ljos-record-entry/v1`
2. newline (`\n`)
3. canonical JSON of the unsigned payload (the entry without `signature`)

`ljos record check` verifies schema rules, signature validity, ballot-id
matching, and run-key pinning:

- when `voter` is `run/<run-id>`, the first key seen for `<run-id>` is pinned
- a later ballot from the same run id under another key is rejected

## Tally rule

Tally is pure over ballot entries:

- keep only the latest ballot per `(question, voter)` by `cast_at`
- if that latest ballot has stance `withdrawn`, it contributes no vote
