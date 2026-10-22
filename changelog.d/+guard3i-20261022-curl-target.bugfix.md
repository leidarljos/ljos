A curlrc line the guard could not read, a `variable` or `expand-`
option, or a `-K -` config could add a URL, and the guard still
took the host on the command line as the whole target. A POST to
another host was allowed while the config could send it to
GitHub. Those now leave the target unknown, and an unknown target
is judged as GitHub.
