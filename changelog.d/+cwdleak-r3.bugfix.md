A relative, empty, or missing relative `TMPDIR` is rewritten to an absolute directory before the process parks, so the runtime directory is not created inside the parked directory.
`ljos hud` leaves that parked directory and removes it before exec, and the process it starts uses the same stable directory as the other background children.
