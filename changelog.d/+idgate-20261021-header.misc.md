The cloud guard's toml header parse takes a missing bracket as an
early return, which is what Rust 1.97 clippy accepts, and a header
it could read still parses as before.
