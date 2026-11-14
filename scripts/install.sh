#!/bin/sh
# Install ljos from its release tarballs, check each tarball against the
# SHA-256 published beside it and its GitHub build attestation, and run
# `ljos onboard` for every coding runner found on this machine. With none
# found, the tracker, the memory writer and the deed store are still set up.
#
# One command for a new machine. It fetches the release assets, checks
# each SHA-256, and sets up the tracker and memory. Search matches words.
# The encoder is not downloaded:
#
#   curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh --no-embed
#
# Two commands, when registration should be its own step. The first
# installs the programs. The second finds Claude Code, Cursor and Codex,
# among the other runners ljos knows, and registers the MCP server and
# the skill for each. A second `ljos onboard` leaves those files as they
# are. `--dry-run` writes nothing. `ljos offboard` removes the registration.
#
#   curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh --no-embed --no-onboard
#   export PATH="$HOME/.local/bin:$PATH" && ljos onboard
#
# With no flag the encoder is installed, as before. It fetches a model on
# first use. `--with-embed` is that same install.
#
#   curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh
#
# Each ljos release attaches this script as the asset install.sh, from
# the first release after 0.28.0 on. Before that release the URL is a 404.
#
# A failure names the cause and the command to run next. No network, a
# download that fails, a checksum that does not match, a truncated
# download, an unsupported OS or CPU, or a missing curl, wget, tar,
# sha256sum or shasum stops there. A second run over the same version
# says so. An older install names both versions. The script prints the
# PATH line for bash, zsh and fish, and does not edit a shell profile
# unless --modify-path is given. An older ljos earlier on PATH is named,
# with the line that puts this install first. A running ljos-mcp is still
# the previous binary until that process is restarted.
#
# Shell completion is one line in the shell's startup file. The script
# prints the line and does not write that file:
#   bash: source <(ljos completions bash)
#   zsh:  source <(ljos completions zsh)
#   fish: ljos completions fish | source
#
# Options:
#   --bin-dir DIR   where the programs go (default: $LJOS_BIN_DIR or ~/.local/bin)
#   --no-embed      skip packset-embed, the encoder for dense search; by
#                   default it is installed (it fetches a model on first
#                   use), and without it search is lexical only
#   --with-embed    accepted for older instructions; the encoder is the
#                   default now
#   --from-dir DIR  take the programs from DIR instead of downloading
#                   (a local build, or an offline copy)
#   --asset-dir DIR take each release archive and its .sha256 from DIR
#                   instead of GitHub. The file names are the ones a
#                   release publishes for this target
#   --target TRIPLE install for TRIPLE (x86_64-unknown-linux-gnu and the
#                   other three the release workflow builds) instead of
#                   this machine
#   --no-build      a missing archive stops the install. No source build
#   --print-assets  print the archive name for each repository and exit.
#                   Each repository needs --pin or --version
#   --no-onboard    install only; onboard nothing
#   --modify-path   when the bin dir is not on PATH, add the export line
#                   to ~/.profile and ~/.bashrc, and to ~/.zshrc when that
#                   file exists. Without this flag the script prints the
#                   line for bash, zsh and fish and edits nothing
#   --no-modify-path
#                   leave the shell profiles alone. This is the default
#   --version VER   install ljos VER (such as 0.29.0) instead of the latest
#   --pin REPO=VER  install REPO at VER (such as packset=0.14.0); repeat
#                   for each repository to hold. vissue, claimdag and deedar
#                   default to the versions in .github/sibling-pin
#   --latest        do not apply those default pins; each unpinned
#                   repository is its latest release
#   --attest        refuse a tarball whose GitHub build attestation is
#                   missing or does not verify
#   --cloud         install the binaries, skip onboarding and packset's
#                   encoder, then run `ljos cloud up` (clone
#                   LJOS_TRACKER_REMOTE, write the harness MCP command, and
#                   on a cloud VM install the ljos release's encoder).
#                   Idempotent: when the binaries are already installed
#                   and `ljos cloud up` answers, that command is all that
#                   runs
#   --force         with --cloud, download again instead of reusing the
#                   binaries already in --bin-dir
#   --help          this text
#
# The checksum guards against a broken or truncated download. It comes
# from the same release page as the tarball, so it does not prove who
# built the tarball. The build attestation does: a statement signed
# through Sigstore that the repository's release workflow built that
# exact file. The script checks it with whichever of these it finds:
#   - gh 2.49 or newer, signed in: `gh attestation verify`
#   - gh 2.49 or newer, not signed in, or cosign: the attestation is
#     fetched from GitHub's public API (needs jq or python3 to read it)
#     and checked offline
# With none of them it says so once and goes on; --attest stops there.
# An attestation that is found and does not verify is a warning, or
# with --attest the end of the install. One line at the end sums up.
#
# A pinned version that has no release for this platform is built from
# that version's crate, never from a newer one.
set -eu

BIN_DIR=${LJOS_BIN_DIR:-$HOME/.local/bin}
WITH_EMBED=1
FROM_DIR=
ASSET_DIR=
TARGET_OVERRIDE=
ONBOARD=1
MODIFY_PATH=0
OWNER=leidarljos
PINS=
ATTEST=0
CLOUD=0
FORCE=0
NO_BUILD=0
PRINT_ASSETS=0
LATEST=0
SOURCE_ONLY=0
SHADOW=0

say() { printf '%s\n' "$*"; }

# Put a program in place by rename. A copy over a program that is running
# fails with "Text file busy"; a rename replaces the directory entry and
# the running process keeps the old file until it exits.
place() {
  tmp="$2.new.$$"
  cp "$1" "$tmp" || die "could not copy $1 into $BIN_DIR. Check that the directory is writable and run \`sh install.sh\` again."
  chmod 755 "$tmp" || die "could not mark $tmp executable. Run \`sh install.sh\` again."
  if ! mv -f "$tmp" "$2"; then
    rm -f "$tmp"
    die "could not replace $2. If a program is using it, stop that process and run \`sh install.sh\` again."
  fi
}

