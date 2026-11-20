# old_terminal fixture provenance

This directory feeds `crates/ljos-cli/tests/old_terminal.rs`. Nothing here was
exported from a running v0.17–v0.29 binary: those release tags are not in this
repository.

## `legacy_atoms.json`

### `terminal_by_version` (preference and lesson per row)

Each pair is a hand-written atom record shaped like the bodies built by
`post_terminal_pair` in the integration test. The expected stored shape and
the session literals (`harness` equals `session`, value `person`, `via` `ljos`,
no `turn`, no `@` in `session`) are asserted in
`git show 4e8daa4:crates/ljos-cli/tests/old_terminal.rs`
(`assert_terminal_shape` about lines 255–265 and the post calls about 294–311),
not in the v0.41.0 module comment.

`post_terminal_pair` and `post_held` add `schema`, `kind`, `level`, `origin`,
`source`, `workspace`, and `via`, so each JSON row is the atom fields only, not
a full `POST /v1/atoms` body.

The `preference_text` and `lesson_text` strings match the literals in the
fixture file, which replaced inline `VERSIONS` entries.

### `runner_lesson` and `agent_lesson`

Hand-written records for workspace `held`, matching the two `post_held` calls in
the same test: lesson text, `origin`, `harness`, and `session` fields as in
those calls.