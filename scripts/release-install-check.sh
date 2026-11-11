#!/bin/sh
# The release attaches scripts/install.sh and the archives the build job
# uploads. This runs that installer against a local directory of those
# archive names, for every target the release workflow builds, and refuses
# a script that asks for a different name.
#
#   scripts/release-install-check.sh
#   scripts/release-install-check.sh --release-dist dist
#
# --release-dist is the release job's downloaded artifacts. Each target's
# ljos-vVERSION-TARGET.tar.gz has to be in it. The install itself uses
# stub archives, so it does not run the release binaries.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
REL=$ROOT/.github/workflows/release.yml
INSTALL=$ROOT/scripts/install.sh
RELEASE_DIST=

while [ $# -gt 0 ]; do
  case $1 in
    --release-dist) RELEASE_DIST=$2; shift 2 ;;
    *) echo "release-install-check: unknown option $1" >&2; exit 1 ;;
  esac
done

die() { printf 'release-install-check: %s\n' "$*" >&2; exit 1; }

[ -f "$REL" ] || die "no $REL"
[ -f "$INSTALL" ] || die "no $INSTALL"

# The build matrix. A new target has to be installed for too, and a
# removed one has to leave this list.
targets=$(sed -n 's/^[[:space:]]*target:[[:space:]]*//p' "$REL" | tr -d ' \r')
want="aarch64-apple-darwin
aarch64-unknown-linux-gnu
x86_64-apple-darwin
x86_64-unknown-linux-gnu"
have=$(printf '%s\n' "$targets" | sed '/^$/d' | sort)
[ "$have" = "$want" ] || die "release.yml targets are:
$have
this check expects:
$want"

# release.yml: name="ljos-${TAG}-${{ matrix.target }}" and that directory
# is the tar. install.sh asks for ljos-vVERSION-TARGET.tar.gz, and TAG is
# v plus the crate version.
grep -F -q 'ljos-${TAG}-${{ matrix.target }}' "$REL" ||
  die "release.yml no longer names the archive ljos-TAG-target"

LJOS_VER=$(sed -n 's/^version *= *"\([0-9.]*\)".*/\1/p' "$ROOT/Cargo.toml" | head -n 1)
[ -n "$LJOS_VER" ] || die "Cargo.toml has no version"

# Archive names the publishers upload. install.sh has to ask for these,
# not for a crate-TARGET.tar.xz name that was never published.
# packset, vissue, deedar, claimdag, ljos-policyd and consensus were
# read from their latest release asset lists on 2026-11-11.
published() {
  repo=$1
  ver=$2
  target=$3
  case $repo in
    ljos|packset|vissue|deedar|claimdag|ljos-policyd)
      printf '%s-v%s-%s.tar.gz\n' "$repo" "$ver" "$target" ;;
    consensus)
      printf 'consensus-v%s-%s.tar.gz\n' "$ver" "$target" ;;
    *) die "no published archive name for $repo" ;;
  esac
}

# Versions fill the name. The shape is what the check holds.
PACKSET_VER=0.17.7
VISSUE_VER=0.24.0
DEEDAR_VER=0.6.1
CLAIMDAG_VER=0.8.0
POLICYD_VER=0.7.0
CONSENSUS_VER=0.10.1

pins() {
  printf '%s\n' \
    --pin "ljos=$LJOS_VER" \
    --pin "packset=$PACKSET_VER" \
    --pin "vissue=$VISSUE_VER" \
    --pin "deedar=$DEEDAR_VER" \
    --pin "claimdag=$CLAIMDAG_VER" \
    --pin "ljos-policyd=$POLICYD_VER" \
    --pin "consensus=$CONSENSUS_VER"
}

expected_for() {
  target=$1
  {
    published ljos "$LJOS_VER" "$target"
    published packset "$PACKSET_VER" "$target"
    published vissue "$VISSUE_VER" "$target"
    published deedar "$DEEDAR_VER" "$target"
    published claimdag "$CLAIMDAG_VER" "$target"
    published ljos-policyd "$POLICYD_VER" "$target"
    published consensus "$CONSENSUS_VER" "$target"
  } | sort -u
}

# set -- wipes the script's arguments. Keep the pin list in a file.
PINS=$ROOT/.git/release-install-pins.$$
trap 'rm -f "$PINS"' EXIT INT TERM
pins > "$PINS"

# One stub archive per published name. The top directory is the archive
# name without .tar.gz, which is how the release job packs ljos, and
# install.sh finds each program by name anywhere in the archive.
STAGE=$(mktemp -d "${TMPDIR:-/tmp}/ljos-release-stage.XXXXXX")
ASSETS=$(mktemp -d "${TMPDIR:-/tmp}/ljos-release-assets.XXXXXX")
trap 'rm -rf "$STAGE" "$ASSETS"; rm -f "$PINS"' EXIT INT TERM

stub() {
  printf '%s\n' '#!/bin/sh' 'exit 0'
}

pack() {
  asset=$1
  shift
  top=${asset%.tar.gz}
  dir=$STAGE/$top
  rm -rf "$dir"
  mkdir -p "$dir"
  for bin in "$@"; do
    stub > "$dir/$bin"
    chmod 755 "$dir/$bin"
  done
  tar -C "$STAGE" -czf "$ASSETS/$asset" "$top"
  (cd "$ASSETS" && sha256sum "$asset" > "$asset.sha256")
  rm -rf "$dir"
}

