`ljos slot` takes `--key`, the run's public seat key that `ljos cloud up`
now prints on the `seat key:` line, and signs it into the slot. A run's
ballot counts only when the tracker pins that key for the run's seat, so
a writer that pins the seat first under its own key no longer votes as
the run. A slot written before this names no key and no longer counts.
