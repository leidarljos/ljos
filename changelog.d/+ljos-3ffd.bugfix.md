The rm, git and find guard shims read `policyd_config` only at the top of `ljos.toml` or under `[gates]`, as the hook and `ljos policy` do.
Under `[identity]` or any other table it no longer names a config for them.
