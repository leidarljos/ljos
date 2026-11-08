#!/usr/bin/env python3
"""Write the command samples the docs commit, from this tree's ljos.

Doctor on a scratch seat, ``ljos --version`` and the help excerpt are
produced here. ``docs/build.sh`` runs this before the export. CI runs it
again and ``git diff --exit-code`` fails when a committed sample differs.

The LongMemEval_S encoder named in the explanation is the one recorded in
``docs/scripts/packset-longmemeval-s.txt``, copied from the public packset
docs. The private bench repo is not read.

    python3 docs/scripts/gen_samples.py            rewrite the samples
    python3 docs/scripts/gen_samples.py --check    fail when they differ
    python3 docs/scripts/gen_samples.py --no-build skip the rewrite when no binary is built
"""

from __future__ import annotations

import argparse
import os
import re
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

import ljos_version  # noqa: E402

# Pandoc writes the committed rst with a tab stop of 4, which is what an org
# source tab becomes in the howto and reference samples checked in today.
RST_TAB = 4

# Doctor also asks the registry. ljos gives each lookup 3 seconds
# (`--max-time` 3) and does not start the pack under a temporary home, so
# this bound is past both and still fails a hang.
DOCTOR_BUDGET_S = 60

ENCODER_LINE = re.compile(r"CombMNZ \(BM25\+ plus ([A-Za-z0-9._-]+)\)")
VERSION_LINE = re.compile(r"ljos (\d+\.\d+\.\d+)\Z")
SAMPLE_FILES = (
    ROOT / "docs/orgmode/howto.org",
    ROOT / "docs/source/howto.rst",
    ROOT / "docs/orgmode/reference.org",
    ROOT / "docs/source/reference.rst",
)
LONGMEMEVAL_FILES = (
    ROOT / "docs/orgmode/explanation.org",
    ROOT / "docs/source/explanation.rst",
)


def longmemeval_encoder() -> str:
    text = (ROOT / "docs/scripts/packset-longmemeval-s.txt").read_text(encoding="utf-8")
    found = ENCODER_LINE.findall(text)
    if len(found) != 1:
        raise SystemExit("packset-longmemeval-s.txt must name one CombMNZ encoder")
    return found[0]


def check_longmemeval() -> None:
    """One LongMemEval paragraph names the packset encoder, and others may record the CPU run."""
    name = longmemeval_encoder()
    for path in LONGMEMEVAL_FILES:
        text = path.read_text(encoding="utf-8")
        paragraphs = re.split(r"\n\s*\n", text)
        hits = [p for p in paragraphs if "LongMemEval" in p]
        rel = path.relative_to(ROOT)
        if not hits:
            raise SystemExit(f"{rel}: no LongMemEval paragraph")
        named = [p for p in hits if name in p]
        if len(named) != 1:
            raise SystemExit(
                f"{rel}: expected one LongMemEval paragraph naming {name}, found {len(named)}"
            )
        for paragraph in hits:
            for wrong in ("multilingual-e5", "bge-small"):
                if wrong in paragraph:
                    raise SystemExit(f"{rel}: LongMemEval paragraph names {wrong}, not {name}")


def find_ljos(build: bool) -> Path | None:
    named = os.environ.get("LJOS_BIN")
    candidates = []
    if named:
        candidates.append(Path(named))
    candidates.append(ROOT / "target/debug/ljos")
    candidates.append(ROOT / "target/release/ljos")
    for path in candidates:
        if path.is_file() and os.access(path, os.X_OK):
            return path.resolve()
    if not build:
        return None
    subprocess.run(
        ["cargo", "build", "-p", "ljos", "--bin", "ljos"],
        cwd=ROOT,
        check=True,
    )
    built = ROOT / "target/debug/ljos"
    if not built.is_file():
        raise SystemExit("cargo build did not produce target/debug/ljos")
    return built.resolve()


def run(cmd: list[str], env: dict[str, str], budget: float | None = None) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            cmd,
            cwd=ROOT,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=budget,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        raise SystemExit(f"{' '.join(cmd)} did not finish within {budget}s") from exc


