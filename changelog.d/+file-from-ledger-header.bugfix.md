A new ledger file counts as written by `ljos file --from` only when it is exactly the header vissue writes for that row.
Lines another writer added above `#+VISSUE_LEDGER_LOG:` now stop the batch; before, a later refused row deleted them.