# The oldest Rust these crates build with, from Cargo.toml's rust-version
# when the tree is here, else the release's floor. Checked before any
# source build, since a distro cargo older than this fails on the first
# crate with a message that names neither the floor nor rustup.
MIN_RUST=1.89
rust_floor() {
  root=$(CDPATH='' cd -- "$(dirname "$0")/.." 2>/dev/null && pwd)
  if [ -n "$root" ] && [ -f "$root/Cargo.toml" ]; then
    v=$(sed -n 's/^rust-version *= *"\([0-9.]*\)".*/\1/p' "$root/Cargo.toml" | head -n1)
    [ -n "$v" ] && MIN_RUST=$v
  fi
}
# rustc's version, or nothing when rustc is not on PATH.
rust_have() {
  command -v rustc >/dev/null 2>&1 || return 1
  rustc --version 2>/dev/null | awk '{print $2}' | sed 's/-.*//'
}

# True when HAVE is at least MIN_RUST. Compares major and minor.
rust_new_enough() {
  have=$1
  [ -n "$have" ] || return 1
  awk -v min="$MIN_RUST" -v have="$have" 'BEGIN {
    split(min, a, "."); split(have, b, ".");
    exit !(b[1] + 0 > a[1] + 0 || (b[1] + 0 == a[1] + 0 && b[2] + 0 >= a[2] + 0))
  }'
}

need_rust() {
  rust_floor
  have=$(rust_have || true)
  if ! rust_new_enough "$have"; then
    if [ -n "$have" ]; then
      die "rustc $have is older than $MIN_RUST, the oldest these crates build with; install a current toolchain from https://rustup.rs, or use a release build for this platform"
    fi
    die "no rustc to build $1; install Rust $MIN_RUST or newer from https://rustup.rs"
  fi
}
warn() { printf '  warning: %s\n' "$*"; }
warn_embed() {
  say "  warning: no packset-embed for $TARGET in $1; search stays lexical."
  say "  Paraphrases that share no word with a memory are missed until an encoder is installed."
}
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case $1 in
    --bin-dir) BIN_DIR=$2; shift 2 ;;
    --with-embed) WITH_EMBED=1; shift ;;
    --no-embed) WITH_EMBED=0; shift ;;
    --from-dir) FROM_DIR=$2; shift 2 ;;
    --asset-dir) ASSET_DIR=$2; shift 2 ;;
    --target) [ $# -ge 2 ] || die "--target needs a triple"; TARGET_OVERRIDE=$2; shift 2 ;;
    --no-build) NO_BUILD=1; shift ;;
    --print-assets) PRINT_ASSETS=1; shift ;;
    --no-onboard) ONBOARD=0; shift ;;
    --modify-path) MODIFY_PATH=1; shift ;;
    --no-modify-path) MODIFY_PATH=0; shift ;;
    --version) [ $# -ge 2 ] || die "--version needs a version"; PINS="$PINS ljos=$2"; shift 2 ;;
    --version=*) PINS="$PINS ljos=${1#--version=}"; shift ;;
    --pin) [ $# -ge 2 ] || die "--pin needs REPO=VER"; PINS="$PINS $2"; shift 2 ;;
    --pin=*) PINS="$PINS ${1#--pin=}"; shift ;;
    --latest) LATEST=1; shift ;;
    --attest) ATTEST=1; shift ;;
    --cloud) CLOUD=1; ONBOARD=0; WITH_EMBED=0; shift ;;
    --force) FORCE=1; shift ;;
    --help|-h) awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; exit 0 ;;
    *) die "unknown option $1 (try --help)" ;;
  esac
done

# The version REPO is pinned to, or nothing for the latest release.
pinned() {
  for pin in $PINS; do
    case $pin in "$1="*) printf '%s' "${pin#*=}"; return ;; esac
  done
}

# vissue, claimdag and deedar follow .github/sibling-pin when this script
# sits in the repository. A downloaded script carries the same versions.
# --pin for one of them wins. --latest leaves every unpinned repository
# at its latest release.
# Three dot-separated numbers, such as 0.23.1. No grep: a PATH that
# cannot fetch a release still has to name curl before anything else.
version_ok() {
  case $1 in
    '' | *[!0-9.]*) return 1 ;;
  esac
  case $1 in
    *.*.*.*) return 1 ;;
  esac
  case $1 in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) return 1 ;;
  esac
  rest=${1#*.*.}
  case $rest in
    *[!0-9]*) return 1 ;;
  esac
}

