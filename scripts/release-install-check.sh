#!/bin/sh
# The release attaches scripts/install.sh and the archives the build job
# uploads. This runs that installer against a local directory of those
# archive names, for every target the release workflow builds, and refuses
# a script that asks for a different name.
#
# A missing archive in that directory is one line naming the archive and
# the next command. It does not fall through into a source build. With no
# --asset-dir, a missing download names the rustc a source build needs.
# --no-build on that download stops before the rustc question.
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

# The versions install.sh uses when nobody passes --pin or --latest.
# The file is the source. The fallback in the script is for a download
# that does not sit next to this tree, and it has to name the same ones.
while IFS= read -r line || [ -n "$line" ]; do
  trim=${line#"${line%%[![:space:]]*}"}
  case "$trim" in
    '' | \#*) continue ;;
  esac
  read -r name _repo _commit ver _extra <<EOF
$trim
EOF
  grep -F -q "$name=$ver" "$INSTALL" ||
    die "install.sh fallback does not pin $name=$ver from .github/sibling-pin"
done < "$ROOT/.github/sibling-pin"

LJOS_VER=$(sed -n 's/^version *= *"\([0-9.]*\)".*/\1/p' "$ROOT/Cargo.toml" | head -n 1)
[ -n "$LJOS_VER" ] || die "Cargo.toml has no version"
MIN_RUST=$(sed -n 's/^rust-version *= *"\([0-9.]*\)".*/\1/p' "$ROOT/Cargo.toml" | head -n 1)
[ -n "$MIN_RUST" ] || die "Cargo.toml has no rust-version"

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

# A missing archive in --asset-dir stops in that directory. The line names
# the archive and the next command. cargo is not started.
fake=$(mktemp -d "${TMPDIR:-/tmp}/ljos-release-fake.XXXXXX")
trap 'rm -rf "$STAGE" "$ASSETS" "$fake"; rm -f "$PINS"' EXIT INT TERM
printf '%s\n' '#!/bin/sh' 'echo rustc 1.83.0 \(fake\)' > "$fake/rustc"
printf '%s\n' '#!/bin/sh' 'echo cargo-was-run >> "$CARGO_MARKER"' 'exit 1' > "$fake/cargo"
# install.sh reads curl's stdout as the HTTP status. 404 on the archive
# is a missing file. 200 on the tag page means the release exists, so
# the script does not report a version that was never published.
cat > "$fake/curl" <<'EOF'
#!/bin/sh
url=
for a in "$@"; do
  case $a in
    http://*|https://*) url=$a ;;
  esac
done
case $url in
  */releases/tag/*|*/releases/tags/*)
    printf '%s' 200
    exit 0
    ;;
esac
printf '%s' 404
exit 0
EOF
chmod 755 "$fake/rustc" "$fake/cargo" "$fake/curl"
gone="$ASSETS/vissue-v$VISSUE_VER-x86_64-unknown-linux-gnu.tar.gz"
rm -f "$gone" "$gone.sha256"
marker=$fake/cargo-ran
target=x86_64-unknown-linux-gnu
asset="vissue-v$VISSUE_VER-$target.tar.gz"
asset_line="install.sh: no $asset in $ASSETS. That is the archive name for $target."
asset_line="$asset_line Put the release archive and its .sha256 in that directory and run \`sh install.sh --asset-dir $ASSETS\` again, or unpack the programs and run \`sh install.sh --from-dir DIR\`."

run_missing() {
  [ -z "$home" ] || rm -rf "$home"
  home=$(mktemp -d "${TMPDIR:-/tmp}/ljos-release-home.XXXXXX")
  rm -f "$marker"
  set +e
  CARGO_MARKER=$marker PATH="$fake:$PATH" \
    sh "$INSTALL" "$@" --bin-dir "$home/bin" > "$home/out" 2> "$home/err"
  status=$?
  set -e
}

expect_line() {
  label=$1
  want=$2
  [ "$status" != 0 ] || die "$label installed anyway"
  grep -F -x -q "$want" "$home/err" || die "$label printed:
$(cat "$home/err")
wanted:
$want"
  [ ! -e "$marker" ] || die "$label started cargo"
  lines=$(grep -c . "$home/err" || true)
  [ "$lines" = 1 ] || die "$label printed $lines lines, wanted 1:
$(cat "$home/err")"
}

home=
run_missing --asset-dir "$ASSETS" --target "$target" \
  --no-embed --no-onboard --no-modify-path $(cat "$PINS")
expect_line "a missing vissue archive" "$asset_line"

# --no-build does not change that refusal.
run_missing --asset-dir "$ASSETS" --target "$target" \
  --no-embed --no-onboard --no-modify-path --no-build $(cat "$PINS")
expect_line "--no-build with --asset-dir" "$asset_line"

# No --asset-dir. A 404 is the source-build path. The line names the
# rustc a source build needs, and cargo is not started.
ljos_asset="ljos-v$LJOS_VER-$target.tar.gz"
source_line="install.sh: no $ljos_asset for $target; a source build needs rustc $MIN_RUST or newer (found rustc 1.83.0). Install Rust from https://rustup.rs and run \`sh install.sh\` again."
run_missing --target "$target" \
  --no-embed --no-onboard --no-modify-path $(cat "$PINS")
expect_line "source build" "$source_line"

# --no-build on that download stops before any rustc question.
nobuild_line="install.sh: no $ljos_asset for $target. The release has no archive for this machine. Supported archives: x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, x86_64-apple-darwin, aarch64-apple-darwin. To build from source, install Rust $MIN_RUST or newer from https://rustup.rs and run \`sh install.sh\` without --no-build."
run_missing --target "$target" \
  --no-embed --no-onboard --no-modify-path --no-build $(cat "$PINS")
expect_line "--no-build with no archive" "$nobuild_line"
rm -rf "$home" "$fake"

echo "release-install-check: install.sh matches the published archives for all 4 targets"
