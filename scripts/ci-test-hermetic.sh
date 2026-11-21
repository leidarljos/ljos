#!/bin/sh
# Run a test command against clean non-/tmp HOME, TMPDIR, and XDG_RUNTIME_DIR
# scratch directories with XDG_*_HOME unset. Scan HOME, ljos-owned locations
# under the runner's real HOME (hashing regular files and walking pre-existing
# symlinks that resolve inside it), and suite-writable /tmp entries
# (/tmp/ljos-$UID and uid-owned ljos-*, .tmp*, tmp.*, each walked in full),
# ignoring concurrent foreign host activity.
set -eu

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <command...>" >&2
  exit 2
fi

real_home=${HOME-}
if [ -z "$real_home" ] || [ ! -d "$real_home" ]; then
  echo "runner real HOME ($real_home) is missing" >&2
  exit 1
fi

tmp_root=${LJOS_HERMETIC_TMP_ROOT:-/tmp}
target_dir=${CARGO_TARGET_DIR:-$PWD/target}
mkdir -p "$target_dir"
scan_dir=$(mktemp -d "$target_dir/hermetic-scan.XXXXXX")
suite_home="$scan_dir/home"
if [ -d "$tmp_root" ] && [ -w "$tmp_root" ]; then
  suite_tmp=$(mktemp -d "$tmp_root/ljos-hermetic-tmp.XXXXXX")
  suite_tmp_name=${suite_tmp#"$tmp_root"/}
else
  suite_tmp="$scan_dir/tmp"
  suite_tmp_name=""
  mkdir -p "$suite_tmp"
fi
suite_run="$scan_dir/run"
mkdir -p "$suite_home" "$suite_run"
chmod 700 "$suite_run"
trap 'chmod -R u+rwx "$scan_dir" "$suite_tmp" 2>/dev/null || true; rm -rf "$scan_dir" "$suite_tmp"' EXIT

for d in \
  "$real_home/.config/ljos" \
  "$real_home/.local/share/ljos" \
  "$real_home/.local/state/ljos" \
  "$real_home/Library/Application Support/ljos"
do
  if [ -e "$d" ] || [ -L "$d" ]; then
    echo "runner real home already contains $d before test run" >&2
    exit 1
  fi
done

cache_ljos="$real_home/.cache/ljos"
if [ -L "$cache_ljos" ] || { [ -e "$cache_ljos" ] && [ ! -d "$cache_ljos" ]; }; then
  echo "runner real home already contains $cache_ljos before test run" >&2
  exit 1
elif [ -d "$cache_ljos" ]; then
  if ! cache_other=$(find "$cache_ljos" -mindepth 1 -maxdepth 1 ! -name sibling-bin 2>/dev/null); then
    echo "runner real home failed to scan $cache_ljos before test run" >&2
    exit 1
  fi
  if [ -n "$cache_other" ] || [ ! -e "$cache_ljos/sibling-bin" ]; then
    echo "runner real home already contains $cache_ljos before test run" >&2
    exit 1
  fi
fi

cargo_home=${CARGO_HOME:-$real_home/.cargo}
rustup_home=${RUSTUP_HOME:-$real_home/.rustup}
cargo_skip_1="$cargo_home/.package-cache"
cargo_skip_2="$cargo_home/.package-cache-mutate"
cargo_skip_3="$cargo_home/.global-cache"
sibling_cache_skip="$real_home/.cache/ljos/sibling-bin"

scan_real_home() {
  out_file=$1
  err_file=$2
  PYTHONDONTWRITEBYTECODE=1 python3 - "$real_home" "$out_file" "$err_file" \
    "$scan_dir" "$target_dir" "$PWD/target" \
    "$cargo_skip_1" "$cargo_skip_2" "$cargo_skip_3" "$sibling_cache_skip" <<'PY'
import errno
import hashlib
import json
import os
import stat
import sys

root = os.path.abspath(sys.argv[1])
out_path = sys.argv[2]
err_path = sys.argv[3]
skip = set()
for raw in sys.argv[4:]:
    if raw:
        skip.add(os.path.abspath(raw))
        skip.add(os.path.realpath(raw))

entries = {}
errors = []

TOOL_NAMES = ("ljos", "vissue", "packset", "claimdag", "deedar", "phronesis")
XDG_PARENTS = (
    ".config",
    ".local/share",
    ".local/state",
    ".cache",
    "Library/Application Support",
)
STAT_ONLY_PREFIXES = (
    ".cargo/registry/",
    ".cargo/git/",
    ".rustup/toolchains/",
    ".rustup/downloads/",
    ".rustup/tmp/",
    ".rustup/update-hashes/",
)

def is_top_owned(name):
    if name in (".cargo", ".rustup"):
        return True
    if name.startswith(".ljos") or name.startswith("ljos-"):
        return True
    for tool in TOOL_NAMES:
        if name == f".{tool}" or name.startswith(f"{tool}-"):
            return True
    return False

def is_xdg_owned(name):
    if name in TOOL_NAMES:
        return True
    if name.startswith(".ljos"):
        return True
    for tool in TOOL_NAMES:
        if name.startswith(f"{tool}-"):
            return True
    return False

def is_skipped(path):
    return os.path.abspath(path) in skip or os.path.realpath(path) in skip

def should_hash(rel):
    norm = rel.replace(os.sep, "/")
    for pfx in STAT_ONLY_PREFIXES:
        if norm.startswith(pfx):
            return False
    return True

def hash_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as fp:
        for chunk in iter(lambda: fp.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()

try:
    root_st = os.stat(root)
except OSError as exc:
    errors.append(f"{root}: {exc.strerror}")
    root_st = None

root_real = os.path.realpath(root)
root_prefix = root_real.rstrip(os.sep) + os.sep

def inside_root(path):
    return os.path.realpath(path).startswith(root_prefix)

visited_dirs = set()
queue = []

def record_path(p, rel):
    if is_skipped(p):
        return
    try:
        st = os.lstat(p)
    except OSError as exc:
        errors.append(f"{p}: {exc.strerror}")
        return
    mode = stat.S_IMODE(st.st_mode)
    if stat.S_ISLNK(st.st_mode):
        target = None
        try:
            target = os.readlink(p)
        except OSError as exc:
            errors.append(f"{p}: {exc.strerror}")
        entries[rel] = ["l", mode, st.st_ino, st.st_size, st.st_mtime_ns, target]
        # A link to the root itself or out of it is recorded by its target
        # string only.
        if not inside_root(p):
            return

        target_key = rel + " -> target"
        try:
            tst = os.stat(p)
        except OSError as exc:
            if exc.errno in (errno.ENOENT, errno.ENOTDIR, errno.ELOOP):
                entries[target_key] = ["unresolved", exc.errno, None, None, None, None]
            else:
                errors.append(f"{p}: {exc.strerror}")
            return
        tmode = stat.S_IMODE(tst.st_mode)
        if stat.S_ISREG(tst.st_mode):
            extra = None
            if should_hash(rel):
                try:
                    extra = hash_file(p)
                except OSError as exc:
                    errors.append(f"{p}: {exc.strerror}")
            entries[target_key] = ["f", tmode, tst.st_ino, tst.st_size, tst.st_mtime_ns, extra]
        elif stat.S_ISDIR(tst.st_mode):
            entries[target_key] = ["d", tmode, tst.st_ino, tst.st_size, tst.st_mtime_ns, None]
            dir_key = (tst.st_dev, tst.st_ino)
            if dir_key not in visited_dirs:
                visited_dirs.add(dir_key)
                queue.append((p, rel))
        else:
            entries[target_key] = ["o", tmode, tst.st_ino, tst.st_size, tst.st_mtime_ns, None]
    elif stat.S_ISDIR(st.st_mode):
        entries[rel] = ["d", mode, st.st_ino, st.st_size, st.st_mtime_ns, None]
        dir_key = (st.st_dev, st.st_ino)
        if dir_key not in visited_dirs:
            visited_dirs.add(dir_key)
            queue.append((p, rel))
    elif stat.S_ISREG(st.st_mode):
        extra = None
        if should_hash(rel):
            try:
                extra = hash_file(p)
            except OSError as exc:
                errors.append(f"{p}: {exc.strerror}")
        entries[rel] = ["f", mode, st.st_ino, st.st_size, st.st_mtime_ns, extra]
    else:
        entries[rel] = ["o", mode, st.st_ino, st.st_size, st.st_mtime_ns, None]

if root_st is not None and stat.S_ISDIR(root_st.st_mode):
    visited_dirs.add((root_st.st_dev, root_st.st_ino))
    try:
        top_entries = sorted(os.scandir(root), key=lambda e: e.name)
    except OSError as exc:
        errors.append(f"{root}: {exc.strerror}")
        top_entries = []
    for entry in top_entries:
        if is_top_owned(entry.name):
            record_path(entry.path, entry.name)

    for parent_rel in XDG_PARENTS:
        parent_path = os.path.join(root, parent_rel)
        if is_skipped(parent_path):
            continue
        try:
            pst = os.lstat(parent_path)
        except OSError as exc:
            if exc.errno in (errno.ENOENT, errno.ENOTDIR):
                continue
            errors.append(f"{parent_path}: {exc.strerror}")
            continue
        if not stat.S_ISDIR(pst.st_mode) and not stat.S_ISLNK(pst.st_mode):
            continue
        try:
            sub_entries = sorted(os.scandir(parent_path), key=lambda e: e.name)
        except OSError as exc:
            if exc.errno in (errno.ENOENT, errno.ENOTDIR):
                continue
            errors.append(f"{parent_path}: {exc.strerror}")
            continue
        for entry in sub_entries:
            if is_xdg_owned(entry.name):
                record_path(entry.path, os.path.join(parent_rel, entry.name))

while queue:
    cur_dir, cur_rel = queue.pop()
    if is_skipped(cur_dir):
        continue
    try:
        it = sorted(os.scandir(cur_dir), key=lambda e: e.name)
    except OSError as exc:
        if exc.errno in (errno.EACCES, errno.EPERM):
            entries[cur_rel + " -> unreadable"] = ["unresolved", exc.errno, None, None, None, None]
        else:
            errors.append(f"{cur_dir}: {exc.strerror}")
        continue

    for entry in it:
        rel = os.path.join(cur_rel, entry.name) if cur_rel else entry.name
        record_path(entry.path, rel)

with open(out_path, "w", encoding="utf-8") as fp:
    json.dump(entries, fp, sort_keys=True)

with open(err_path, "w", encoding="utf-8") as fp:
    for line in errors:
        fp.write(line + "\n")

sys.exit(1 if errors else 0)
PY
}

diff_real_home() {
  before_file=$1
  after_file=$2
  PYTHONDONTWRITEBYTECODE=1 python3 - "$real_home" "$before_file" "$after_file" <<'PY'
import json
import os
import sys

root = os.path.abspath(sys.argv[1])
with open(sys.argv[2], encoding="utf-8") as fp:
    before = json.load(fp)
with open(sys.argv[3], encoding="utf-8") as fp:
    after = json.load(fp)

added = [k for k in sorted(after) if k not in before]
deleted = [k for k in sorted(before) if k not in after]
modified = []
labels = ("type", "mode", "inode", "size", "mtime", "content")
for k in sorted(before):
    if k not in after:
        continue
    b = before[k]
    a = after[k]
    if b != a:
        changed = [lbl for idx, lbl in enumerate(labels) if b[idx] != a[idx]]
        modified.append((k, changed))

if added:
    print(f"tests wrote new entries to runner real home ({root}):", file=sys.stderr)
    for k in added:
        print(os.path.join(root, k), file=sys.stderr)
if modified:
    print(f"tests modified entries in runner real home ({root}):", file=sys.stderr)
    for k, changed in modified:
        print(f"{os.path.join(root, k)} ({', '.join(changed)})", file=sys.stderr)
if deleted:
    print(f"tests deleted entries from runner real home ({root}):", file=sys.stderr)
    for k in deleted:
        print(os.path.join(root, k), file=sys.stderr)

sys.exit(1 if (added or modified or deleted) else 0)
PY
}

scan_tmp() {
  out_file=$1
  err_file=$2
  PYTHONDONTWRITEBYTECODE=1 python3 - "$tmp_root" "$suite_tmp_name" "$out_file" "$err_file" <<'PY'
import errno
import json
import os
import stat
import sys

tmp_root = sys.argv[1]
ignore_name = sys.argv[2]
out_path = sys.argv[3]
err_path = sys.argv[4]
entries = {}
errors = []

uid = os.geteuid()
rt_name = f"ljos-{uid}"
root_prefix = os.path.realpath(tmp_root).rstrip(os.sep) + os.sep
visited_dirs = set()
queue = []

def inside_root(path):
    return os.path.realpath(path).startswith(root_prefix)

def record_tmp(path, rel, walk_children):
    try:
        st = os.lstat(path)
    except OSError as exc:
        if exc.errno != errno.ENOENT:
            errors.append(f"{path}: {exc.strerror}")
        return
    mode = stat.S_IMODE(st.st_mode)
    if stat.S_ISLNK(st.st_mode):
        target = None
        try:
            target = os.readlink(path)
        except OSError as exc:
            errors.append(f"{path}: {exc.strerror}")
        entries[rel] = ["l", mode, st.st_uid, st.st_ino, st.st_size, st.st_mtime_ns, target]
        # A link to the root itself or out of it is recorded by its target
        # string only, so foreign trees are never walked.
        if not inside_root(path):
            return
        target_key = rel + " -> target"
        try:
            tst = os.stat(path)
        except OSError as exc:
            if exc.errno in (errno.ENOENT, errno.ENOTDIR, errno.ELOOP):
                entries[target_key] = ["unresolved", exc.errno, None, None, None, None, None]
            else:
                errors.append(f"{path}: {exc.strerror}")
            return
        tmode = stat.S_IMODE(tst.st_mode)
        if stat.S_ISDIR(tst.st_mode):
            entries[target_key] = ["d", tmode, tst.st_uid, tst.st_ino, tst.st_size, tst.st_mtime_ns, None]
            if walk_children:
                dir_key = (tst.st_dev, tst.st_ino)
                if dir_key not in visited_dirs:
                    visited_dirs.add(dir_key)
                    queue.append((path, rel))
        else:
            tkind = "f" if stat.S_ISREG(tst.st_mode) else "o"
            entries[target_key] = [tkind, tmode, tst.st_uid, tst.st_ino, tst.st_size, tst.st_mtime_ns, None]
    elif stat.S_ISDIR(st.st_mode):
        entries[rel] = ["d", mode, st.st_uid, st.st_ino, st.st_size, st.st_mtime_ns, None]
        if walk_children:
            dir_key = (st.st_dev, st.st_ino)
            if dir_key not in visited_dirs:
                visited_dirs.add(dir_key)
                queue.append((path, rel))
    elif stat.S_ISREG(st.st_mode):
        entries[rel] = ["f", mode, st.st_uid, st.st_ino, st.st_size, st.st_mtime_ns, None]
    else:
        entries[rel] = ["o", mode, st.st_uid, st.st_ino, st.st_size, st.st_mtime_ns, None]

if os.path.isdir(tmp_root):
    try:
        top_entries = sorted(os.scandir(tmp_root), key=lambda e: e.name)
    except OSError as exc:
        errors.append(f"{tmp_root}: {exc.strerror}")
        top_entries = []

    for entry in top_entries:
        name = entry.name
        if ignore_name and name == ignore_name:
            continue
        if name == rt_name:
            record_tmp(entry.path, name, True)
            continue
        if name.startswith("ljos-") or name.startswith(".tmp") or name.startswith("tmp."):
            try:
                st = os.lstat(entry.path)
            except OSError as exc:
                if exc.errno == errno.ENOENT:
                    continue
                errors.append(f"{entry.path}: {exc.strerror}")
                continue
            if st.st_uid == uid:
                # Another wrapper's live TMPDIR is checked by that run.
                record_tmp(entry.path, name, not name.startswith("ljos-hermetic-tmp."))

while queue:
    cur_dir, cur_rel = queue.pop()
    try:
        it = sorted(os.scandir(cur_dir), key=lambda e: e.name)
    except OSError as exc:
        if exc.errno in (errno.EACCES, errno.EPERM):
            entries[cur_rel + " -> unreadable"] = ["unresolved", exc.errno, None, None, None, None, None]
        elif exc.errno in (errno.ENOENT, errno.ENOTDIR):
            pass
        else:
            errors.append(f"{cur_dir}: {exc.strerror}")
        continue
    for entry in it:
        rel = os.path.join(cur_rel, entry.name)
        record_tmp(entry.path, rel, True)

with open(out_path, "w", encoding="utf-8") as fp:
    json.dump(entries, fp, sort_keys=True)

with open(err_path, "w", encoding="utf-8") as fp:
    for line in errors:
        fp.write(line + "\n")

sys.exit(1 if errors else 0)
PY
}

diff_tmp() {
  before_file=$1
  after_file=$2
  PYTHONDONTWRITEBYTECODE=1 python3 - "$tmp_root" "$before_file" "$after_file" <<'PY'
import json
import os
import sys

tmp_root = sys.argv[1]
with open(sys.argv[2], encoding="utf-8") as fp:
    before = json.load(fp)
with open(sys.argv[3], encoding="utf-8") as fp:
    after = json.load(fp)

added = [k for k in sorted(after) if k not in before]
modified = []
labels = ("type", "mode", "uid", "inode", "size", "mtime", "target")
for k in sorted(before):
    if k not in after:
        continue
    b = before[k]
    a = after[k]
    if b != a:
        changed = [lbl for idx, lbl in enumerate(labels) if b[idx] != a[idx]]
        modified.append((k, changed))

if added:
    print(f"tests left new entries in {tmp_root}:", file=sys.stderr)
    for k in added:
        print(os.path.join(tmp_root, k), file=sys.stderr)
if modified:
    print(f"tests modified or replaced entries in {tmp_root}:", file=sys.stderr)
    for k, changed in modified:
        print(f"{os.path.join(tmp_root, k)} ({', '.join(changed)})", file=sys.stderr)

sys.exit(1 if (added or modified) else 0)
PY
}

if ! scan_real_home "$scan_dir/before.json" "$scan_dir/before.err"; then
  echo "runner real home scan failed before test run:" >&2
  cat "$scan_dir/before.err" >&2
  exit 1
fi

if ! scan_tmp "$scan_dir/tmp_before.json" "$scan_dir/tmp_before.err"; then
  echo "/tmp scan failed before test run:" >&2
  cat "$scan_dir/tmp_before.err" >&2
  exit 1
fi

test_rc=0
env \
  -u LJOS_HERMETIC_TMP_ROOT \
  -u XDG_CONFIG_HOME \
  -u XDG_DATA_HOME \
  -u XDG_STATE_HOME \
  -u XDG_CACHE_HOME \
  HOME="$suite_home" \
  TMPDIR="$suite_tmp" \
  XDG_RUNTIME_DIR="$suite_run" \
  CARGO_HOME="$cargo_home" \
  RUSTUP_HOME="$rustup_home" \
  "$@" || test_rc=$?

scan_rc=0
suite_find_rc=0
find "$suite_home" -mindepth 1 >"$scan_dir/suite.out" 2>"$scan_dir/suite.err" || suite_find_rc=$?
if [ -s "$scan_dir/suite.out" ]; then
  echo "tests wrote to HOME ($suite_home):" >&2
  cat "$scan_dir/suite.out" >&2
  scan_rc=1
fi
if [ "$suite_find_rc" -ne 0 ]; then
  echo "scan of HOME ($suite_home) failed (find exited $suite_find_rc):" >&2
  cat "$scan_dir/suite.err" >&2
  scan_rc=1
fi

for pair in "TMPDIR:$suite_tmp" "XDG_RUNTIME_DIR:$suite_run"; do
  name=${pair%%:*}
  dir=${pair#*:}
  dir_find_rc=0
  find "$dir" -mindepth 1 >"$scan_dir/$name.out" 2>"$scan_dir/$name.err" || dir_find_rc=$?
  if [ "$dir_find_rc" -ne 0 ]; then
    echo "scan of $name ($dir) failed (find exited $dir_find_rc):" >&2
    cat "$scan_dir/$name.err" >&2
    scan_rc=1
  fi
done

tmp_scan_rc=0
scan_tmp "$scan_dir/tmp_after.json" "$scan_dir/tmp_after.err" || tmp_scan_rc=$?
if ! diff_tmp "$scan_dir/tmp_before.json" "$scan_dir/tmp_after.json"; then
  scan_rc=1
fi
if [ "$tmp_scan_rc" -ne 0 ]; then
  echo "scan of /tmp failed:" >&2
  cat "$scan_dir/tmp_after.err" >&2
  scan_rc=1
fi

for d in \
  "$real_home/.config/ljos" \
  "$real_home/.local/share/ljos" \
  "$real_home/.local/state/ljos" \
  "$real_home/Library/Application Support/ljos"
do
  if [ -e "$d" ] || [ -L "$d" ]; then
    echo "tests wrote to runner real home ($d):" >&2
    d_rc=0
    find "$d" >"$scan_dir/d.out" 2>"$scan_dir/d.err" || d_rc=$?
    if [ -s "$scan_dir/d.out" ]; then
      cat "$scan_dir/d.out" >&2
    fi
    if [ "$d_rc" -ne 0 ]; then
      echo "scan of $d failed (find exited $d_rc):" >&2
      cat "$scan_dir/d.err" >&2
    fi
    scan_rc=1
  fi
done

if [ -L "$cache_ljos" ] || { [ -e "$cache_ljos" ] && [ ! -d "$cache_ljos" ]; }; then
  echo "tests wrote to runner real home ($cache_ljos):" >&2
  scan_rc=1
elif [ -d "$cache_ljos" ]; then
  if ! cache_other=$(find "$cache_ljos" -mindepth 1 -maxdepth 1 ! -name sibling-bin 2>/dev/null); then
    echo "scan of $cache_ljos failed" >&2
    scan_rc=1
  elif [ -n "$cache_other" ] || [ ! -e "$cache_ljos/sibling-bin" ]; then
    echo "tests wrote to runner real home ($cache_ljos):" >&2
    printf '%s\n' "$cache_other" >&2
    scan_rc=1
  fi
fi

home_scan_rc=0
scan_real_home "$scan_dir/after.json" "$scan_dir/home.err" || home_scan_rc=$?
if ! diff_real_home "$scan_dir/before.json" "$scan_dir/after.json"; then
  scan_rc=1
fi
if [ "$home_scan_rc" -ne 0 ]; then
  echo "scan of runner real home ($real_home) failed:" >&2
  cat "$scan_dir/home.err" >&2
  scan_rc=1
fi

if [ "$test_rc" -ne 0 ]; then
  exit "$test_rc"
fi
exit "$scan_rc"
