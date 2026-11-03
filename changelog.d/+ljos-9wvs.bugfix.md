`ljos mesh down` sends SIGKILL after the grace only while the pid still names the tailscaled ljos started, or names no process at all.
On macOS, a process that writes to the pack runs `ps` once per tailscaled pid file to decide the `source.mesh` label, not once per write.
`ljos mesh up` says so when the tailscaled pid file has no start time, even when that tailscaled already answers.
`ljos mesh up` keeps a tailscaled whose start time cannot be read running and writes its pid alone.
Its line says that writes carry no `source.mesh` label and that `ljos mesh down` signals nothing.
