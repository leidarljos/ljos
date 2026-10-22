`git send-pack` does not run the pre-push hook, and the shell
guard only checked its ref names. `develop` on another
repository was allowed, and the push landed there.

A send-pack written on the line is checked against the
repository allowlist. A name git cannot resolve is refused.
An ordinary `git push` is still left to the hook.
