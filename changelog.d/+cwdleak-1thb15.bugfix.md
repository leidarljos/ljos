`TMPDIR`, `VISSUE_ROOT`, and `ISSUE_ROOT` keep the bytes they were given, including an empty value and a relative path.
Resolving those values stored `/tmp` or a path in the start directory, and a walk that did not answer exited 1.
A later HUD chosen after a mode 644 `ljos-hud` in an empty `PATH` entry is exec'd with `argv[0]` set to `ljos-hud`.