def scratch_env(home: Path, path: str) -> dict[str, str]:
    return {
        "HOME": str(home),
        "PATH": path,
        "TMPDIR": str(home / "tmp"),
        "XDG_CACHE_HOME": str(home / "cache"),
        "XDG_CONFIG_HOME": str(home / "config"),
        "XDG_DATA_HOME": str(home / "data"),
        "XDG_STATE_HOME": str(home / "state"),
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
    }


def samples_from(ljos: Path) -> tuple[str, str]:
    """Doctor row plus version, and the help excerpt, as plain text."""
    cargo = ljos_version.version()
    base = scratch_env(Path("/"), f"{ljos.parent}:/usr/bin:/bin")
    version = run([str(ljos), "--version"], base)
    if version.returncode != 0:
        raise SystemExit(f"ljos --version failed: {version.stderr.strip()}")
    line = version.stdout.strip()
    match = VERSION_LINE.fullmatch(line)
    if match is None:
        raise SystemExit(f"ljos --version printed {line!r}")
    if match.group(1) != cargo:
        raise SystemExit(
            f"ljos --version says {match.group(1)}, Cargo.toml says {cargo}; rebuild ljos"
        )

    with tempfile.TemporaryDirectory(prefix="ljos-doc-sample-") as tmp:
        home = Path(tmp)
        for name in ("tmp", "cache/ljos", "config", "data", "state", "bin", "stub"):
            (home / name).mkdir(parents=True)
        # The day cache holds this binary's version, so the row says
        # crates.io (cached) and does not follow a newer registry answer.
        (home / "cache/ljos/crate-ljos").write_text(cargo + "\n", encoding="utf-8")
        link = home / "bin/ljos"
        link.symlink_to(ljos)
        stub = home / "stub/curl"
        stub.write_text("#!/bin/sh\nexit 1\n", encoding="utf-8")
        stub.chmod(stub.stat().st_mode | stat.S_IEXEC)
        env = scratch_env(home, f"{home / 'stub'}:{home / 'bin'}")
        doctor = run([str(link), "doctor", "--all"], env, DOCTOR_BUDGET_S)
        row = ljos_row(doctor.stdout, link, ljos, cargo)
        if doctor.returncode not in (0, 1):
            raise SystemExit(
                f"ljos doctor --all exited {doctor.returncode}: {doctor.stderr.strip()}"
            )
        help_text = run([str(ljos), "--help"], base)
        doctor_help = run([str(ljos), "doctor", "--help"], base)
        if help_text.returncode != 0 or doctor_help.returncode != 0:
            raise SystemExit("ljos help failed")
        doctor_block = (
            "$ ljos --version\n"
            f"{line}\n"
            "$ ljos doctor --all\n"
            f"{row}\n"
        )
        help_block = (
            "$ ljos --version\n"
            f"{line}\n"
            "$ ljos --help\n"
            f"{help_excerpt(help_text.stdout)}\n"
            "$ ljos doctor --help\n"
            f"{doctor_help.stdout.strip()}\n"
        )
        return doctor_block, help_block


def ljos_row(stdout: str, link: Path, binary: Path, version: str) -> str:
    paths = [str(link), str(binary)]
    paths.sort(key=len, reverse=True)
    for raw in stdout.splitlines():
        parts = raw.split("\t")
        if len(parts) < 3 or parts[1] != "ljos":
            continue
        state = parts[2]
        for path in paths:
            state = state.replace(path, "/path/to/ljos")
        row = f"{parts[0]}\tljos\t{state}"
        if "behind" in state or "ahead of" in state:
            raise SystemExit(f"doctor ljos row is not the matching shape: {row!r}")
        expect = f"crates.io (cached) {version}"
        if expect not in state or f"/path/to/ljos  {version}" not in state:
            raise SystemExit(f"doctor ljos row: {row!r}")
        return row
    raise SystemExit(f"ljos doctor --all printed no ljos row:\n{stdout}")


