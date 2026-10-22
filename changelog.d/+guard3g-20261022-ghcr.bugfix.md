`curl -X PUT https://ghcr.io/...` was allowed. The host check
only named github.com, and ghcr.io accepts the same token.

ghcr.io, githubusercontent.com, and pkg.github.com are GitHub
hosts. A write to one of them is refused like a write to
api.github.com.
