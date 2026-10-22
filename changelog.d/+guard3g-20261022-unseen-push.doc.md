The cloud guard docs now say which pushes this machine does not
see. A script, make, python, or dulwich is not on the command
line the shell guard reads. `--no-verify` inside a script skips
the pre-push hook. `git send-pack` does not run that hook. A
server-side ruleset is what stops those.