def command_column(line: str) -> int | None:
    """Column of a command name. A wrapped description is indented further."""
    if not line.startswith(" ") or not line.strip():
        return None
    return len(line) - len(line.lstrip(" "))


def help_excerpt(text: str) -> str:
    """Usage, the doctor command, and the options. Not the whole command list."""
    lines = text.splitlines()
    try:
        commands_at = lines.index("Commands:")
        options_at = lines.index("Options:")
    except ValueError as exc:
        raise SystemExit("ljos --help has no Commands or Options section") from exc
    head = lines[:commands_at]
    doctor: list[str] = []
    i = commands_at + 1
    while i < options_at:
        if lines[i].strip().startswith("doctor"):
            column = command_column(lines[i])
            doctor.append(lines[i])
            i += 1
            while i < options_at:
                if lines[i].strip() == "" or command_column(lines[i]) == column:
                    break
                doctor.append(lines[i])
                i += 1
            break
        i += 1
    if not doctor:
        raise SystemExit("ljos --help has no doctor line")
    body = head + ["Commands:"] + doctor + [""] + lines[options_at:]
    return "\n".join(body).rstrip()


def org_src(body: str) -> str:
    return "#+begin_src text\n" + body.rstrip("\n") + "\n#+end_src\n"


def rst_src(body: str) -> str:
    # expandtabs, not a fixed run of spaces: a tab advances to the next stop.
    expanded = body.expandtabs(RST_TAB)
    indented = "\n".join("   " + line if line else "" for line in expanded.splitlines())
    # Blank lines keep the comment markers from joining the directive.
    return "\n.. code:: text\n\n" + indented + "\n\n"


def replace_region(text: str, begin: str, end: str, body: str) -> str:
    start = text.find(begin)
    stop = text.find(end)
    if start < 0 or stop < 0 or stop < start:
        raise SystemExit(f"missing sample markers {begin!r}")
    start += len(begin)
    if text[start:start + 1] == "\n":
        start += 1
    return text[:start] + body + text[stop:]


def rewritten(doctor: str, help_text: str) -> dict[Path, str]:
    doctor_org = org_src(doctor)
    doctor_rst = rst_src(doctor)
    help_org = org_src(help_text)
    help_rst = rst_src(help_text)
    out = {}
    mapping = {
        ROOT / "docs/orgmode/howto.org": ("# BEGIN ljos-sample doctor", "# END ljos-sample doctor", doctor_org),
        ROOT / "docs/source/howto.rst": (".. BEGIN ljos-sample doctor", ".. END ljos-sample doctor", doctor_rst),
        ROOT / "docs/orgmode/reference.org": ("# BEGIN ljos-sample help", "# END ljos-sample help", help_org),
        ROOT / "docs/source/reference.rst": (".. BEGIN ljos-sample help", ".. END ljos-sample help", help_rst),
    }
    for path, (begin, end, body) in mapping.items():
        out[path] = replace_region(path.read_text(encoding="utf-8"), begin, end, body)
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail when a committed sample differs")
    parser.add_argument("--no-build", action="store_true", help="do not build ljos when it is missing")
    args = parser.parse_args()
    sys.dont_write_bytecode = True
    check_longmemeval()
    ljos = find_ljos(build=not args.no_build)
    if ljos is None:
        print("gen_samples: no ljos binary; left the committed samples", file=sys.stderr)
        return 0
    doctor, help_text = samples_from(ljos)
    files = rewritten(doctor, help_text)
    changed = [path for path, text in files.items() if text != path.read_text(encoding="utf-8")]
    if args.check:
        if changed:
            for path in changed:
                print(f"gen_samples: {path.relative_to(ROOT)} differs; run python3 docs/scripts/gen_samples.py")
            return 1
        return 0
    for path in changed:
        path.write_text(files[path], encoding="utf-8")
        print(f"gen_samples: wrote {path.relative_to(ROOT)}")
    if not changed:
        print("gen_samples: samples match")
    return 0


if __name__ == "__main__":
    sys.exit(main())
