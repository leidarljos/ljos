#!/bin/sh
# Install ljos from its release tarballs, check each tarball against the
# SHA-256 published beside it and its GitHub build attestation, and run
# `ljos onboard` for every coding agent found on this machine. With none
# found, the tracker, the memory writer and the deed store are still set up.
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
#   --attest        refuse a tarball whose GitHub build attestation is
#                   missing or does not verify
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
ONBOARD=1
OWNER=leidarljos
PINS=
ATTEST=0

say() { printf '%s\n' "$*"; }
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
    printf '%s\n' "$repo" >> "$TMP/copied"
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
  attested=$(attest "$TMP/$asset" "$repo" "$got")
  printf '%s\n' "$attested" >> "$TMP/attested"
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
  label=$repo
  case $crate in *-mcp) label=$crate ;; esac
  say "  $label $tag"
  [ "$attested" != failed ] || warn "$asset: its build attestation does not verify: $(head -n 1 "$TMP/attest.err" 2>/dev/null)"
}

for pin in $PINS; do
  printf '%s\n' "$COMPONENTS" | grep -q "^${pin%%=*} " ||
    die "--pin ${pin%%=*}: not one of the repositories this installs"
done

say "Installing ljos into $BIN_DIR ($TARGET)"
printf '%s\n' "$COMPONENTS" | while read -r repo crate style progs; do
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
  say "Connecting the coding agents found here:"
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
    say "  none found. Setting up the tracker, memory and deed store."
    say "  After you install an agent, run \`ljos onboard --harness NAME\`."
    if ! ljos onboard --harness all > "$TMP/onboard-seat" 2>&1; then
      say "  seat setup reported a problem:"
      sed 's/^/    /' "$TMP/onboard-seat"
    fi
  fi
fi

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
say "The guard asks before a force push:"
# A checkout this script was started in may carry a push gate. Ask from
# the home directory, and ignore a git dir the caller exported.
(
  cd "$HOME" || exit 1
  unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_PREFIX \
    GIT_OBJECT_DIRECTORY GIT_COMMON_DIR GIT_NAMESPACE
  ljos policy -- git push --force origin main
) || true
say "Done in $(( $(date +%s) - START )) s. Next: \`ljos doctor\`."
