# Reproducible Release Build Recipe

To build byte-identical `ljos`, `ljos-mcp`, and `ljos-hud` binaries across
different checkout directories (whether under `/tmp` or inside `$HOME`), place
`--remap-path-prefix=${SRCDIR}=/build/ljos` **after**
`--remap-path-prefix=${HOME}=/home/build` in `RUSTFLAGS`. Because `rustc`
applies the last matching `--remap-path-prefix`, a checkout inside `$HOME`
remaps its source paths to `/build/ljos` rather than `/home/build/...`:

```bash
cd "$SRCDIR"
export SOURCE_DATE_EPOCH=1774656000
export CARGO_INCREMENTAL=0
export LC_ALL=C.UTF-8
export TZ=UTC
export RUSTFLAGS="--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup --remap-path-prefix=${HOME}=/home/build --remap-path-prefix=${SRCDIR}=/build/ljos"
cargo +1.89.0 build --release --locked --workspace --bins
```
