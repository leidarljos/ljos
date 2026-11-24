Runner hook onboard owns a `command` entry only when the whole command is one simple command in a form onboard writes: `PROGRAM hook --runner RUNNER` with `ljos` as the program's file name, or that runner's guard script alone.
Onboard leaves any other command as it is, including one with a shell operator, a redirection, an expansion, a comment or an extra word.
It owns an Amp `to` path only when the path is absolute and names `ljos-guard-amp.sh`.
An older entry whose unquoted program path contains spaces is owned only when it matches the command onboard writes now.
Entries such as `--runner qwen-extra`, `printf /tmp/ljos-guard-auggie.sh`, or `ljos hook --runner qwen && printf foreign` stay as they were.