apply_sibling_pins() {
  [ "$LATEST" = 1 ] && return 0
  here=${0%/*}
  [ "$here" = "$0" ] && here=.
  pinfile=$here/../.github/sibling-pin
  if [ -f "$pinfile" ]; then
    while IFS= read -r line || [ -n "$line" ]; do
      trim=${line#"${line%%[![:space:]]*}"}
      case "$trim" in
        '' | \#*) continue ;;
      esac
      read -r name _repo _commit ver _extra <<EOF
$trim
EOF
      [ -n "${ver:-}" ] || continue
      [ -z "$(pinned "$name")" ] || continue
      PINS="$PINS $name=$ver"
    done < "$pinfile"
    return 0
  fi
  # The versions in .github/sibling-pin. release-install-check.sh
  # refuses a copy that has drifted.
  for item in vissue=0.23.1 claimdag=0.8.0 deedar=0.4.0; do
    name=${item%%=*}
    [ -z "$(pinned "$name")" ] || continue
    PINS="$PINS $item"
  done
}
apply_sibling_pins

for pin in $PINS; do
  ver=${pin#*=}
  ver=${ver#v}
  case $pin in
    *=*) ;;
    *) die "--pin takes REPO=VER, not $pin" ;;
  esac
  version_ok "$ver" ||
    die "${pin%%=*}: $ver is not a version such as 0.29.0"
done

# The binaries are already here and this ljos knows `cloud up`. Skip the
# download. A later boot only refreshes the tracker clone and the MCP path.
if [ "$CLOUD" = 1 ] && [ "$FORCE" = 0 ] && [ -z "$FROM_DIR" ] \
  && [ -x "$BIN_DIR/ljos" ] && [ -x "$BIN_DIR/ljos-mcp" ] && [ -x "$BIN_DIR/ljos-policyd" ] \
  && "$BIN_DIR/ljos" cloud up --help >/dev/null 2>&1; then
  exec "$BIN_DIR/ljos" cloud up
fi

START=$(date +%s)

# fetch URL DEST. Sets FETCH_KIND to ok, net (nothing answered), http
# (the server answered an error other than 404) or missing (404).
FETCH_KIND=ok
FETCH_CODE=
fetch() {
  FETCH_KIND=net
  FETCH_CODE=
  : > "$TMP/fetch.err"
  if command -v curl >/dev/null 2>&1; then
    code=$(curl -sSL -o "$2" -w '%{http_code}' "$1" 2>"$TMP/fetch.err") || {
      FETCH_KIND=net
      FETCH_CODE=${code:-000}
      return 1
    }
    case $code in
      200|201|204) FETCH_KIND=ok; return 0 ;;
      404) FETCH_KIND=missing; FETCH_CODE=$code; return 1 ;;
      *) FETCH_KIND=http; FETCH_CODE=$code; return 1 ;;
    esac
  fi
  if wget -q -O "$2" "$1" 2>"$TMP/fetch.err"; then
    FETCH_KIND=ok
    return 0
  fi
  if grep -q '404' "$TMP/fetch.err" 2>/dev/null; then
    FETCH_KIND=missing
  else
    FETCH_KIND=net
  fi
  return 1
}
latest_url() {
  : > "$TMP/fetch.err"
  if command -v curl >/dev/null 2>&1; then
    out=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$1" 2>"$TMP/fetch.err") || return 1
    printf '%s' "$out"
    return 0
  fi
  wget -q --max-redirect=5 -S --spider "$1" 2>"$TMP/fetch.err" || return 1
  sed -n 's/^ *[Ll]ocation: *//p' "$TMP/fetch.err" | tail -n 1
}
fetch_why() {
  head -n 1 "$TMP/fetch.err" 2>/dev/null | tr -d '\r' || true
}
# The HTTP status, when the server sent one, follows the first error line.
fetch_detail() {
  why=$(fetch_why)
  if [ -n "${FETCH_CODE:-}" ] && [ "$FETCH_CODE" != 000 ]; then
    why="${why:+$why }(HTTP $FETCH_CODE)"
  fi
  printf '%s' "$why"
}
if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) TARGET=x86_64-unknown-linux-gnu ;;
  Linux-aarch64|Linux-"arm64") TARGET=aarch64-unknown-linux-gnu ;;
  Darwin-x86_64) TARGET=x86_64-apple-darwin ;;
  Darwin-"arm64") TARGET=aarch64-apple-darwin ;;
  *) TARGET=unknown ;;
esac
if [ -n "$TARGET_OVERRIDE" ]; then
  TARGET=$TARGET_OVERRIDE
fi

# The seat crates, without the encoder. One command on a machine the
# release archives do not cover.
SOURCE_CMD="cargo install --locked ljos packset vissue-cli vissue-mcp deedar-cli claimdag-cli ljos-policyd ljos-consensus"
supported_line() {
  printf '%s' "$(uname -s) $(uname -m) is not supported. Supported: Linux and macOS on x86_64 and aarch64. On Windows, run this script inside WSL. Build from source: $SOURCE_CMD"
}
supported_target() {
  case $1 in
    x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu|x86_64-apple-darwin|aarch64-apple-darwin) return 0 ;;
    *) return 1 ;;
  esac
}
# Name an unsupported machine, and a missing download tool, before mktemp.
# A person with neither curl nor tar should see that, not a missing mktemp.
if [ -z "$FROM_DIR" ] && [ "$PRINT_ASSETS" != 1 ] && ! supported_target "$TARGET"; then
  rust_floor
  if [ -n "$ASSET_DIR" ]; then
    die "$(uname -s) $(uname -m) is not a supported OS or arch (target $TARGET). Pass --target with one of x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, x86_64-apple-darwin, aarch64-apple-darwin, and run \`sh install.sh --asset-dir $ASSET_DIR --target TRIPLE\` again."
  fi
  have=$(rust_have || true)
  if [ "$NO_BUILD" = 1 ] || ! rust_new_enough "$have"; then
    die "$(supported_line)"
  fi
  SOURCE_ONLY=1
  say "$(supported_line)"
fi
if [ -z "$FROM_DIR" ] && [ "$PRINT_ASSETS" != 1 ] && [ "$SOURCE_ONLY" != 1 ]; then
  miss=
  add_miss() {
    if [ -z "$miss" ]; then
      miss=$1
    else
      miss="$miss, $1"
    fi
  }
  if [ -z "$ASSET_DIR" ] && ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
    add_miss "curl or wget"
  fi
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    add_miss "sha256sum or shasum"
  fi
  if ! command -v tar >/dev/null 2>&1; then
    add_miss "tar"
  fi
  if [ -n "$miss" ]; then
    extra=
    case $miss in
      *curl*) extra=" With the archives on disk: \`sh install.sh --asset-dir DIR\`." ;;
    esac
    die "missing $miss. Install what is named and run \`sh install.sh\` again.$extra With the programs unpacked: \`sh install.sh --from-dir DIR\`."
  fi
fi

TMP=$(mktemp -d "${TMPDIR:-/tmp}/ljos-install.XXXXXX")
trap 'rm -rf "$TMP"' EXIT INT TERM
if ! mkdir -p "$BIN_DIR" 2>"$TMP/mkdir.err"; then
  why=$(head -n 1 "$TMP/mkdir.err" 2>/dev/null | tr -d '\r' || true)
  die "$BIN_DIR is not writable${why:+: $why}. Run \`sh install.sh --bin-dir DIR\` with a directory you can write."
