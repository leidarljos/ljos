Atoms that waited while this machine's own atom log did not open were not exported by later syncs, even after this machine was a recipient again; only `ljos sync --compact` sealed them.
A sync now seals them whole into one new snapshot once this machine can open what it seals.
