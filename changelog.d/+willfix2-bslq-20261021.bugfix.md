A terminal, a multiplexer or init that ran a hook's shell no longer owns the hook's sitting: tmux, screen, zellij, herdr, systemd, init, sshd, login and the terminal emulators the seat walk already refuses, `gnome-terminal-server` among them.
Each outlives every sitting under it, so a sitting whose own shell had exited was never finished as orphaned while that terminal stayed open.
The walk from the hook stops below them, and when only shells sit there, the outermost of them owns the sitting.
A known harness (claude, codex, cursor-agent, agy, opencode and the rest ljos knows by name) now owns the sitting over a nearer `uv`, `npx` or `node` that only started the hook and exited with it, which had closed the Write's claim on the next `ljos_start`.
A harness that runs as plain `node` still owns its sitting when no known harness sits above it.
The book keeps the owner's start time next to its pid, so a pid the kernel has handed to another process no longer keeps a finished runner's sitting open.
The walk reads pid 1 too, so a harness that is a container's pid 1 owns its sitting; any other pid 1 is the init and owns none.
The walk reads `/proc` and holds on Linux only. On macOS the hook's parent still owns the sitting, so there `ljos_start` after a Write still finishes the Write's claim and files a new one.
