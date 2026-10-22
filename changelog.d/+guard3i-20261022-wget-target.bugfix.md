A wgetrc or `-e` command could set an input file, a base, a proxy,
or a `Host:` header, and the guard still took the host on the
command line as the target. A POST to another host was allowed
while the file could send it to GitHub. Those commands now leave
the target unknown, and an unknown target is judged as GitHub.
