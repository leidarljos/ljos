`ljos doctor --fix` creates a missing ljos home and config file, moves a stale lock or pid file aside when its process is gone, starts packsetd with `packset ensure`, and records a runner session seat.
A row it can repair ends with `run ljos doctor --fix`.
The command prints one line for each repair and one line for each problem it left.
