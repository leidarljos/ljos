The rm, git and find shims keep running when the passwd home lookup fails.
They use `$HOME` when that entry is missing, and with neither home they print one warning and do not refuse the command.
