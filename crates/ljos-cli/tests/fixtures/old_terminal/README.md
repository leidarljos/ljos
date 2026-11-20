# old_terminal fixture provenance

This directory feeds `crates/ljos-cli/tests/old_terminal.rs`. Nothing here was
exported from a running v0.17–v0.29 binary: those release tags are not in this
repository.

## `legacy_atoms.json`

### `terminal_by_version` (preference and lesson per row)

Each pair is a hand-written `POST /v1/atoms` body shaped like
`post_legacy_atom` in
`git show 4e8daa4:crates/ljos-cli/tests/old_terminal.rs` (lines 202–224), which
the integration test used when release archives were missing. The expected
stored shape is described in the module comment of
`git show v0.41.0:crates/ljos-cli/tests/old_terminal.rs` (lines 1–6): no
`source.seat`, `harness` equals `session`, value `person`, `via` is `ljos`, no
`turn`, no `@` in `session`.

The `preference_text` and `lesson_text` strings are the same literals the test
used for each `VERSIONS` entry in that file.

### `runner_lesson` and `agent_lesson`

Hand-written bodies matching the `post_legacy_atom` calls for workspace `held` in
`git show 4e8daa4:crates/ljos-cli/tests/old_terminal.rs` (runner `0.29.0`
branch and agent `0.32.1` branch): `RUNNER_LESSON` and `AGENT_LESSON`
constants, `origin`, `harness`, and `session` fields as in those calls.