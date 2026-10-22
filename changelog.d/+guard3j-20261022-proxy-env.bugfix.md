curl, wget, and httpie send through a proxy named in `http_proxy`,
`https_proxy`, or `all_proxy`, and the guard never read them, so a
forwarding proxy could carry a POST to GitHub. A set proxy variable,
from the process, an `export` on the line, or a prefix, now leaves
the target of a write unknown. `NO_PROXY` does not exempt one.