for target in $want; do
  got=$(sh "$INSTALL" --print-assets --target "$target" $(cat "$PINS") | sort -u)
  exp=$(expected_for "$target")
  [ "$got" = "$exp" ] || die "install.sh archives for $target:
$got
published archives:
$exp"
  printf '%s\n' "$got" | grep -q '\.tar\.xz$' &&
    die "install.sh asks for a tar.xz archive for $target"

  pack "$(published ljos "$LJOS_VER" "$target" | tr -d '\n')" ljos ljos-mcp ljos-hud
  pack "$(published packset "$PACKSET_VER" "$target" | tr -d '\n')" packset packsetd packset-mcp
  pack "$(published vissue "$VISSUE_VER" "$target" | tr -d '\n')" vissue vissue-mcp
  pack "$(published deedar "$DEEDAR_VER" "$target" | tr -d '\n')" deedar
  pack "$(published claimdag "$CLAIMDAG_VER" "$target" | tr -d '\n')" claimdag
  pack "$(published ljos-policyd "$POLICYD_VER" "$target" | tr -d '\n')" ljos-policyd
  pack "$(published consensus "$CONSENSUS_VER" "$target" | tr -d '\n')" ljos-consensus

  if [ -n "$RELEASE_DIST" ]; then
    name="ljos-v$LJOS_VER-$target.tar.gz"
    [ -f "$RELEASE_DIST/$name" ] || die "release dist has no $name"
  fi
done

need_bins="ljos ljos-mcp packset packsetd packset-mcp vissue vissue-mcp deedar claimdag ljos-policyd ljos-consensus"
for target in $want; do
  home=$(mktemp -d "${TMPDIR:-/tmp}/ljos-release-home.XXXXXX")
  sh "$INSTALL" --asset-dir "$ASSETS" --target "$target" --no-embed --no-onboard \
    --no-modify-path --no-build --bin-dir "$home/bin" $(cat "$PINS") \
    > "$home/out" 2> "$home/err" || die "install.sh failed for $target:
$(cat "$home/out" "$home/err")"
  for bin in $need_bins; do
    [ -x "$home/bin/$bin" ] || die "install.sh did not install $bin for $target"
  done
  rm -rf "$home"
done

# A missing archive must not fall through into cargo. The line names the
# rustc a source build would need, and cargo is not started.
fake=$(mktemp -d "${TMPDIR:-/tmp}/ljos-release-fake.XXXXXX")
trap 'rm -rf "$STAGE" "$ASSETS" "$fake"; rm -f "$PINS"' EXIT INT TERM
printf '%s\n' '#!/bin/sh' 'echo rustc 1.83.0 \(fake\)' > "$fake/rustc"
printf '%s\n' '#!/bin/sh' 'echo cargo-was-run >> "$CARGO_MARKER"' 'exit 1' > "$fake/cargo"
chmod 755 "$fake/rustc" "$fake/cargo"
gone="$ASSETS/vissue-v$VISSUE_VER-x86_64-unknown-linux-gnu.tar.gz"
rm -f "$gone" "$gone.sha256"
marker=$fake/cargo-ran
home=$(mktemp -d "${TMPDIR:-/tmp}/ljos-release-home.XXXXXX")
set +e
CARGO_MARKER=$marker PATH="$fake:$PATH" \
  sh "$INSTALL" --asset-dir "$ASSETS" --target x86_64-unknown-linux-gnu \
    --no-embed --no-onboard --no-modify-path --bin-dir "$home/bin" $(cat "$PINS") \
    > "$home/out" 2> "$home/err"
status=$?
set -e
[ "$status" != 0 ] || die "a missing vissue archive installed anyway"
grep -q 'needs rustc 1.89' "$home/err" || die "source build did not name rustc 1.89:
$(cat "$home/err")"
grep -q 'found rustc 1.83.0' "$home/err" || die "source build did not name the rustc it found:
$(cat "$home/err")"
[ ! -e "$marker" ] || die "cargo ran though rustc is older than 1.89"
# The refusal is one line.
lines=$(grep -c . "$home/err" || true)
[ "$lines" = 1 ] || die "source build printed $lines lines, wanted 1:
$(cat "$home/err")"

# --no-build stops before any rustc question.
rm -rf "$home"
home=$(mktemp -d "${TMPDIR:-/tmp}/ljos-release-home.XXXXXX")
set +e
CARGO_MARKER=$marker PATH="$fake:$PATH" \
  sh "$INSTALL" --asset-dir "$ASSETS" --target x86_64-unknown-linux-gnu \
    --no-embed --no-onboard --no-modify-path --no-build --bin-dir "$home/bin" \
    $(cat "$PINS") > "$home/out" 2> "$home/err"
status=$?
set -e
[ "$status" != 0 ] || die "--no-build installed from a missing archive"
grep -q "no vissue-v$VISSUE_VER-x86_64-unknown-linux-gnu.tar.gz for x86_64-unknown-linux-gnu" "$home/err" ||
  die "--no-build did not name the missing archive:
$(cat "$home/err")"
[ ! -e "$marker" ] || die "cargo ran under --no-build"
rm -rf "$home" "$fake"

echo "release-install-check: install.sh matches the published archives for all 4 targets"
