`ljos claim` commits and pushes the issue the way a note does, so the claim reaches the other hosts before the next note.
A `STARTED` claim stays taken when this machine's hold for that issue names a program that is still running, and goes back to `TODO` once that program is gone and the lease has passed.
The hold names the nearest program above the command, so a short-lived runner is not kept alive by the daemon above it.
