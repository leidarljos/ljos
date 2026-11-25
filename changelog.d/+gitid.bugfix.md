Tracker commits and merges take their author and committer from git config, not from identity the calling seat exports.
A ticket commit, a tracker push's merge or rebase, the merge `ljos sync` and `ljos inbox --sync` make, and the user-rules merge and commit all drop `GIT_AUTHOR_*`, `GIT_COMMITTER_*` and `EMAIL`, and the `user`, `author` and `committer` names and emails and any include in `GIT_CONFIG_PARAMETERS` or `GIT_CONFIG_COUNT`.
A non-UTF-8 `GIT_CONFIG_PARAMETERS` is read as bytes, as git reads it.
A `GIT_CONFIG_COUNT` git would refuse is refused with one line that names it; `ljos sync` used to abort on `GIT_CONFIG_COUNT=99999999999999`.
The doctor's `git identity` row asks git the way a tracker commit does, so an identity exported only in the environment is a warning.
