Runner hook relay faults now fail closed. If the inner `ljos hook` cannot
start, times out, is signaled, or exits non-zero without a decision, the
relay returns a deny in each runner's native deny contract instead of
allowing the command.

The relay also treats an inner-hook "guard is unhealthy" note as a guard
fault and denies with a reason that names the fault and points to
`ljos doctor`.
