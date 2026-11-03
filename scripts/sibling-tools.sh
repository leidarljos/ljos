#!/usr/bin/env bash
# Build the pinned sibling tools the cargo test job runs, and install them
# into DEST.
#
#   scripts/sibling-tools.sh DEST
#
# .github/sibling-pin names vissue, claimdag and deedar. packset, phronesis
# and ljos-policyd are pinned below. Those three are not rows of the pin
# file: the tests parse that file as exactly vissue, claimdag and deedar,
# and these three are not built the same way. CI and a local run both
# call this script, so the commits live in one place.
#
# Each tool's first --version line has to be the whole pin. A substring
# such as 0.20 also matches 0.20.1. ljos-policyd is asked with `version`,
# and that line has to be the phronesis build, not the host table.
#
# Needs, when a binary is missing or wrong: git, cargo, rustc, gcc, g++,
# meson, ninja, pkg-config and capnp. age is not built here.
# A DEST that already holds every binary at its pin is checked and left
# as it is, so a restored CI cache does not compile again.
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
pin=$root/.github/sibling-pin

# The commit the cargo test job built for packset and packsetd.
packset_repo=https://github.com/leidarljos/packset.git
packset_sha=0fed66408b081a1749befac682304d92e7b29586
# workspace.package.version at that commit.
packset_version=0.17.3
# Eight hex digits. packset's build.rs stamps `git rev-parse --short=8 HEAD`
# into `NAME --version` on a clean tree, which for this commit is 0fed6640.
# PACKSET_COMMIT sets that stamp so a dirty tree does not append `+`.
packset_stamp=0fed6640

# phronesis keeps no release tags. This is the commit the cargo test job
# configured with meson and passed as PHRONESIS_DIR. Its VERSION file
# reads 0.2.9. ljos-policyd does not print that number.
phronesis_repo=https://github.com/leidarljos/phronesis.git
phronesis_sha=7d115de79407a54e0be5050f95107cfa08fe6277

# Cargo.toml version at this commit is 0.7.0. The tag of that name can
# move, so the commit is the pin. Linked to phronesis, `ljos-policyd
# version` prints `ljos-policyd 0.7.0 (phronesis)`.
policyd_repo=https://github.com/leidarljos/ljos-policyd.git
policyd_sha=233639176f497444068e7b92526fe8556528a9a9
policyd_version=0.7.0

usage() {
  printf '%s\n' "usage: scripts/sibling-tools.sh DEST"
}

# stdout is the first line. A pipe into head can SIGPIPE a tool that
# writes more, and pipefail would then fail a good pin.
tool_line() {
  local out
  out=$("$@") || return 1
  printf '%s' "${out%%$'\n'*}"
}

matches() {
  local bin=$1 want=$2
  shift 2
  local got
  [ -x "$bin" ] || return 1
  got=$(tool_line "$bin" "$@") || return 1
  [ "$got" = "$want" ]
}

expect_line() {
  local what=$1 bin=$2 want=$3
  shift 3
  local got
  if [ ! -x "$bin" ]; then
    printf 'sibling-tools: %s: %s is not executable\n' "$what" "$bin" >&2
    return 1
  fi
  if ! got=$(tool_line "$bin" "$@"); then
    printf 'sibling-tools: %s failed\n' "$what" >&2
    return 1
  fi
  if [ "$got" != "$want" ]; then
    printf "sibling-tools: %s printed '%s', pin says '%s'\n" "$what" "$got" "$want" >&2
    return 1
  fi
  printf '%s\n' "$got"
}

parse_pins() {
  names=()
  repos=()
  commits=()
  versions=()
  local line_no=0 line trim name repo commit version extra
  while IFS= read -r line || [ -n "$line" ]; do
    line_no=$((line_no + 1))
    trim=${line#"${line%%[![:space:]]*}"}
    trim=${trim%"${trim##*[![:space:]]}"}
    case "$trim" in
      '' | \#*) continue ;;
    esac
    extra=
    # Four fields. The version in this file has no spaces.
    read -r name repo commit version extra <<<"$trim"
    if [ -n "$extra" ] || [ -z "${version:-}" ]; then
      printf 'sibling-pin:%s: want name repository commit version\n' "$line_no" >&2
      exit 1
    fi
    case "$repo" in
      https://github.com/leidarljos/*.git) ;;
      *)
        printf 'sibling-pin:%s: %s repository is not a leidarljos git url\n' "$line_no" "$name" >&2
        exit 1
        ;;
    esac
    # git names these objects with SHA-1: 40 hexadecimal digits.
    if [[ ! $commit =~ ^[0-9a-f]{40}$ ]]; then
      printf 'sibling-pin:%s: %s commit is not a full SHA-1\n' "$line_no" "$name" >&2
      exit 1
    fi
    case "$name" in
      vissue | claimdag | deedar) ;;
      *)
        printf 'sibling-pin:%s: no package for %s\n' "$line_no" "$name" >&2
        exit 1
        ;;
    esac
    names+=("$name")
    repos+=("$repo")
    commits+=("$commit")
    versions+=("$version")
  done <"$pin"
}