fi
if ! touch "$BIN_DIR/.ljos-install-write" 2>"$TMP/mkdir.err"; then
  why=$(head -n 1 "$TMP/mkdir.err" 2>/dev/null | tr -d '\r' || true)
  die "$BIN_DIR is not writable${why:+: $why}. Run \`sh install.sh --bin-dir DIR\` with a directory you can write."
fi
rm -f "$BIN_DIR/.ljos-install-write"

# A release published before `cloud up` exists cannot bootstrap itself.
# This script, run from the ljos tree, builds that command from the tree
# and the download below leaves the new binary in place.
if [ "$CLOUD" = 1 ] && [ "$FORCE" = 0 ] && [ -z "$FROM_DIR" ]; then
  if ! { [ -x "$BIN_DIR/ljos" ] && "$BIN_DIR/ljos" cloud up --help >/dev/null 2>&1; }; then
    script_root=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
    if [ -f "$script_root/crates/ljos-cli/Cargo.toml" ] && command -v cargo >/dev/null 2>&1; then
      need_rust ljos
      say "Building ljos from this tree so cloud up is available"
      cargo install --locked --force --path "$script_root/crates/ljos-cli" \
        --root "$(CDPATH='' cd -- "$(dirname "$BIN_DIR")" && pwd)"
    fi
  fi
fi

# repository, crate, programs, tarball style
COMPONENTS="ljos ljos ljos,ljos-mcp v
packset packset packset,packsetd,packset-mcp v
vissue vissue-cli vissue v
vissue vissue-mcp vissue-mcp v
deedar deedar-cli deedar v
claimdag claimdag-cli claimdag v
ljos-policyd ljos-policyd ljos-policyd v
consensus ljos-consensus ljos-consensus v"

# No published archive. One line names the rustc a source build needs, and
# cargo is not started when this machine is older than that.
source_build() {
  crate=$1 ver=$2 asset=$3
  rust_floor
  have=$(rust_have || true)
  if ! rust_new_enough "$have"; then
    if [ "$SOURCE_ONLY" = 1 ]; then
      die "$(supported_line)"
    fi
    if [ -n "$have" ]; then
      die "no $asset for $TARGET; a source build needs rustc $MIN_RUST or newer (found rustc $have). Install Rust from https://rustup.rs and run \`sh install.sh\` again."
    fi
    die "no $asset for $TARGET; a source build needs rustc $MIN_RUST or newer (no rustc on PATH). Install Rust from https://rustup.rs and run \`sh install.sh\` again."
  fi
  command -v cargo >/dev/null 2>&1 ||
    die "no $asset for $TARGET; rustc $have meets $MIN_RUST and cargo is not on PATH. Install cargo and run \`sh install.sh\` again."
  if [ -n "$ver" ]; then
    say "  building $crate $ver from source (no $asset for $TARGET)"
    cargo install --locked --quiet --root "$TMP/cargo" --version "=$ver" "$crate" >&2
  else
    say "  building $crate from source (no $asset for $TARGET)"
    cargo install --locked --quiet --root "$TMP/cargo" "$crate" >&2
  fi
  for built in "$TMP"/cargo/bin/*; do
    place "$built" "$BIN_DIR/$(basename "$built")"
  done
}

# What can check a build attestation here. gh gained `attestation` in
# 2.49; an older gh is the same as none.
GH_ATTEST=0
GH_SIGNED_IN=0
if command -v gh >/dev/null 2>&1 && gh attestation --help >/dev/null 2>&1; then
  GH_ATTEST=1
  gh auth status >/dev/null 2>&1 && GH_SIGNED_IN=1
fi
COSIGN=0
command -v cosign >/dev/null 2>&1 && COSIGN=1
JSON_TOOL=
if command -v jq >/dev/null 2>&1; then
  JSON_TOOL=jq
elif command -v python3 >/dev/null 2>&1; then
  JSON_TOOL=python3
fi
if [ "$GH_SIGNED_IN" = 1 ]; then
  VERIFIER=gh
elif [ -n "$JSON_TOOL" ] && [ "$GH_ATTEST" = 1 ]; then
  VERIFIER=gh-bundle
elif [ -n "$JSON_TOOL" ] && [ "$COSIGN" = 1 ]; then
  VERIFIER=cosign
else
  VERIFIER=none
fi
[ "$ATTEST" = 0 ] || [ "$VERIFIER" != none ] ||
  die "--attest needs gh 2.49 or newer (https://cli.github.com), or cosign with jq or python3"

# GET URL into FILE and print the HTTP status, 000 when nothing answered.
api_get() {
  code=
  if command -v curl >/dev/null 2>&1; then
    code=$(curl -sSL -o "$2" -w '%{http_code}' -H 'Accept: application/vnd.github+json' "$1" 2>/dev/null) || true
  else
    wget -q -S -O "$2" --header='Accept: application/vnd.github+json' "$1" 2>"$2.hdr" || true
    code=$(sed -n 's/^  *HTTP\/[0-9.]* \([0-9][0-9][0-9]\).*/\1/p' "$2.hdr" | tail -n 1)
  fi
  printf '%s' "${code:-000}"
}

# Each bundle of a GitHub attestations answer, one JSON document a line.
bundles_of() {
  case $JSON_TOOL in
    jq) jq -c '.attestations[].bundle' "$1" ;;
    python3) python3 -c 'import json, sys
for a in json.load(open(sys.argv[1]))["attestations"]:
    print(json.dumps(a["bundle"]))' "$1" ;;
  esac
}

