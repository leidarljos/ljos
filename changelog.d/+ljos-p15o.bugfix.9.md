The git that `ljos_test_boot::git` and `git_at` build now sets `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1` unless the test process names a global or system config of its own, so a `commit.gpgsign` or hook in the login's `~/.gitconfig` no longer breaks a scratch commit.
The cloud hook, cloud VM and objection pause tests do the same in their shell, `ljos seat` and `vissue` helpers.
