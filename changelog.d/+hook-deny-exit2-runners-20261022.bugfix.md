The runner relay now treats Windsurf, Factory, Qwen and Crush as
exit-code gates: a deny exits 2 and writes the reason to stderr, so
their command guards block refused commands reliably.
