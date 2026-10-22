`curl -L` and plain `wget` follow a redirect, and a 307 or 308 keeps
the method, so a POST to another host could end at GitHub. Curl's
`-L`, `--location`, and `--location-trusted`, and wget unless
`max_redirect` is 0, now leave the target of a write unknown.
