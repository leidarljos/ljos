`ljos doctor` now gives a concrete repair command for an unresponsive
`ljos-policyd` in the `policy` row, not only for a missing binary.

The policy health check still fails both cases, and tests now cover
missing and unresponsive binaries with the expected fix guidance.