# Check FILE (whose SHA-256 is DIGEST) against the build attestations of
# REPO's workflows. Prints one word: verified, missing (the release has
# none), unchecked (nothing here can check one) or failed (found, and it
# does not verify). --attest stops on anything but verified.
attest() {
  file=$1 repo=$2 digest=$3
  name=$(basename "$file")
  case $VERIFIER in
    none)
      printf 'unchecked'
      return ;;
    gh)
      if gh attestation verify "$file" --repo "$OWNER/$repo" >/dev/null 2>"$TMP/attest.err"; then
        printf 'verified'
      elif grep -qi 'no attestations found' "$TMP/attest.err"; then
        [ "$ATTEST" = 0 ] || die "$name: $OWNER/$repo published no build attestation for it"
        printf 'missing'
      else
        [ "$ATTEST" = 0 ] || die "$name: its build attestation does not verify: $(head -n 1 "$TMP/attest.err")"
        printf 'failed'
      fi
      return ;;
  esac
  answer="$TMP/attest-$repo-$digest.json"
  code=$(api_get "https://api.github.com/repos/$OWNER/$repo/attestations/sha256:$digest" "$answer")
  case $code in
    200) ;;
    404)
      [ "$ATTEST" = 0 ] || die "$name: $OWNER/$repo published no build attestation for it"
      printf 'missing'
      return ;;
    *)
      [ "$ATTEST" = 0 ] || die "$name: GitHub's attestation API answered $code"
      printf 'unchecked'
      return ;;
  esac
  bundles_of "$answer" > "$answer.jsonl" 2>/dev/null || : > "$answer.jsonl"
  ok=0
  if [ "$VERIFIER" = gh-bundle ]; then
    gh attestation verify "$file" --repo "$OWNER/$repo" --bundle "$answer.jsonl" >/dev/null 2>"$TMP/attest.err" && ok=1
  else
    # cosign 2.x reads this bundle format only behind --new-bundle-format;
    # cosign 3 reads it by default and may drop the flag.
    while IFS= read -r bundle; do
      printf '%s\n' "$bundle" > "$answer.one"
      for flag in --new-bundle-format ""; do
        # shellcheck disable=SC2086
        if cosign verify-blob-attestation $flag --bundle "$answer.one" \
            --certificate-oidc-issuer https://token.actions.githubusercontent.com \
            --certificate-identity-regexp "^https://github\.com/$OWNER/$repo/\.github/workflows/" \
            --type https://slsa.dev/provenance/v1 "$file" >/dev/null 2>"$TMP/attest.err"; then
          ok=1
          break
        fi
      done
      [ "$ok" = 1 ] && break
    done < "$answer.jsonl"
  fi
  if [ "$ok" = 1 ]; then
    printf 'verified'
  else
    [ "$ATTEST" = 0 ] || die "$name: its build attestation does not verify: $(head -n 1 "$TMP/attest.err" 2>/dev/null)"
    printf 'failed'
  fi
}

# exists when the tag is published, missing when GitHub says it is not,
# unknown when the question could not be asked.
release_state() {
  repo=$1
  ver=$2
  if fetch "https://github.com/$OWNER/$repo/releases/tag/v$ver" "$TMP/tag-$repo-$ver"; then
    printf '%s' exists
    return 0
  fi
  case $FETCH_KIND in
    missing) printf '%s' missing ;;
    *) printf '%s' unknown ;;
  esac
  return 0
}

