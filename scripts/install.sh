#!/bin/sh
# Install the ljos seat from its release tarballs, check each tarball
# against the SHA-256 published beside it, and run `ljos onboard` for
# every coding agent found on this machine.
#
#   curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh
#
# Each ljos release attaches this script as the asset install.sh, from
# the first release after 0.28.0 on. Before that release the URL is a 404.
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
#   --no-onboard    install only; onboard nothing
#   --version VER   install ljos VER (such as 0.29.0) instead of the latest
#   --pin REPO=VER  install REPO at VER (such as packset=0.14.0); repeat
#                   for each repository to hold
#   --attest        refuse a tarball whose GitHub build attestation does
#                   not verify (needs the gh CLI, signed in)
#   --help          this text
#
# The checksum guards against a broken or truncated download. It comes
# from the same release page as the tarball, so it does not prove who
# built the tarball. With the gh CLI on PATH the script also checks each
# tarball's build attestation, which says the release workflow of that
# repository built it, and says when none verified (a release from before
# attestations, or gh not signed in). --attest makes that stop the
# install.
#
# A pinned version that has no release for this platform is built from
# that version's crate, never from a newer one.
set -eu

BIN_DIR=${LJOS_BIN_DIR:-$HOME/.local/bin}
WITH_EMBED=1
FROM_DIR=
ONBOARD=1
OWNER=leidarljos
PINS=
ATTEST=0

say() { printf '%s\n' "$*"; }
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
    --no-onboard) ONBOARD=0; shift ;;
    --version) [ $# -ge 2 ] || die "--version needs a version"; PINS="$PINS ljos=$2"; shift 2 ;;
    --version=*) PINS="$PINS ljos=${1#--version=}"; shift ;;
    --pin) [ $# -ge 2 ] || die "--pin needs REPO=VER"; PINS="$PINS $2"; shift 2 ;;
    --pin=*) PINS="$PINS ${1#--pin=}"; shift ;;
    --attest) ATTEST=1; shift ;;
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

for pin in $PINS; do
  ver=${pin#*=}
  ver=${ver#v}
  case $pin in
    *=*) ;;
    *) die "--pin takes REPO=VER, not $pin" ;;
  esac
  printf '%s' "$ver" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' ||
    die "${pin%%=*}: $ver is not a version such as 0.29.0"
done

START=$(date +%s)

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL -o "$2" "$1"; }
  latest_url() { curl -fsSLI -o /dev/null -w '%{url_effective}' "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q -O "$2" "$1"; }
  latest_url() { wget -q --max-redirect=5 -S --spider "$1" 2>&1 | sed -n 's/^ *[Ll]ocation: *//p' | tail -n 1; }
else
  [ -n "$FROM_DIR" ] || die "need curl or wget"
fi

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  [ -n "$FROM_DIR" ] || die "need sha256sum or shasum to check the downloads"
fi

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) TARGET=x86_64-unknown-linux-gnu ;;
  Linux-aarch64|Linux-arm64) TARGET=aarch64-unknown-linux-gnu ;;
  Darwin-x86_64) TARGET=x86_64-apple-darwin ;;
  Darwin-arm64) TARGET=aarch64-apple-darwin ;;
  *) TARGET=unknown ;;
esac

TMP=$(mktemp -d "${TMPDIR:-/tmp}/ljos-install.XXXXXX")
trap 'rm -rf "$TMP"' EXIT INT TERM
mkdir -p "$BIN_DIR"

# repository, crate, tarball style, programs
COMPONENTS="ljos ljos v ljos,ljos-mcp
packset packset v packset,packsetd,packset-mcp
vissue vissue-cli dist vissue
vissue vissue-mcp dist vissue-mcp
deedar deedar-cli v deedar
claimdag claimdag-cli v claimdag
ljos-policyd ljos-policyd v ljos-policyd
consensus ljos-consensus v ljos-consensus"

