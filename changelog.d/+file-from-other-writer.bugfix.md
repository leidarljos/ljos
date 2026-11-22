`ljos file --from` now takes a filed row as its own only when the one file that create writes changed and the new heading is exactly the row, so a refused later row no longer deletes a file another writer put under `issues/`, a logbook line another writer added to the new ticket, or a ledger record it could not read.
When anything else changed, the batch stops and leaves the tracker as it is.
