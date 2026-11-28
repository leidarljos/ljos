`ljos hud` stays in its start directory unless that directory is the parked one this process created.
When the process cwd is that parked directory, a child keeps the tracker when that directory can be searched, otherwise `/`.
A relative or empty `TMPDIR` is not a temp root, so a home is not taken from those bytes and the command guard stays on.
