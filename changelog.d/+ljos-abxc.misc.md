The packsetd test helper keeps one node per writer it starts, in place of a fixed 128 slots.
It reuses a node once its writer stops.
A Registry owns that list, and a test checks the reuse with fake process ids.
