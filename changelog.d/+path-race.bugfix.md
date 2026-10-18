A test that needs a fake `vissue` or `deedar` prepends that directory on the calling thread, so the process `PATH` still names `cat`.
A test that changes `HOME`, `XDG_*`, or another variable records it on the calling thread and passes that environment to the child, so the process environment stays the one the suite started with.
