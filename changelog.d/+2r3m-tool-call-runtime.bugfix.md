`ljos` now checks tool-call records in both the active `XDG_RUNTIME_DIR` path and the default runtime path, so overriding `XDG_RUNTIME_DIR` does not hide an existing record.
When a tool-call record cannot be found after a recorded write fault, `ljos` now names each `tool-calls` path it checked.
