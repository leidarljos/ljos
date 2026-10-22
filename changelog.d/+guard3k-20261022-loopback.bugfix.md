Every IPv4 and IPv6 literal counted as a possible GitHub peer, so
`curl -X POST http://127.0.0.1:8761/x` and `curl -X POST
'http://[::1]:8761/x'` were refused while the same write to
`localhost` passed. A curl or wget write straight to `::1` or to
`127.0.0.0/8`, in any form the clients take, passes now. A proxy,
`--resolve`, `--connect-to`, `-x`, a `Host:` header, or a redirect
still refuses it, and wget needs `--max-redirect=0`, as it does for
`localhost`. A `[` that opens an IPv6 address is no longer read as a
curl glob. HTTPie keeps loopback refused, and every other address
stays refused.
