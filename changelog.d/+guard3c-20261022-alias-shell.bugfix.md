cloud guard now parses `!` git aliases through nested shell command
expansion so aliases that hide `git push` under `sh -c` cannot bypass the
`cursor/*` push refusal.
