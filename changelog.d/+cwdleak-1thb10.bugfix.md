A slash-free `LJOS_HUD_BIN` is missing when that trimmed name is not a regular file in the start directory, and `PATH` is not searched.
When the trimmed name is a regular file, that same string is handed to `execvp`. An unset `PATH` searches `/bin` and then `/usr/bin`.
