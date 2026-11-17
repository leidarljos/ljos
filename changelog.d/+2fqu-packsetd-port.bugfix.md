A test no longer leaves `packsetd` listening on `127.0.0.1:8761`.
Integrate tests set `PACKSET_URL` to `off`, so a writer already on that port does not change the result.
`ljos` names both homes in one line when the writer on its port is for a different `HOME`.
