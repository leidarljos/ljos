`ljos hud` stays in its start directory unless that directory is the parked one this process created.
When it leaves that parked directory, a relative `TMPDIR` or tracker root is made absolute first, the HUD program path is made absolute first, and the directory it enters can be searched, or the process uses `/`.
A `TMPDIR` rewritten from a relative or empty value is not a new temp root, so a home inside it does not turn memory and the command guard off.
