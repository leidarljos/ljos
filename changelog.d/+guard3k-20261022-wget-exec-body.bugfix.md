Wget reads a `-e` or `--execute` command as a wgetrc line, with the
name in any case and every `-` and `_` dropped, so `-e post-data=x`
sends a POST. The guard matched only the underscore spellings, judged
the request a GET, and let it reach GitHub or follow a redirect there.
Every spelling of `post-data`, `post-file`, `body-data`, `body-file`,
`method`, and `max-redirect` in `-e` is read now.
