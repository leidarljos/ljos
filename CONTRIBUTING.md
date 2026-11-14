# Contributing

## Changelog entries

Do not edit `CHANGELOG.md` in a pull request.
Add one fragment per change under `changelog.d/`, named `<PR number>.<type>.md`.
The type is one of `feature`, `bugfix`, `doc`, `removal` or `misc`.
A change with no pull request yet can use `+<branch>.<type>.md`.
A second fragment for the same number and type is `<PR number>.<type>.1.md`, then `.2`, and so on.
Write the entry as one Markdown bullet body, one sentence per line, with no leading dash.
CI runs `towncrier build --draft --version` with the version from `[workspace.package]` in `Cargo.toml`.
A malformed name, an unknown type, or two files for the same number and type fails that draft.
At release time `towncrier build --version X.Y.Z` collects the fragments into `CHANGELOG.md` and removes them.

## Tests

`cargo test --locked --workspace` runs the real programs ljos drives, so put them on `PATH` first.
`scripts/sibling-tools.sh DEST` builds them into DEST, and the `cargo test` job runs that script.
It reads `.github/sibling-pin` for `vissue`, `claimdag` and `deedar`, and the commits in the script for `packset`, `packsetd`, [phronesis](https://github.com/leidarljos/phronesis) and `ljos-policyd`.
It installs the binaries into DEST and checks each `--version` against its pin.
`ljos-policyd version` must contain `(phronesis)`, because the guard tests ask that build how a shell line reads.
The script needs `git`, the Rust toolchain in `rust-toolchain.toml`, and the packages `g++`, `meson`, `ninja-build`, `capnproto` and `pkg-config`.
`g++` is the C++ compiler phronesis's c-capnproto wrap needs; without it `meson setup` stops with `Compiler c++ cannot compile programs.`
`age` and `age-keygen` 1.1.1 come from the `age` package.
`apt-get install age` installs that package on Ubuntu 24.04, and `brew install age` installs it on macOS.
The script does not build them.
`source scripts/sibling-tools.sh` builds the pinned `vissue` 0.24.0 and `packsetd` 0.18.1 and puts them on `PATH`.
A test that needs `age-keygen`, `packsetd` or that `vissue` checks for the tool first.
When it is missing the test prints one line naming the tool, the version and that command, and the test does not fail.
`LJOS_TEST_REQUIRE_TOOLS=1` fails the test with that same line.
The cargo test job sets that variable.
Run the suite from a shell with no `LJOS_OFF` set and outside an agent runner.

## Commit identity

CI checks only the commits a push or a pull request adds. A push
uses `before..sha`. A new branch, whose `before` is all zeros, uses
the commits that are not on any other remote branch. A pull request
uses `base..head`. `scripts/identity-gate.sh` checks that range and
fails when an author or committer is not Rohit Goswami
<rgoswami@ieee.org>, or a message has a Co-authored-by or
Signed-off-by trailer, a generated-with line, or the machine-account
text named in the script. Commit dates are not read. Commits already
on the base are outside the range. `scripts/identity-gate-test.sh`
covers a clean range, a wrong author, a wrong committer, a differing
committer date, a foreign commit and a trailer.
