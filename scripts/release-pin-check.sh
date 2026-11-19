#!/bin/sh
# The sibling releases install.sh installs by default are the ones in
# .github/release-pin. This asks GitHub whether each archive install.sh
# would fetch for them, and its .sha256, is on the release page, for
# every target. A pin to a version that was tagged and never released,
# such as vissue 0.24.1, is a 404 here instead of a source build on a
# new machine.
#
#   scripts/release-pin-check.sh
#
# It needs network access and curl. The archive names come from
# `install.sh --print-assets`, so this checks what the script asks for.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
INSTALL=$ROOT/scripts/install.sh
PIN=$ROOT/.github/release-pin
OWNER=leidarljos

die() { printf 'release-pin-check: %s\n' "$*" >&2; exit 1; }

[ -f "$PIN" ] || die "no $PIN"
command -v curl >/dev/null 2>&1 || die "needs curl"

names=
while IFS= read -r line || [ -n "$line" ]; do
  trim=${line#"${line%%[![:space:]]*}"}
  case "$trim" in
    '' | \#*) continue ;;
  esac
  read -r name ver _extra <<EOF
$trim
EOF
  [ -n "${ver:-}" ] || die "$PIN: no version on the line for $name"
  names="$names $name"
done < "$PIN"
[ -n "$names" ] || die "$PIN names no repository"

bad=0
count=0
for target in aarch64-apple-darwin aarch64-unknown-linux-gnu x86_64-apple-darwin x86_64-unknown-linux-gnu; do
  assets=$(sh "$INSTALL" --print-assets --target "$target" \
    --pin ljos=0.0.0 --pin packset=0.0.0 --pin ljos-policyd=0.0.0 --pin consensus=0.0.0) ||
    die "install.sh --print-assets failed for $target"
  for name in $names; do
    asset=$(printf '%s\n' "$assets" | sed -n "s/^\\($name-v[0-9][0-9.]*-$target\\.tar\\.gz\\)\$/\\1/p" | head -n 1)
    [ -n "$asset" ] || die "install.sh asks for no $name archive for $target"
    ver=${asset#"$name"-v}
    ver=${ver%%-*}
    for file in "$asset" "$asset.sha256"; do
      url="https://github.com/$OWNER/$name/releases/download/v$ver/$file"
      count=$((count + 1))
      if ! curl -fsSIL --retry 3 --retry-delay 2 --max-time 30 -o /dev/null "$url" 2>/dev/null; then
        printf 'release-pin-check: no %s\n  %s\n' "$file" "$url" >&2
        bad=1
      fi
    done
  done
done

if [ "$bad" != 0 ]; then
  die "a pinned release has no archive. Pick a version from https://github.com/$OWNER/NAME/releases and edit .github/release-pin and the fallback list in scripts/install.sh."
fi
echo "release-pin-check: all $count pinned release files resolve"
