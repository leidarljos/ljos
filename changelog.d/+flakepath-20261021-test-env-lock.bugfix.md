Sync and guard-shim tests that spawn git now hold the shared test environment lock, so parallel lib tests no longer race on PATH or other process-global environment values.
