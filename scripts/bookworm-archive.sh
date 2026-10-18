#!/bin/bash
# Bookworm-compatible x86_64 archive of this commit.
#
# The ljos binaries are built the way .github/workflows/release.yml builds
# the linux target: cargo --locked --release --target x86_64-unknown-linux-gnu
# for the ljos and ljos-hud packages, with the toolchain rust-toolchain.toml
# pins. vissue, packsetd, and ljos-consensus are the other programs the
# seat bundle at 4410a5f shipped, at those versions.
#
# Run this on Debian 12. A fresh image installs the compiler and libwayland
# first. The archive is dist/ljos-<sha>-x86_64-linux.tar.gz and a sha256
# file beside it. It does not tag or publish a release.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

export DEBIAN_FRONTEND=noninteractive

if [ -f "${HOME}/.cargo/env" ]; then
  # shellcheck disable=SC1091
  . "${HOME}/.cargo/env"
fi

need_apt=0
command -v git >/dev/null 2>&1 || need_apt=1
command -v curl >/dev/null 2>&1 || need_apt=1
command -v pkg-config >/dev/null 2>&1 || need_apt=1
pkg-config --exists wayland-client 2>/dev/null || need_apt=1
if [ "$need_apt" -eq 1 ]; then
  apt-get update
  apt-get install -y --no-install-recommends \
    build-essential ca-certificates curl git pkg-config libwayland-dev
fi

if ! command -v rustc >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs |
    sh -s -- -y --profile minimal --default-toolchain none
  # shellcheck disable=SC1091
  . "${HOME}/.cargo/env"
  channel=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)
  test -n "$channel"
  rustup toolchain install "$channel" --profile minimal \
    --component rustfmt,clippy --target x86_64-unknown-linux-gnu
fi

if ! git config --global --get-all safe.directory 2>/dev/null | grep -qx "$root"; then
  git config --global --add safe.directory "$root"
fi

rustup target list --installed | grep -qx 'x86_64-unknown-linux-gnu'

sha=$(git rev-parse HEAD)
target=x86_64-unknown-linux-gnu
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target/bookworm}"

cargo build --locked --release --target "$target" -p ljos -p ljos-hud

prefix=${BOOKWORM_PREFIX:-/tmp/ljos-seat-bins}
cargo install --locked --root "$prefix" vissue-cli --version 0.21.0
cargo install --locked --root "$prefix" packset --version 0.15.2
cargo install --locked --root "$prefix" ljos-consensus --version 0.7.0

name="ljos-${sha}-x86_64-linux"
stage=$(mktemp -d)
dir="$stage/$name"
mkdir -p "$dir"
bindir="$CARGO_TARGET_DIR/$target/release"
cp "$bindir/ljos" "$bindir/ljos-mcp" "$bindir/ljos-hud" "$dir/"
cp "$prefix/bin/vissue" "$prefix/bin/packsetd" "$prefix/bin/ljos-consensus" "$dir/"
cp README.md CHANGELOG.md LICENSE "$dir/"
chmod 755 "$dir/ljos" "$dir/ljos-mcp" "$dir/ljos-hud" \
  "$dir/vissue" "$dir/packsetd" "$dir/ljos-consensus"
(
  cd "$dir"
  sha256sum ljos ljos-mcp ljos-hud vissue packsetd ljos-consensus > SHA256SUMS
)

# Debian 12 is glibc 2.36. A symbol newer than that will not run there.
too_new=$(
  readelf -V \
    "$dir/ljos" "$dir/ljos-mcp" "$dir/ljos-hud" \
    "$dir/vissue" "$dir/packsetd" "$dir/ljos-consensus" |
    awk '
      /GLIBC_[0-9]/ {
        for (i = 1; i <= NF; i++) {
          if ($i ~ /GLIBC_[0-9]/) {
            v = $i
            sub(/.*GLIBC_/, "", v)
            sub(/[^0-9.].*/, "", v)
            n = split(v, a, ".")
            major = a[1] + 0
            minor = (n > 1 ? a[2] + 0 : 0)
            if (major > 2 || (major == 2 && minor > 36)) print v
          }
        }
      }
    ' | sort -u
)
if [ -n "$too_new" ]; then
  printf 'glibc newer than 2.36: %s\n' "$too_new" >&2
  exit 1
fi

for bin in ljos ljos-mcp ljos-hud vissue packsetd ljos-consensus; do
  "$dir/$bin" --version
done

out=${ARCHIVE_DIR:-$root/dist}
mkdir -p "$out"
tar -C "$stage" -czf "$out/$name.tar.gz" "$name"
(
  cd "$out"
  sha256sum "$name.tar.gz" > "$name.tar.gz.sha256"
)
rm -rf "$stage"
printf '%s\n' "$out/$name.tar.gz"
cat "$out/$name.tar.gz.sha256"
