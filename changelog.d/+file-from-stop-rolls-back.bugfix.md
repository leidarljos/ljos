When a row stops the batch, `ljos file --from` now takes back the tickets written before it.
Before, it did so only for a row the tracker refused.
When a row's id scan fails, the tracker stays as it was.
When a row's write cannot be checked, or another writer changed it meanwhile, the other projects stay as they were.
The error names the tickets that stay.
