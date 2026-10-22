Curl expands `{a,b}` and `[1-3]` in a URL, so a POST to
`https://git{hub,xxx}.com/` went to `github.com` while the guard read
one host that is not GitHub. A glob in a URL host on the command line
or in a curlrc now leaves the target unknown, unless `-g` is given.
