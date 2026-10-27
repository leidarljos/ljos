The release runner test builds ljos and ljos-mcp when those release binaries are missing.
cargo test does not build that profile, so the check stopped before it could read the binary.
