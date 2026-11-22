`ljos file --from` now takes a row's write as its own only when it matches what create writes.
Its `CREATED` date must be the day the create ran.
Its `DISCOVERED_FROM` must be the first known `[[id:...]]` in the body, read as vissue reads it, and must be absent when the body links no known id.
A link written `[[id: ID]]` counts, and a forged date or origin stops the batch.
