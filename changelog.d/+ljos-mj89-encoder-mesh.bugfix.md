`ljos cloud up` downloads the encoder archives into an unpredictable private directory and extracts the encoder and its runtime library into a new directory of their own.
It then makes `packset-embed` in the binary directory a symlink to that encoder in one rename, so a crash leaves the previous encoder and runtime or the new pair, never one of each.
It removes the previous encoder's directory only after that rename is synced, and when the sync fails it says the new encoder is in place and keeps the previous directory.
`ljos cloud up` publishes its identity file relative to a held directory descriptor.
`ljos mesh up`, offboard restoration, upgrades, and git hook rewrites use the same publication boundary for executable or secret files.
