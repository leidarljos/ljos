On macOS the git shim read ljos-policyd's refusal as having no rule, said the guard was unhealthy, and ran the command.
The shim took the verdict apart with `head`, `cut`, `sed` and `tr`, and BSD `cut` drops a final line without a newline.
The shim now reads the rule and the `protected_branches` list in the shell.
A force push onto a protected branch asks on macOS as it does on Linux, and on a PATH that has git but not those tools.
`ljos onboard` rewrites the installed shims.
