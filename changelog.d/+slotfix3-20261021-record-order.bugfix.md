`ljos record check` reads legacy unsigned ballots by commit order, as the
push gate does: a ballot counts as legacy when its issue held it in the
commit that brought the tracker's first pin. It read the committer date
of the ballot's commit before, which any tracker writer sets.
