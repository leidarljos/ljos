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

The MCP tool list size step in that job installs the tokenizer, then runs:

```
scripts/mcp-size.sh --check
```

The Doc samples step runs:

```
python3 docs/scripts/gen_samples.py
```

```
git diff --exit-code
```

The `deny` job runs:

```
cargo deny check
```

## Tests

The programs a test run needs are named under [Tests](CONTRIBUTING.md#tests).
Run the suite from a shell with no `LJOS_OFF` set and outside a runner.
A pre-push hook that asks for a yes on `/dev/tty` waits for an answer nobody gives, and a test or push run under it hangs with no output.
Leave `core.hooksPath` unset, so that hook is not named.
Run the suite, and every push, from a shell with no terminal attached.

```
setsid -w cargo test --workspace --no-fail-fast < /dev/null
```

If a run stops printing, check for a hook waiting on `/dev/tty` before you read it as a slow test.

## Changelog fragments

Add one fragment per change.
The name, the types and the draft check are in [Changelog entries](CONTRIBUTING.md#changelog-entries) and [`towncrier.toml`](towncrier.toml).

## Version copies

The version is written by hand in `[workspace.package]` in [`Cargo.toml`](Cargo.toml).
Every copy [`cogfiles.txt`](cogfiles.txt) lists follows it.
[`scripts/ljos_version.py`](scripts/ljos_version.py) rewrites those copies.
The Version copies step in [`.github/workflows/ci.yml`](.github/workflows/ci.yml) checks them.

## Doc samples

[`docs/scripts/gen_samples.py`](docs/scripts/gen_samples.py) writes the doctor, version and help samples the docs commit.
The Doc samples step runs that script and then `git diff --exit-code`, and fails when a committed sample differs.

## Commit identity

[`scripts/identity-gate.sh`](scripts/identity-gate.sh) checks the commits a push or a pull request adds.
The rule is [Commit identity](CONTRIBUTING.md#commit-identity).
