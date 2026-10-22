An unsigned seat ballot counts as legacy only when its issue's entry
already held it in the tracker commit that brought the first seat pin,
by commit order. The gate read the committer date of the ballot's commit
before, which any tracker writer sets, so a ballot cast after signing
with a backdated commit counted unsigned. A tracker that never committed
a pin has no legacy ballots; `LJOS_UNSIGNED_BALLOTS=allow` is for that.
