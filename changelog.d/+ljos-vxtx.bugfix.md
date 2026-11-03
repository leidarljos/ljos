`ljos file`, `ljos note` and `ljos sitting` run in a product checkout no longer write or commit tickets there.
An untracked `vissue.toml`, an empty `Software/`, or the boards a cloned project carries no longer make a checkout the tracker; the write goes to the configured tracker, or names the one it looked for.
`ljos cloud up` takes a workspace checkout for the tracker only when its `vissue.toml` is committed or its origin is `LJOS_TRACKER_REMOTE`.
The git hooks take a repository for the tracker by the same rule, and a tracker whose projects sit under `VISSUE_PREFIX` passes it.
