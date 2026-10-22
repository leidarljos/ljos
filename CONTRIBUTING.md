# Contributing

## Changelog entries

Do not edit `CHANGELOG.md` in a pull request.
Add one fragment per change under `changelog.d/` instead, named `<PR number>.<type>.md`.
The type is one of `feature`, `bugfix`, `doc`, `removal` or `misc`.
A change with no pull request yet can use `+<branch>.<type>.md`.
Write the entry as one Markdown bullet body, one sentence per line, with no leading dash.
At release time `towncrier build --version X.Y.Z` collects the fragments into `CHANGELOG.md` and removes them.

## Commit identity

CI checks only the commits a push or a pull request adds. A push
uses `before..sha`. A new branch, whose `before` is all zeros, uses
the commits that are not on any other remote branch. A pull request
uses `base..head`. `scripts/identity-gate.sh` checks that range and
fails when an author or committer is not Rohit Goswami
<rgoswami@ieee.org>, or a message has a Co-authored-by or
Signed-off-by trailer, a generated-with line, or the machine-account
text named in the script. Commits already on the base are outside
the range. `scripts/identity-gate-test.sh` covers a clean range, a
wrong author, a wrong committer and a trailer.