install_one() {
  repo=$1 crate=$2 style=$3 progs=$4
  if [ -n "$FROM_DIR" ]; then
    for p in $(printf '%s' "$progs" | tr ',' ' '); do
      if [ "$p" = packset-embed ] && [ ! -x "$FROM_DIR/$p" ]; then
        warn_embed "$FROM_DIR"
        continue
      fi
      [ -x "$FROM_DIR/$p" ] || die "$FROM_DIR/$p is missing"
      place "$FROM_DIR/$p" "$BIN_DIR/$p"
    done
    printf '%s\n' "$repo" >> "$TMP/copied"
    return
  fi
  ver=$(pinned "$repo")
  ver=${ver#v}
  if [ "$SOURCE_ONLY" = 1 ]; then
    source_build "$crate" "$ver" "$repo-$TARGET.tar.gz"
    case ",$progs," in
      *,packset-embed,*) [ -x "$BIN_DIR/packset-embed" ] || warn_embed "a source build" ;;
    esac
    return
  fi
  if [ -n "$ver" ]; then
    tag=v$ver
  else
    tag=$(latest_url "https://github.com/$OWNER/$repo/releases/latest" || true)
    tag=${tag##*/}
    case $tag in
      v*) ;;
      *)
        why=$(fetch_detail)
        die "could not read the latest $repo release${why:+: $why}. Check the network and run \`sh install.sh\` again. To install a version you name: \`sh install.sh --version VER\`. With the archives on disk: \`sh install.sh --asset-dir DIR\`."
        ;;
    esac
  fi
  case $style in
    v) asset="$repo-$tag-$TARGET.tar.gz" ;;
    dist) asset="$crate-$TARGET.tar.xz" ;;
  esac
  url="https://github.com/$OWNER/$repo/releases/download/$tag/$asset"
  # vissue and vissue-mcp share one archive. The second row reuses it.
  if [ -s "$TMP/$asset" ]; then
    :
  elif [ -n "$ASSET_DIR" ] && [ -s "$ASSET_DIR/$asset" ]; then
    cp "$ASSET_DIR/$asset" "$TMP/$asset"
  elif [ -n "$ASSET_DIR" ]; then
    die "no $asset in $ASSET_DIR. That is the archive name for $TARGET. Put the release archive and its .sha256 in that directory and run \`sh install.sh --asset-dir $ASSET_DIR\` again, or unpack the programs and run \`sh install.sh --from-dir DIR\`."
  elif ! fetch "$url" "$TMP/$asset"; then
    case $FETCH_KIND in
      missing)
        rm -f "$TMP/$asset"
        if [ -n "$ver" ]; then
          rel=$(release_state "$repo" "$ver")
          if [ "$rel" = missing ]; then
            die "$repo $ver has no release. List versions at https://github.com/$OWNER/$repo/releases"
          fi
        fi
        if [ "$NO_BUILD" = 1 ]; then
          die "no $asset for $TARGET. The release has no archive for this machine. Supported archives: x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, x86_64-apple-darwin, aarch64-apple-darwin. To build from source, install Rust $MIN_RUST or newer from https://rustup.rs and run \`sh install.sh\` without --no-build."
        fi
        source_build "$crate" "$ver" "$asset"
        case ",$progs," in
          *,packset-embed,*) [ -x "$BIN_DIR/packset-embed" ] || warn_embed "a source build" ;;
        esac
        return
        ;;
      *)
        if [ "$FETCH_KIND" = net ] && [ -s "$TMP/$asset" ]; then
          rm -f "$TMP/$asset"
          die "$asset: the download was truncated. Removed the partial file. Run \`sh install.sh\` again."
        fi
        rm -f "$TMP/$asset"
        why=$(fetch_detail)
        die "could not download $url${why:+: $why}. Check the network and run \`sh install.sh\` again. With the archives on disk: \`sh install.sh --asset-dir DIR\`. With the programs unpacked: \`sh install.sh --from-dir DIR\`."
        ;;
    esac
  fi
  if [ -n "$ASSET_DIR" ]; then
    cp "$ASSET_DIR/$asset.sha256" "$TMP/$asset.sha256" || die "$asset has no checksum beside it in $ASSET_DIR"
  else
    fetch "$url.sha256" "$TMP/$asset.sha256" || die "$asset has no published checksum"
  fi
  want=$(sed 's/[ *].*//' "$TMP/$asset.sha256" | head -n 1)
  got=$(sha256 "$TMP/$asset")
  if [ "$want" != "$got" ]; then
    rm -f "$TMP/$asset"
    if [ -n "$ASSET_DIR" ]; then
      die "$asset: checksum mismatch. Nothing was installed. Replace the archive and run \`sh install.sh --asset-dir $ASSET_DIR\` again."
    fi
    die "$asset: checksum mismatch. Removed the partial file. Run \`sh install.sh\` again."
  fi
  attested=skipped
  if [ -z "$ASSET_DIR" ]; then
    attested=$(attest "$TMP/$asset" "$repo" "$got")
    printf '%s\n' "$attested" >> "$TMP/attested"
  fi
  mkdir -p "$TMP/x-$repo"
  case $asset in
    *.tar.xz) tar -xJf "$TMP/$asset" -C "$TMP/x-$repo" ;;
    *) tar -xzf "$TMP/$asset" -C "$TMP/x-$repo" ;;
  esac
  for p in $(printf '%s' "$progs" | tr ',' ' '); do
    found=$(find "$TMP/x-$repo" -type f -name "$p" | head -n 1)
    if [ -z "$found" ] && [ "$p" = packset-embed ]; then
      warn_embed "$asset"
      continue
    fi
    [ -n "$found" ] || die "$asset holds no $p"
    place "$found" "$BIN_DIR/$p"
  done
  label=$repo
  case $crate in *-mcp) label=$crate ;; esac
  say "  $label $tag"
  [ "$attested" != failed ] || warn "$asset: its build attestation does not verify: $(head -n 1 "$TMP/attest.err" 2>/dev/null)"
}

for pin in $PINS; do
  printf '%s\n' "$COMPONENTS" | grep -q "^${pin%%=*} " ||
    die "--pin ${pin%%=*}: not one of the repositories this installs"
done

# One archive name per repository row, for this target. A row with no
# pin is an error: printing must not ask GitHub which tag is current.
if [ "$PRINT_ASSETS" = 1 ]; then
  while read -r repo crate progs style; do
    [ -n "$repo" ] || continue
    ver=$(pinned "$repo")
    ver=${ver#v}
    [ -n "$ver" ] || die "--print-assets needs --pin $repo=VER"
    tag=v$ver
    case $style in
      v) asset="$repo-$tag-$TARGET.tar.gz" ;;
      dist) asset="$crate-$TARGET.tar.xz" ;;
      *) die "unknown archive style $style" ;;
    esac
    printf '%s\n' "$asset"
  done <<EOF
$COMPONENTS
EOF
  exit 0
fi

say "Installing ljos into $BIN_DIR ($TARGET)"

# Pids that already have the installed ljos-mcp mapped. After a rename
# those processes keep the previous binary.
pids_for() {
  want=$1
  [ -e "$want" ] || return 0
  for d in /proc/[0-9]*; do
    exe=$(readlink "$d/exe" 2>/dev/null) || continue
    exe=${exe%" (deleted)"}
    [ "$exe" = "$want" ] || continue
    printf '%s\n' "${d#/proc/}"
  done
}
MCP_PIDS=$(pids_for "$BIN_DIR/ljos-mcp" | sort -u | tr '\n' ' ' || true)
PREV_VER=
if [ -x "$BIN_DIR/ljos" ]; then
  PREV_VER=$("$BIN_DIR/ljos" --version 2>/dev/null | head -n 1 || true)
  [ -n "$PREV_VER" ] || PREV_VER="an older ljos"
fi

printf '%s\n' "$COMPONENTS" | while read -r repo crate progs style; do
  if [ "$CLOUD" = 1 ] && [ "$FORCE" = 0 ] && [ "$repo" = ljos ] && [ -x "$BIN_DIR/ljos" ] \
    && "$BIN_DIR/ljos" cloud up --help >/dev/null 2>&1; then
    say "  ljos already provides cloud up; left in place"
    continue
  fi
  if [ "$repo" = packset ] && [ "$WITH_EMBED" = 1 ]; then
    progs="$progs,packset-embed"
  fi
  install_one "$repo" "$crate" "$style" "$progs"
done

# One line for the whole install: what the checksums and the build
# attestations said.
count() {
  n=$(grep -c "^$2\$" "$TMP/$1" 2>/dev/null) || true
  printf '%s' "${n:-0}"
}
if [ -f "$TMP/copied" ]; then
  say "  copied from $FROM_DIR; a local copy has no checksum or attestation to check"
