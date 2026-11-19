`install.sh` asked for the vissue 0.24.1 archive, which was tagged and never released, and fell back to two source builds.
It now installs the releases named in `.github/release-pin` (vissue 0.25.0), and `scripts/release-pin-check.sh` fails CI when a pinned archive does not resolve.
`.github/sibling-pin` builds the same vissue for the tests, and `scripts/release-install-check.sh` fails when the two files differ.
A source build is announced on one line that says it is slow.
