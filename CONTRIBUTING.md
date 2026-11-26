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
A patch release (`Z` is not 0) carries fixes, documentation and miscellaneous entries only.
A `feature` or `removal` fragment needs a minor bump, because towncrier files every `feature` fragment under Features whatever version it is given.
`scripts/changelog-check.py` fails when `Cargo.toml` names a patch version that has no section in `CHANGELOG.md` yet and a `feature` or `removal` fragment is waiting, and when a release after 0.45.4 has a Features or Removals section under a patch number.
Move the fragment to the next minor release or give it another type.
The same script fails when one release, or the waiting fragments, state a change twice: the same entry, or one RUSTSEC, GHSA or CVE advisory in two entries.
The same script fails when `CITATION.cff` `version` or `date-released` differs from the newest released heading in `CHANGELOG.md`.
One change is one fragment, also when it lands on two branches.
The CI `changelog fragments` job runs it, and `--selftest` runs its own cases.

## Tests

`cargo test --locked --workspace` runs the real programs ljos drives, so put them on `PATH` first.
`scripts/sibling-tools.sh DEST` builds them into DEST, and the `cargo test` job runs that script.
It reads `.github/sibling-pin` for `vissue`, `claimdag` and `deedar`, and the commits in the script for `packset`, `packsetd`, [phronesis](https://github.com/leidarljos/phronesis) and `ljos-policyd`.
It installs the binaries into DEST and checks each `--version` against its pin.
`scripts/install.sh` installs a different list: `.github/release-pin` names released versions of `vissue`, `claimdag` and `deedar`, and `scripts/release-pin-check.sh` asks GitHub that each archive is there.
The release pin is written in three places, and a change to one is a change to all three:
`.github/release-pin` is the source, `crates/ljos-cli/release-pin` is a byte-for-byte copy that the published `ljos` crate embeds (a crate cannot read `.github/`), and the `for item in` line in `apply_release_pins` in `scripts/install.sh` is the fallback list for an installer downloaded without this tree.
`scripts/release-install-check.sh` fails when the crate copy is not identical to the source, or when the fallback list names a tool or version the source does not, or lacks one it does, and each tool's version in `.github/sibling-pin` has to match its row in `.github/release-pin`.
The `release installer` job and the release workflow run it, and `cp .github/release-pin crates/ljos-cli/release-pin` refreshes the copy.
`scripts/release-pin-check.sh` then asks GitHub that each pinned archive exists.
`ljos-policyd version` must contain `(phronesis)`, because the guard tests ask that build how a shell line reads.
The script needs `git`, the Rust toolchain in `rust-toolchain.toml`, and the packages `g++`, `meson`, `ninja-build`, `capnproto` and `pkg-config`.
`g++` is the C++ compiler phronesis's c-capnproto wrap needs; without it `meson setup` stops with `Compiler c++ cannot compile programs.`
`age` and `age-keygen` 1.1.1 come from the `age` package.
`apt-get install age` installs that package on Ubuntu 24.04, and `brew install age` installs it on macOS.
The script does not build them.
`source scripts/sibling-tools.sh` builds the pinned `vissue` 0.25.0 and `packsetd` 0.18.2 and puts them on `PATH`.
The release test is `LJOS_TEST_REQUIRE_TOOLS=1 cargo test --locked -p ljos`.
Run that source line first. It reads `.github/sibling-pin` and installs `vissue` the same way the cargo test job does.
A test that runs `vissue` and does not check for the binary first fails when `vissue` is missing.
A test that needs `age-keygen`, `emacs`, `packsetd` or that `vissue` checks for the tool first.
`emacs` 29.3 with Org comes from the `emacs-nox` package: `apt-get install emacs-nox` on Ubuntu 24.04, `brew install emacs` on macOS.
The handbook and workflow render tests run `org-lint` in the `emacs` they find on `PATH`.
They load `ob-dot`, so the Org 9.8 in Homebrew's `emacs` and the Org 9.6 in `emacs-nox` both know the `dot` block.
When it is missing the test prints one line naming the tool, the version and that command, and the test does not fail.
`LJOS_TEST_REQUIRE_TOOLS=1` fails the test with that same line.
A `packsetd` a test starts runs with `INSIDE_EMBED=off` unless the test names an encoder with `PACKSET_EMBED`, so an installed `packset-embed` does not change what a test or a doc sample prints.
The cargo test job sets that variable.
Run the suite from a shell with no `LJOS_OFF` set and outside an agent runner.

## Commit identity

A push, a tag push or a pull request checks every commit reachable from that commit, and skips none.
GitHub starts no run for a `[skip ci]` push, so the next push, tag or release run catches its commits.
The check fails when an author is not Rohit Goswami <rgoswami@ieee.org>, a committer is not that identity, or a message has a Co-authored-by or Signed-off-by trailer, a generated-with line, or the machine-account text named in the identity script.
The one other committer it takes is GitHub <noreply@github.com> on a merge of two or more parents, which is the commit github.com writes when a pull request is merged there.
That merge must carry one signature, and `gpg` must report it with `GOODSIG` and a `VALIDSIG` line whose primary key fingerprint is one that `webflow_fprs` in [`scripts/identity-gate.sh`](scripts/identity-gate.sh) pins.
The pin is GitHub's web-flow key 968479A1AFF927E37D1A566BB5690EEEBB952194, and [`scripts/github-web-flow.asc`](scripts/github-web-flow.asc) holds it.
`gpg` checks the signature in a temporary `GNUPGHOME` that holds only that file, so no other keyring is read, and a key file that lacks a pinned key fails the check.
A GitHub commit that is not a merge, is unsigned, is signed by another key, or is signed by an expired or revoked key fails, and so does a signed merge with any other committer.
GitHub's older web-flow key 5DE3E0509C47EA3CF04A42D34AEE18F83AFDEB23 expired on 16 January 2024, signs no commit on `develop` or `main`, and is not pinned.
When GitHub changes its key, the failure names the commit and the key that signed it.
Check that key at <https://github.com/web-flow.gpg>, add it to `scripts/github-web-flow.asc`, and add its primary fingerprint to `webflow_fprs`.
The check runs `gpg`, and a missing `gpg` fails it.
Commit dates are not read.
A git error fails the check.
The identity test covers a clean range, a wrong author, a wrong committer, a GitHub merge signed by a throwaway key the test pins, the same merge under GitHub's key, an unsigned GitHub merge, a GitHub commit that is not a merge, a signed merge with another committer, a merge signed by an unpinned key whose user ID says GitHub, a pin that names a signing subkey, a subkey of an unpinned primary key, a subkey of a pinned primary key, a merge signed by a key the key file lacks, a merge signed by an expired pinned key, a key file that lacks a pinned key, 1dd43134 as github.com signed it and with its message edited, a differing committer date, a foreign commit, a trailer, no exclusion list, a bad commit that is never skipped, a bad commit on a branch, a tag and a pull request, a later push after a `[skip ci]` bad commit, a re-run after the branches are deleted and created again, a release merge, and one push of a bad commit to main and develop.
