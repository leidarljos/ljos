`ljos mesh down` sends SIGKILL after the grace only while the pid still names the tailscaled ljos started, or names no process at all.
On macOS, a process that writes to the pack runs `ps` once per tailscaled pid file to decide the `source.mesh` label, not once per write.
