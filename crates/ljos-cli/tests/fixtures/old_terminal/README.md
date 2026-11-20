# old_terminal fixture provenance

This directory feeds `crates/ljos-cli/tests/old_terminal.rs`. Nothing here was
exported from a running v0.17–v0.29 binary: those release tags are not in this
repository.

## `legacy_atoms.json`

### `terminal_by_version`

Each row is one workspace (`v0170`, `v0220`, `v0250`, `v0290`) with the
preference and lesson texts that `4e8daa4` posted when release archives were
missing. In `git show 4e8daa4:crates/ljos-cli/tests/old_terminal.rs` the
`VERSIONS` array and the loop over it build:

- `Keep the {version} notes in one paper file.`
- `The {version} staging host reboots on its own night.`

The JSON `preference_text` and `lesson_text` fields are those four pairs with
each `{version}` filled in.

Each stored atom must match `assert_terminal_shape` in the integration test:
`user-declared`, no `source.basis`, `source.seat` absent or empty, no `turn`,
`harness` equals `session`, `via` `ljos`, no `@` in `session`. The module
comment at the top of `old_terminal.rs` describes the same v0.17–v0.29 shape.

When archives are missing, `4e8daa4` posts through `post_legacy_atom` with
`origin` `user-declared`, `harness` and `session` `person`. The current test
builds the same objects in `post_terminal_pair`, which adds `schema`, `kind`,
`level`, `origin`, `source`, `workspace`, and `via`. Each JSON row lists only
the variable fields, not a full `POST /v1/atoms` body.

### `runner_lesson` and `agent_lesson`

These match the held-lesson fallbacks in `4e8daa4` when binaries are missing:
`RUNNER_LESSON` (`The lint job must pass before a merge.`, `user-declared`,
`claude`, `claude@runnerlesson01`) and `AGENT_LESSON` (`The deploy window
closes at four on Friday.`, `agent-derived`, `claude`, `claude@agentlesson01`).
`post_held` in the integration test supplies the same envelope fields as
`post_terminal_pair`.