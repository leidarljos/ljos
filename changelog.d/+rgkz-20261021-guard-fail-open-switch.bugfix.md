Guard-fault handling now has an explicit escape hatch:
`LJOS_GUARD_FAIL_OPEN=1` restores the legacy allow-with-note behavior.

By default, guard faults fail closed in runner relays. The switch is
parsed with the same truthy forms as `POLICYD_REQUIRED`.
