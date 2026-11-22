Scratch git from `tests/common/git.rs` clears its environment, then sets `GIT_CONFIG_NOSYSTEM` to `1` and `GIT_CONFIG_GLOBAL` to `/dev/null` unless that command opts in.
A caller's `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM` and `GIT_CONFIG_NOSYSTEM` are not copied onto that command.
