A terminal, a multiplexer or init that ran a hook's shell no longer owns the hook's sitting: tmux, screen, zellij, herdr, systemd, init, sshd, login and the terminal emulators the seat walk already refuses, `gnome-terminal-server` among them.
Each outlives every sitting under it, so a sitting whose own shell had exited was never finished as orphaned while that terminal stayed open.
The walk from the hook stops below them, and when only shells sit there, the outermost of them owns the sitting.
