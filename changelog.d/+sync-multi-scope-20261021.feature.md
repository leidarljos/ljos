`ljos sync` now supports tracker repositories that name more than one scope in
`.ljos/sync.toml` with a `[scopes]` table, each with its own age recipients.
The primary scope keeps the existing `name` and `recipients` keys, and each
extra scope exports to its own host log file under `.ljos/atoms/`.

Exports and imports now preserve scope boundaries per host log so a cloud scope
can be shared without exposing atoms from box-only scopes. Existing
single-scope tracker configs keep their current behavior unchanged.
