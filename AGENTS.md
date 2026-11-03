# House rules

A runner reads this file at the start of a sitting in this checkout.

## Checks

The `test` job in [`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs these, in this order.

```
cargo fmt --all --check
```

```
cargo clippy --locked --workspace --all-targets -- -D warnings
```

```
cargo test --workspace --no-fail-fast
```

The test job's Model Context Protocol (MCP) tool list size step installs the tokenizer.
Then it runs this command.

```
scripts/mcp-size.sh --check
```

The Doc samples step runs these two.

```
python3 docs/scripts/gen_samples.py
```

```
git diff --exit-code
```

The `deny` job runs this one.

```
cargo deny check
```

## Tests

The programs a test run needs are named under [Tests](CONTRIBUTING.md#tests).
Run the suite from a shell with no `LJOS_OFF` set and outside a runner.
Watch for a pre-push hook that asks for a yes on `/dev/tty`.
Nobody answers it, so a test or push run under it hangs with no output.
Leave `core.hooksPath` unset, so that hook is not named.
Run the suite, and every push, from a shell with no terminal attached.

```
setsid -w cargo test --workspace --no-fail-fast < /dev/null
```

If a run stops printing, check for a hook waiting on `/dev/tty` before you read it as a slow test.

## Changelog fragments

Add one fragment per change.
[Changelog entries](CONTRIBUTING.md#changelog-entries) gives the name, the types and the draft check.
So does [`towncrier.toml`](towncrier.toml).

## Version copies

The version is written by hand in `[workspace.package]` in [`Cargo.toml`](Cargo.toml).
Every copy [`cogfiles.txt`](cogfiles.txt) lists follows it.
[`scripts/ljos_version.py`](scripts/ljos_version.py) rewrites those copies.
The Version copies step in [`.github/workflows/ci.yml`](.github/workflows/ci.yml) checks them.

## Doc samples

[`gen_samples.py`](docs/scripts/gen_samples.py) writes the doctor, version and help samples the docs commit.
The Doc samples step runs that script and then `git diff --exit-code`, and fails when a committed sample differs.

## Commit identity

[`scripts/identity-gate.sh`](scripts/identity-gate.sh) checks the commits a push or a pull request adds.
The rule is [Commit identity](CONTRIBUTING.md#commit-identity).

## Cloud briefs

Step 0 is `ljos mesh up`.
Run it before the rest of the brief.
Pass the join key with `--key-file` or `LJOS_MESH_KEY_FILE`.
The key stays in that file.
A failure prints one line on stderr and exits 0, and the brief continues.
`ljos mesh down` leaves the mesh when the brief is finished.
[docs/mesh.md](docs/mesh.md) has the login server, the hostname and the flags.