fi
if [ -f "$TMP/attested" ]; then
  total=$(wc -l < "$TMP/attested" | tr -d ' ')
  verified=$(count attested verified)
  missing=$(count attested missing)
  failed=$(count attested failed)
  unchecked=$(count attested unchecked)
  line="Checksums match ($total downloads);"
  if [ "$verified" = "$total" ]; then
    line="$line build attestations verified for all $total."
  elif [ "$unchecked" = "$total" ]; then
    line="$line attestations not checked: install gh 2.49+ or cosign."
  else
    line="$line build attestations: $verified verified"
    [ "$missing" = 0 ] || line="$line, $missing not published"
    [ "$failed" = 0 ] || line="$line, $failed failed (see the warnings above)"
    [ "$unchecked" = 0 ] || line="$line, $unchecked not checked"
    line="$line."
  fi
  say "$line"
fi

# `--cloud` leaves the encoder to `ljos cloud up`.
# This line is the install that skips the encoder.
if [ "$WITH_EMBED" = 0 ] && [ "$CLOUD" = 0 ]; then
  say "Search matches words. The encoder is not installed."
fi

# A shell opened after the install finds the programs: each profile gets
# the line once, under a marker a later install looks for. `cloud up`
# writes its own line, so --cloud leaves the profiles to it.
PROFILE_MARK='# Added by the ljos installer: its programs on PATH.'
case $BIN_DIR in
  "$HOME"/*) shown_dir="\$HOME/${BIN_DIR#"$HOME"/}" ;;
  *) shown_dir=$BIN_DIR ;;
esac
PATH_LINE="export PATH=\"$shown_dir:\$PATH\""
add_to_profiles() {
  added=
  for rc in .profile .bashrc .zshrc; do
    file=$HOME/$rc
    [ "$rc" != .zshrc ] || [ -f "$file" ] || continue
    if [ -f "$file" ] && grep -qxF "$PROFILE_MARK" "$file"; then
      continue
    fi
    if [ -s "$file" ]; then
      printf '\n' >> "$file" || continue
    fi
    printf '%s\n%s\n' "$PROFILE_MARK" "$PATH_LINE" >> "$file" || continue
    added="$added ~/$rc"
  done
  printf '%s' "$added"
}
PATH_HAD_BIN=0
case ":$PATH:" in
  *":$BIN_DIR:"*) PATH_HAD_BIN=1 ;;
esac
AHEAD=$(command -v ljos 2>/dev/null || true)
same_bin() {
  [ "$1" = "$2" ] && return 0
  a=$(readlink -f "$1" 2>/dev/null || printf '%s' "$1")
  b=$(readlink -f "$2" 2>/dev/null || printf '%s' "$2")
  [ "$a" = "$b" ]
}
ver_of() {
  "$1" --version 2>/dev/null | head -n 1 || true
}
ver_num() {
  printf '%s\n' "$1" | awk '
    match($0, /[0-9]+\.[0-9]+\.[0-9]+/) {
      print substr($0, RSTART, RLENGTH)
      exit
    }
  '
}
version_lt() {
  awk -v a="$1" -v b="$2" 'BEGIN {
    split(a, x, ".")
    split(b, y, ".")
    for (i = 1; i <= 3; i++) {
      av = x[i] + 0
      bv = y[i] + 0
      if (av < bv) exit 0
      if (av > bv) exit 1
    }
    exit 1
  }'
}
shell_profiles() {
  say "$BIN_DIR is not on PATH. Add one line to the shell profile named here. This script does not edit that file."
  say "  bash (~/.bashrc): $PATH_LINE"
  say "  zsh (~/.zshrc): $PATH_LINE"
  say "  fish (~/.config/fish/config.fish): fish_add_path $shown_dir"
}
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) if [ "$MODIFY_PATH" = 1 ] && [ "$CLOUD" = 0 ]; then
       added=$(add_to_profiles)
       if [ -n "$added" ]; then
         say "Added $BIN_DIR to PATH in$added; a new shell finds ljos. In this one, run:"
       else
         say "Your shell profiles already add $BIN_DIR to PATH. In this shell, run:"
       fi
     else
       shell_profiles
       say "In this shell, run:"
     fi
     say "  $PATH_LINE"
     if [ "$MODIFY_PATH" = 1 ] && [ "$CLOUD" = 0 ]; then
       say "  fish (~/.config/fish/config.fish): fish_add_path $shown_dir"
     fi
     PATH="$BIN_DIR:$PATH"; export PATH ;;
esac
if [ -n "$AHEAD" ] && [ -x "$BIN_DIR/ljos" ] && ! same_bin "$AHEAD" "$BIN_DIR/ljos"; then
  old=$(ver_of "$AHEAD")
  new=$(ver_of "$BIN_DIR/ljos")
  say "PATH finds $AHEAD${old:+ ($old)} before $BIN_DIR/ljos${new:+ ($new)}."
  if [ "$PATH_HAD_BIN" = 1 ]; then
    say "  This shell would run the older one. Put the new install first:"
    say "  $PATH_LINE"
  else
    say "  The line above puts $BIN_DIR first, ahead of $AHEAD."
  fi
  PATH="$BIN_DIR:$PATH"
  export PATH
  SHADOW=1
fi

# harness name, then how to tell it is here: a command, or a directory
# under $HOME.
RUNNERS="claude cmd:claude
codex cmd:codex
grok cmd:grok
cursor cmd:cursor-agent dir:.cursor
gemini cmd:gemini
copilot cmd:copilot dir:.copilot
qwen cmd:qwen
factory cmd:droid dir:.factory
crush cmd:crush
kiro cmd:kiro-cli dir:.kiro
opencode cmd:opencode
hermes cmd:hermes
omp cmd:omp
antigravity cmd:agy
windsurf dir:.codeium/windsurf
cline dir:Documents/Cline"

found_any=0
if [ "$ONBOARD" = 1 ]; then
  say "Connecting the coding runners found here:"
  printf '%s\n' "$RUNNERS" > "$TMP/runners"
  while read -r name probes; do
    here=0
    for probe in $probes; do
      case $probe in
        cmd:*) command -v "${probe#cmd:}" >/dev/null 2>&1 && here=1 ;;
        dir:*) [ -d "$HOME/${probe#dir:}" ] && here=1 ;;
      esac
    done
    [ "$here" = 1 ] || continue
    found_any=1
    if ljos onboard --harness "$name" > "$TMP/onboard-$name" 2>&1; then
      say "  $name: onboarded"
    else
      say "  $name: onboard reported a problem:"
      sed 's/^/    /' "$TMP/onboard-$name"
    fi
  done < "$TMP/runners"
  if [ "$found_any" = 0 ]; then
    say "  none found. Setting up the ticket tracker, the memory writer, and the deed store."
    say "  The deed store keeps signed records of the work."
    say "  No coding tool is installed, so no runner was registered."
    say "  After you install an agent, run \`ljos onboard\`."
    say "  \`ljos onboard --example\` lists those tools. \`ljos onboard --harness NAME\` registers the one you name."
    if ! ljos onboard --harness all > "$TMP/onboard-seat" 2>&1; then
      say "  seat setup reported a problem:"
      sed 's/^/    /' "$TMP/onboard-seat"
    fi
  fi
fi

# The first doctor looks for this directory. Create it here so that
# row does not warn on a machine that just installed.
state_root=${XDG_STATE_HOME:-$HOME/.local/state}
mkdir -p "$state_root/ljos"
chmod 700 "$state_root/ljos" 2>/dev/null || true

# The encoder fetches its model on first use. Fetch it now, while the
# network is known to work, so the first search is not lexical.
if [ -x "$BIN_DIR/packset-embed" ]; then
  if printf '%s\n' '{"id":"warm","text":"warm the encoder"}' | "$BIN_DIR/packset-embed" > /dev/null 2> "$TMP/embed.err"; then
    say "Encoder model ready."
  else
    say "  warning: packset-embed could not fetch its model ($(tail -n 1 "$TMP/embed.err"));"
    say "  search is lexical until it can. Run packset-embed by hand to see why."
  fi
fi

# A temporary HOME is a test sandbox to ljos: the guard and the memory
# writer stay off there. Say so, so a trial in /tmp does not mislead.
tmp_root=${TMPDIR:-/nonexistent}
case "$(cd "$HOME" && pwd -P)/" in
  /tmp/*|/var/tmp/*|/private/tmp/*|/dev/shm/*|"${tmp_root%/}"/*)
    say "Note: HOME is a temporary directory, so the command guard and memory stay off;"
    say "  \`ljos doctor\` says why. Install into a HOME outside /tmp to try them." ;;
esac
say "The guard lets a force push run, with a warning:"
# A push onto main also trips the sitting gate, which hides this warning.
# A topic branch shows the force-push rule on its own. Ask from the home
# directory, and ignore a git dir the caller exported.
(
  cd "$HOME" || exit 1
  unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_PREFIX \
    GIT_OBJECT_DIRECTORY GIT_COMMON_DIR GIT_NAMESPACE
  # The sample is a warning to read. It is not a command this person ran,
  # so it does not land in their bypass log. The token stays in the pack
  # home onboard made. Pointing the sample at the empty directory made
  # the search say it had no token.
  mkdir -p "$TMP/policy-demo"
  pack_home=${XDG_DATA_HOME:-$HOME/.local/share}/packset
  XDG_DATA_HOME="$TMP/policy-demo" PACKSET_HOME="$pack_home" \
    ljos policy -- git push --force origin topic
) || true
if [ "$CLOUD" = 1 ]; then
  if "$BIN_DIR/ljos" cloud up --help >/dev/null 2>&1; then
    "$BIN_DIR/ljos" cloud up
  else
    die "$BIN_DIR/ljos has no cloud up. Run scripts/install.sh from the ljos tree, or a release that includes it."
  fi
fi
if [ -n "$PREV_VER" ]; then
  now=$(ver_of "$BIN_DIR/ljos")
  old_n=$(ver_num "$PREV_VER")
  new_n=$(ver_num "$now")
  if [ -n "$old_n" ] && [ "$old_n" = "$new_n" ]; then
    say "ljos $new_n is already installed at $BIN_DIR. Reinstalled the same version."
  elif [ -n "$old_n" ] && [ -n "$new_n" ] && version_lt "$old_n" "$new_n"; then
    say "Updated ljos at $BIN_DIR from $old_n to $new_n."
  elif [ -n "$old_n" ] && [ -n "$new_n" ]; then
    say "Replaced ljos $old_n at $BIN_DIR with $new_n."
  else
    say "Replaced the ljos already at $BIN_DIR ($PREV_VER) with ${now:-the programs just installed}."
  fi
fi
live=
for pid in $MCP_PIDS; do
  [ -d "/proc/$pid" ] || continue
  live="$live $pid"
done
if [ -n "$live" ]; then
  say "ljos-mcp is still running (pid$live) and is the previous binary."
  say "  Restart the assistant, or run \`kill$live\`, then start it again."
fi
secs=$(( $(date +%s) - START ))
if [ "$PATH_HAD_BIN" = 0 ] || [ "$SHADOW" = 1 ]; then
  say "Done in $secs s. This shell does not run the ljos just installed until PATH is set."
  say "Next: $PATH_LINE && ljos doctor"
else
  say "Done in $secs s. Next: \`ljos doctor\`."
fi
say "Shell completion is one line in the shell's startup file. This script does not write that file."
say "  bash: source <(ljos completions bash)"
say "  zsh: source <(ljos completions zsh)"
say "  fish: ljos completions fish | source"
