The pre-push hook took its allowlist from HOME or
XDG_CONFIG_HOME, and it ran readlink and tr from PATH. A push
could point those at another allowlist, or at programs that
hid the real path, and the hook then allowed the push.

The hook now resolves the allowlist from its own path, takes
the tilde root from the passwd entry, and calls those programs
by absolute path with PATH fixed to /usr/bin:/bin.
