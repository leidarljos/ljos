The tool-call record is no longer read from or written to a path under `/dev/null`, so a hook with `XDG_RUNTIME_DIR` set, no home and an open temporary directory no longer fails with an unreadable record that names that path.
With no directory that can hold the record, the read fails closed and says why.
