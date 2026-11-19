`ljos reap --idle` refuses a duration that does not fit in a timestamp.
A value such as `18446744073709551615h` saturated and the cutoff cast it to `-1`, so `reap --apply` released every claim.
`release --older-than` refuses the same overflow.
A lease that does not fit no longer counts a claim as idle.
