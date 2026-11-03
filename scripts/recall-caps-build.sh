#!/bin/sh
# Build one release `ljos` per recall-cap variant.
#
# The three constants stay as they are in git. This script patches them for
# one build, copies the binary out, and restores the file before the next
# variant and on exit. A variant is named for the three values:
# pool, issue cap, related cap. The others stay at today's 4, 20 and 5.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
dest=${1:-/tmp/recall-bins}
lib=$root/crates/ljos-cli/src/lib.rs
saved=$(mktemp)
cp "$lib" "$saved"
restore() {
    cp "$saved" "$lib"
    rm -f "$saved"
}
trap restore EXIT INT TERM

mkdir -p "$dest"
cd "$root"

set_caps() {
    pool=$1
    issue=$2
    related=$3
    python3 - "$lib" "$pool" "$issue" "$related" <<'PY'
import pathlib, sys
path, pool, issue, related = sys.argv[1:]
file = pathlib.Path(path)
text = file.read_text()
replacements = [
    ("pub const POOL_PER_SHOWN: usize = 4;", f"pub const POOL_PER_SHOWN: usize = {pool};"),
    ("pub const ISSUE_MEMORY_CAP: usize = 20;", f"pub const ISSUE_MEMORY_CAP: usize = {issue};"),
    ("pub const RELATED_MEMORY_CAP: usize = 5;", f"pub const RELATED_MEMORY_CAP: usize = {related};"),
]
for old, new in replacements:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{old!r} occurs {count} times")
    text = text.replace(old, new, 1)
file.write_text(text)
PY
}

build_one() {
    pool=$1
    issue=$2
    related=$3
    name=ljos-p${pool}-i${issue}-r${related}
    cp "$saved" "$lib"
    set_caps "$pool" "$issue" "$related"
    cargo build --release --locked -p ljos --bin ljos
    install -m 0755 "$root/target/release/ljos" "$dest/$name"
    echo "built $name"
}

# Today's triple is built once. The other six move one constant.
build_one 4 20 5
build_one 2 20 5
build_one 8 20 5
build_one 4 20 3
build_one 4 20 8
build_one 4 10 5
build_one 4 40 5
cp "$saved" "$lib"
echo "restored $lib"
