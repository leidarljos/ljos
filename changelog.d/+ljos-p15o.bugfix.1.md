A test whose `HOME` is unset, a temp root, or too long for a socket path now stops with that reason instead of making its seat under the checkout's `crates/target`.
Every git ljos starts sets `GIT_CEILING_DIRECTORIES` to the ceilings it inherited plus the `target` and `crates/target` of the build tree it runs from, so a git in a directory there whose `.git` is missing no longer reaches the checkout's `.git/config`.
Commands that clear their environment set that variable again.