already() {
  local i
  for i in "${!names[@]}"; do
    matches "$dest/${names[$i]}" "${names[$i]} ${versions[$i]}" --version || return 1
  done
  matches "$dest/packset" "packset ${packset_version} (${packset_stamp})" --version || return 1
  matches "$dest/packsetd" "packsetd ${packset_version} (${packset_stamp})" --version || return 1
  matches "$dest/ljos-policyd" "ljos-policyd ${policyd_version} (phronesis)" version || return 1
}

verify() {
  local i ok=0
  for i in "${!names[@]}"; do
    expect_line "${names[$i]} --version" "$dest/${names[$i]}" "${names[$i]} ${versions[$i]}" --version || ok=1
  done
  expect_line "packset --version" "$dest/packset" "packset ${packset_version} (${packset_stamp})" --version || ok=1
  expect_line "packsetd --version" "$dest/packsetd" "packsetd ${packset_version} (${packset_stamp})" --version || ok=1
  expect_line "ljos-policyd version" "$dest/ljos-policyd" "ljos-policyd ${policyd_version} (phronesis)" version || ok=1
  return "$ok"
}

need() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'sibling-tools: need %s on PATH (%s)\n' "$1" "$2" >&2
    exit 1
  }
}

require_tools() {
  need git "to fetch the pinned commits"
  need cargo "to build the Rust tools"
  need rustc "to build the Rust tools"
  need gcc "the g++ package installs it; phronesis is C"
  need g++ "package g++; phronesis's c-capnproto wrap is C++"
  need meson "package meson"
  need ninja "package ninja-build"
  need pkg-config "package pkg-config"
  need capnp "package capnproto"
}

fetch_commit() {
  local dir=$1 repo=$2 commit=$3
  rm -rf "$dir"
  git init -q "$dir"
  git -C "$dir" remote add origin "$repo"
  git -C "$dir" fetch --depth 1 origin "$commit"
  git -C "$dir" checkout --detach FETCH_HEAD
  test "$(git -C "$dir" rev-parse HEAD)" = "$commit"
}

build_siblings() {
  local i dir pkg
  for i in "${!names[@]}"; do
    case "${names[$i]}" in
      vissue) pkg=vissue-cli ;;
      claimdag) pkg=claimdag-cli ;;
      deedar) pkg=deedar-cli ;;
    esac
    dir=$work/src-${names[$i]}
    printf 'sibling-tools: building %s %s\n' "${names[$i]}" "${commits[$i]}"
    fetch_commit "$dir" "${repos[$i]}" "${commits[$i]}"
    cargo build --locked --manifest-path "$dir/Cargo.toml" -p "$pkg" --bin "${names[$i]}"
    install -m 0755 "$dir/target/debug/${names[$i]}" "$dest/${names[$i]}"
  done
}

build_packset() {
  local dir=$work/packset
  printf 'sibling-tools: building packset %s\n' "$packset_sha"
  fetch_commit "$dir" "$packset_repo" "$packset_sha"
  PACKSET_COMMIT=$packset_stamp \
    cargo build --locked --manifest-path "$dir/Cargo.toml" -p packset --bin packset --bin packsetd
  install -m 0755 "$dir/target/debug/packset" "$dest/packset"
  install -m 0755 "$dir/target/debug/packsetd" "$dest/packsetd"
}

build_policyd() {
  local phr=$work/phronesis pol=$work/ljos-policyd prefix=$work/phronesis-prefix
  printf 'sibling-tools: building phronesis %s\n' "$phronesis_sha"
  fetch_commit "$phr" "$phronesis_repo" "$phronesis_sha"
  # meson and the policyd C shim follow CC and CXX. The g++ package is
  # the compiler this script asks for. `c++` on PATH may be another one.
  (
    cd "$phr"
    CC=gcc CXX=g++ meson setup build --prefix="$prefix" -Dtests=disabled
    meson compile -C build
    meson install -C build
  )
  printf 'sibling-tools: building ljos-policyd %s\n' "$policyd_sha"
  fetch_commit "$pol" "$policyd_repo" "$policyd_sha"
  CC=gcc CXX=g++ PHRONESIS_DIR=$prefix \
    cargo build --locked --release --manifest-path "$pol/Cargo.toml"
  install -m 0755 "$pol/target/release/ljos-policyd" "$dest/ljos-policyd"
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi
if [ "$#" -ne 1 ] || [ -z "$1" ]; then
  usage >&2
  exit 2
fi
if [ ! -f "$pin" ]; then
  printf 'sibling-tools: no pin file at %s\n' "$pin" >&2
  exit 1
fi

mkdir -p "$1"
dest=$(CDPATH= cd -- "$1" && pwd)
export GIT_TERMINAL_PROMPT=0
parse_pins

if already; then
  verify
  exit 0
fi

require_tools
printf 'sibling-tools: building into %s\n' "$dest"
work=$(mktemp -d "${TMPDIR:-/tmp}/sibling-tools.XXXXXX")
trap 'rm -rf "$work"' EXIT
build_siblings
build_packset
build_policyd
verify
