A second `ljos memory import` of a file already stored prints `nothing new: N rows already present` and does not take the writer or call packset.
`ljos memory integrate --all` records a watermark, and a second run of an unchanged import does no work.
`--json` on `ljos memory import` and `ljos memory integrate` adds `imported`, `unchanged`, `skipped` and `reason`.
