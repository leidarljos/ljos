`scripts/changelog-check.py` runs in the changelog CI job.
It fails a patch version that has a `feature` or `removal` fragment waiting, and a release after 0.45.4 with a Features or Removals section under a patch number.
It also fails a release, or a set of waiting fragments, that states one change twice, as 0.45.4 did for the `url` 2.5.9 bump.
`CHANGELOG.md` 0.45.4 now says that bump once.
