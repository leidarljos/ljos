The check for git calls without `GIT_CEILING_DIRECTORIES` now stops at the end of the block a call is in, so a ceiling set in a later function no longer hides a call that lacks one.
The `ljos parent` test and the `scripts/uninstall.sh` test now set the ceiling after `env_clear`.
