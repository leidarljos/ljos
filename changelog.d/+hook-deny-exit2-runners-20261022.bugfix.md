The runner relay now treats Windsurf, Factory, Qwen and Crush as
exit-code gates: a deny exits 2 and writes the reason to stderr, so
their command guards block refused commands reliably.

README and integration docs now separate hook-enforced runners from
MCP-only integrations and align the hook-failure table with that relay
contract.
