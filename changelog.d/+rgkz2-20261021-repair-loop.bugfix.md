A guard fault now tells the agent to ask the person to run
`ljos doctor` in their own terminal, and names
`LJOS_GUARD_FAIL_OPEN=1` as the switch that allows other lines.

`ljos doctor`, `ljos doctor --fix` and `ljos onboard` still run
while the guard is faulted, so the seat can be repaired. A line
that also runs another command stays denied.
