`ljos memory export --machine NAME` sets the machine in the file.
The default is `LJOS_SYNC_HOST` when that is set, and otherwise the hostname.
`ljos memory import --as-machine NAME` stores that machine for the import.
The file bytes stay as they are, so the import id stays the digest of the file.