source_build() {
  crate=$1 ver=$2
  command -v cargo >/dev/null 2>&1 ||
    die "no release build of $crate for $TARGET, and no cargo to build it; install Rust from https://rustup.rs"
  if [ -n "$ver" ]; then
    say "  building $crate $ver from source (no release build for $TARGET)"
    cargo install --locked --quiet --root "$TMP/cargo" --version "=$ver" "$crate" >&2
  else
    say "  building $crate from source (no release build for $TARGET)"
    cargo install --locked --quiet --root "$TMP/cargo" "$crate" >&2
  fi
  cp "$TMP"/cargo/bin/* "$BIN_DIR"/
}

# Check FILE's build attestation from REPO's release workflow with the gh
# CLI. Prints what it found; with --attest, anything but a pass stops.
attest() {
  file=$1 repo=$2
  if ! command -v gh >/dev/null 2>&1; then
    [ "$ATTEST" = 0 ] || die "--attest needs the gh CLI (https://cli.github.com)"
    printf 'no attestation check (gh not installed)'
    return
  fi
  if gh attestation verify "$file" --repo "$OWNER/$repo" >/dev/null 2>"$TMP/attest.err"; then
    printf 'attestation verified'
    return
  fi
  [ "$ATTEST" = 0 ] || die "$(basename "$file"): no build attestation from $OWNER/$repo verified: $(head -n 1 "$TMP/attest.err")"
  printf 'no attestation verified'
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
      cp "$FROM_DIR/$p" "$BIN_DIR/$p"
    done
    say "  $repo: copied from $FROM_DIR"
    return
  fi
  ver=$(pinned "$repo")
  ver=${ver#v}
  if [ -n "$ver" ]; then
    tag=v$ver
  else
    tag=$(latest_url "https://github.com/$OWNER/$repo/releases/latest")
    tag=${tag##*/}
    case $tag in v*) ;; *) die "could not read the latest $repo release" ;; esac
  fi
  case $style in
    v) asset="$repo-$tag-$TARGET.tar.gz" ;;
    dist) asset="$crate-$TARGET.tar.xz" ;;
  esac
  url="https://github.com/$OWNER/$repo/releases/download/$tag/$asset"
  if [ "$TARGET" = unknown ] || ! fetch "$url" "$TMP/$asset" 2>/dev/null; then
    source_build "$crate" "$ver"
    case ",$progs," in
      *,packset-embed,*) [ -x "$BIN_DIR/packset-embed" ] || warn_embed "a source build" ;;
    esac
    return
  fi
  fetch "$url.sha256" "$TMP/$asset.sha256" || die "$asset has no published checksum"
  want=$(sed 's/[ *].*//' "$TMP/$asset.sha256" | head -n 1)
  got=$(sha256 "$TMP/$asset")
  [ "$want" = "$got" ] || die "$asset: checksum mismatch (published $want, downloaded $got); nothing from it was installed"
  attested=$(attest "$TMP/$asset" "$repo")
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
    cp "$found" "$BIN_DIR/$p"
    chmod 755 "$BIN_DIR/$p"
  done
  say "  $repo $tag: checksum ok, $attested"
}

for pin in $PINS; do
  printf '%s\n' "$COMPONENTS" | grep -q "^${pin%%=*} " ||
    die "--pin ${pin%%=*}: not one of the repositories this installs"
done

say "Installing the ljos seat into $BIN_DIR ($TARGET)"
printf '%s\n' "$COMPONENTS" | while read -r repo crate style progs; do
  if [ "$repo" = packset ] && [ "$WITH_EMBED" = 1 ]; then
    progs="$progs,packset-embed"
  fi
  install_one "$repo" "$crate" "$style" "$progs"
done

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say "Note: $BIN_DIR is not on PATH; add it to your shell profile."
     PATH="$BIN_DIR:$PATH"; export PATH ;;
esac

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
  say "Onboarding the coding agents found here"
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
    say "  none found. Run \`ljos onboard --harness NAME\` once one is installed,"
    say "  or \`ljos onboard --harness json\` for the entry to paste anywhere."
  fi
fi

say "Checking a command the guard refuses:"
ljos policy -- git push --force origin main || true
say "Done in $(( $(date +%s) - START )) s. \`ljos doctor\` lists what each part says."
