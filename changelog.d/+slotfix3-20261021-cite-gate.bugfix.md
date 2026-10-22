The push gate now reads `LJOS_CITE`: a push that carries a cite which
does not stand is denied, with the reason the cite check gives. Before,
nothing on the push path called the cite check, so a cited push ran
whatever the issue's ballots said, and the claims that the push gate
counts the same ballots as `ljos consensus` described no check.
