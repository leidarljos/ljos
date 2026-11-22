The installer now treats a writer port like a writer address under a temporary home.
It keeps memory on when the selected writer port is nonblank, as `ljos` does.
The selected port is `PACKSET_PORT` when it is set, and `LJOS_MEM_PORT` otherwise.
