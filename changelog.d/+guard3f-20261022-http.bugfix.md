gh, curl, wget, and httpie are now an allowlist when they talk to
GitHub. A resolved GET or HEAD passes. Any other method, a body, or a
flag whose meaning is not on the command is refused. ljos itself only
reads. A program that calls the API on its own is outside this filter.
