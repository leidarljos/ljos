A curlrc holding only an ordinary option such as `tlsv1.2`,
`http1.0`, or `continue-at` refused every curl, a GET to GitHub and
a POST to another host included, because any option the guard did
not know was opaque. The TLS, HTTP version, auth, retry, range,
trace, and output options are known and harmless now, on the command
line and in a curlrc. The options that move the target stay parsed.
