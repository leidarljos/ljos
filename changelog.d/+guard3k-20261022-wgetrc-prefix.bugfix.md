The guard reads a wgetrc before wget does, and a `WGETRC` or
`SYSTEM_WGETRC` set on the line was not a protected write target, so
`printf 'input = https://api.github.com/user\n' > f && SYSTEM_WGETRC=f
wget ...` was judged from the file as it was before the line ran. A
path the line sets in either variable, as a prefix, an `env` or
`export` argument, or a bare assignment, is protected now, so a write
to it on that line is refused. A relative `SYSTEM_WGETRC` is read
from the command's directory, and a missing one still means
`/etc/wgetrc`, as wget reads it.
