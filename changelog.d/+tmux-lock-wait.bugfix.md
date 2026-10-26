A shell hook arms its eight second deadline before it takes tmux-sock.lock.
The take is a try-lock that retries for a short budget, and that budget stays inside the deadline.
When the lock is not free in time, the pane lookup is skipped and the call is judged as if no pane were found.
