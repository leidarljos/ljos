Curl options on the command line that send a request somewhere the
URL does not say were skipped: `--resolve`, `--connect-to`, a proxy,
a `Host:` header, a DoH URL, a unix socket, and any option the guard
did not know. A POST to another host could reach GitHub. The command
line and a curlrc now read options through one table, abbreviations
and `--no-` included, and each of those leaves the target unknown.
