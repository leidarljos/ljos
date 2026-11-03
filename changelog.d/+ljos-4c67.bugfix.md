Tracker git never prompts: every clone, fetch and push runs with `GIT_TERMINAL_PROMPT=0` and no askpass program.
`ljos sitting` and `ljos note` against a remote that wants a credential commit to the local tracker and exit 0.
They print `the remote was not reached (...), so the next sync pushes it`.
`ljos sync` by hand pushes those commits once the remote answers, also in a checkout that names no scope.
