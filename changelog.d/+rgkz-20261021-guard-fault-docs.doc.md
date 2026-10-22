The integration docs now describe runner relay guard faults as fail
closed by default, including missing or unresponsive `ljos-policyd` and
`LJOS_GUARD_TIMEOUT` lapses.

The same docs now document `LJOS_GUARD_FAIL_OPEN=1` as the temporary
escape hatch that restores the previous allow-with-note behavior.
