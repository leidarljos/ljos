`ljos doctor` used to wait forever on a habitat that never
exited, and a policyd whose `version` answered while `check`
hung looked healthy. A capture now stops at three minutes. The
policy row runs `check` as well, and a hang there is unresponsive.

A `POLICYD_BIN` that names a missing file says so, instead of
suggesting a cargo install. `LJOS_GUARD_FAIL_OPEN=1` is its own
row, and an allow it chooses is written to the bypass log.
