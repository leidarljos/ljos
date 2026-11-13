#!/usr/bin/env python3
"""Write the command samples the docs commit, from this tree's ljos.

Doctor on a scratch seat, ``ljos --version``, the help excerpt and the
two-agents how-to are produced here. ``docs/build.sh`` runs this before
the export. CI runs it again and ``git diff --exit-code`` fails when a
committed sample differs.

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
import time
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
    ROOT / "docs/orgmode/howto-check.org",
    ROOT / "docs/source/howto-check.rst",
    ROOT / "docs/orgmode/reference-help.org",
    ROOT / "docs/source/reference-help.rst",
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
    """Usage, the heading over doctor, that command, and the options."""
    lines = text.splitlines()
    try:
        options_at = lines.index("Options:")
    except ValueError as exc:
        raise SystemExit("ljos --help has no Options section") from exc
    try:
        usage_at = next(i for i, line in enumerate(lines) if line.startswith("Usage:"))
    except StopIteration as exc:
        raise SystemExit("ljos --help has no Usage line") from exc
    head = lines[: usage_at + 1]
    doctor: list[str] = []
    heading: list[str] = []
    i = usage_at + 1
    while i < options_at:
        if lines[i].strip().startswith("doctor"):
            column = command_column(lines[i])
            if (
                i > 0
                and lines[i - 1].rstrip().endswith(":")
                and command_column(lines[i - 1]) is None
            ):
                heading = [lines[i - 1]]
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
    body = head + [""] + heading + doctor + [""] + lines[options_at:]
    return "\n".join(body).rstrip()


def org_src(body: str, lang: str = "text") -> str:
    return f"#+begin_src {lang}\n" + body.rstrip("\n") + "\n#+end_src\n"


def rst_src(body: str, lang: str = "text") -> str:
    # expandtabs, not a fixed run of spaces: a tab advances to the next stop.
    expanded = body.expandtabs(RST_TAB)
    indented = "\n".join("   " + line if line else "" for line in expanded.splitlines())
    # Blank lines keep the comment markers from joining the directive.
    return f"\n.. code:: {lang}\n\n" + indented + "\n\n"


def replace_region(text: str, begin: str, end: str, body: str) -> str:
    start = text.find(begin)
    stop = text.find(end)
    if start < 0 or stop < 0 or stop < start:
        raise SystemExit(f"missing sample markers {begin!r}")
    start += len(begin)
    if text[start:start + 1] == "\n":
        start += 1
    return text[:start] + body + text[stop:]


# Session ids are fixed, so the seat names are the same on every run.
# Ticket ids, message ids, review times and the home directory are not.
AGENT_C1 = "0199a1b2-claude-session-one"
AGENT_C2 = "0199a1b2-claude-session-two"
AGENT_THREAD = "019a7c3e-55aa-7b10-9c2d-3e4f5a6b7c8d"
AGENT_CODEX = "019a7c3e-codex-session-one"
AGENT_CURSOR = "0199c0de-cursor-conversation-one"
AGENT_SEAT_1 = "claude-f2pu9d56g0"
AGENT_SEAT_2 = "claude-dxw6sq4j7l"
AGENT_CURSOR_SEAT = "cursor-e6ghys7rpo"

_ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
_ISSUE_ID = re.compile(r"demo-[a-z0-9]{4}\b")
_MSG_ID = re.compile(r"\b[0-9a-f]{32}\b")
_SHORT_ID = re.compile(r"(?<=id )[0-9a-f]{8}\b")
_FENCE = re.compile(r"fence [0-9a-f]+")
_REVIEW = re.compile(r"next review \d{4}-\d{2}-\d{2} \d{2}:\d{2} [+-]\d{2}:\d{2}")
_SINCE = re.compile(r"(?:still running|its runner is gone), since [^,)]+")
_STAMP = re.compile(r"\t\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z")


def _need_tool(name: str) -> None:
    from shutil import which

    if which(name) is None:
        raise SystemExit(f"gen_samples: {name} is not on PATH")


def _pty_run(argv: list[str], env: dict[str, str], cwd: Path, timeout: float) -> tuple[int, str]:
    import fcntl
    import pty
    import select
    import struct
    import termios

    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    proc = subprocess.Popen(
        argv,
        stdin=subprocess.DEVNULL,
        stdout=slave,
        stderr=slave,
        env=env,
        cwd=cwd,
        close_fds=True,
    )
    os.close(slave)
    chunks: list[bytes] = []
    deadline = time.time() + timeout
    while True:
        left = deadline - time.time()
        if left <= 0:
            proc.kill()
            raise SystemExit(f"{' '.join(argv)} did not finish within {timeout}s")
        ready, _, _ = select.select([master], [], [], min(0.2, left))
        if ready:
            try:
                data = os.read(master, 8192)
            except OSError:
                break
            if not data:
                break
            chunks.append(data)
            continue
        if proc.poll() is not None:
            while True:
                ready, _, _ = select.select([master], [], [], 0.05)
                if not ready:
                    break
                try:
                    data = os.read(master, 8192)
                except OSError:
                    break
                if not data:
                    break
                chunks.append(data)
            break
    os.close(master)
    code = proc.wait(timeout=10)
    text = _ANSI.sub("", b"".join(chunks).decode("utf-8", "replace"))
    text = text.replace("\r\n", "\n").replace("\r", "")
    return code, text


def _agent_env(home: Path, ljos: Path, extra: dict[str, str] | None = None) -> dict[str, str]:
    path = os.environ.get("PATH", "/usr/bin:/bin")
    env = scratch_env(home, f"{ljos.parent}:{path}")
    env.update(
        {
            "USER": "ada",
            "LOGNAME": "ada",
            "TZ": "UTC",
            "LJOS_SYNC_HOST": "box",
            "GIT_CONFIG_GLOBAL": str(home / "gitconfig"),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_AUTHOR_NAME": "Ada Example",
            "GIT_AUTHOR_EMAIL": "ada@example.com",
            "GIT_COMMITTER_NAME": "Ada Example",
            "GIT_COMMITTER_EMAIL": "ada@example.com",
            "GIT_TERMINAL_PROMPT": "0",
            "COLUMNS": "80",
            "TERM": "dumb",
            # A distinct runtime directory, so a parent process's session
            # id is not read as this process's.
            "XDG_RUNTIME_DIR": str(home / "run"),
        }
    )
    for key in (
        "CLAUDE_CODE_SESSION_ID",
        "CODEX_THREAD_ID",
        "CODEX_SESSION_ID",
        "CURSOR_CONVERSATION_ID",
        "LJOS_SEAT",
        "CURSOR_AGENT",
        "CLAUDECODE",
    ):
        env.pop(key, None)
    if extra:
        env.update(extra)
    return env


def _show(argv: list[str], env: dict[str, str], cwd: Path, display: str, timeout: float = 60) -> str:
    code, text = _pty_run(argv, env, cwd, timeout)
    if code != 0 and not text.lstrip().startswith("label ") and not text.lstrip().startswith("sitting "):
        raise SystemExit(f"{display} exited {code}:\n{text}")
    body = text.rstrip("\n")
    return f"$ {display}\n{body}\n"


def _canon_agents(text: str, home: Path) -> str:
    text = text.replace(str(home), "/home/ada")
    seen: dict[str, str] = {}

    def issue(match: re.Match[str]) -> str:
        raw = match.group(0)
        if raw not in seen:
            seen[raw] = f"demo-{len(seen) + 1}"
        return seen[raw]

    text = _ISSUE_ID.sub(issue, text)
    text = _MSG_ID.sub("…", text)
    text = _SHORT_ID.sub("…", text)
    text = _FENCE.sub("fence …", text)
    text = _REVIEW.sub("next review …", text)
    text = _SINCE.sub("…", text)
    text = _STAMP.sub("\t…", text)
    return text


def _assert_agents(parts: dict[str, str]) -> None:
    whole = "\n".join(parts.values())
    for needle in (
        "CLAUDE_CODE_SESSION_ID in this process",
        "CODEX_THREAD_ID in this process",
        "CURSOR_CONVERSATION_ID in this process",
        f"sent … from {AGENT_SEAT_1} to {AGENT_SEAT_2}",
        f"from {AGENT_SEAT_1}",
        "this session has no id",
        "export LJOS_SEAT=NAME",
        "source\tLJOS_SEAT",
        f"not by {AGENT_SEAT_2}@",
        "/home/ada/",
    ):
        if needle not in whole:
            raise SystemExit(f"gen_samples: agent sample missing {needle!r}")
    memory = parts["memory"]
    # The other session of the same runner is one family: no from-line.
    family = [block for block in memory.split("$ ") if "claude-session-two" in block]
    if not family or f"from {AGENT_SEAT_1}" in family[0]:
        raise SystemExit("gen_samples: a second session of the runner was labelled as another seat")
    if "/tmp" in whole or "ljos-howto" in whole:
        raise SystemExit("gen_samples: agent sample still names a temporary directory")


def agent_samples(ljos: Path) -> dict[str, str]:
    """Console blocks for the two-agents how-to, from this binary.

    The seat names are a pure function of the session ids. Ticket ids,
    message ids, review times and the home directory are rewritten to
    stand-ins, the same way the doctor row rewrites the binary path.
    """
    for name in ("vissue", "claimdag", "packset", "packsetd"):
        _need_tool(name)
    home = Path(tempfile.mkdtemp(prefix="ljos-doc-agents-"))
    try:
        for rel in (
            ".config/ljos",
            ".local/state/ljos",
            ".cache",
            ".local/share",
            "run",
            "tmp",
            "work",
        ):
            (home / rel).mkdir(parents=True)
        subprocess.run(
            ["git", "config", "--file", str(home / "gitconfig"), "user.name", "Ada Example"],
            check=True,
        )
        subprocess.run(
            ["git", "config", "--file", str(home / "gitconfig"), "user.email", "ada@example.com"],
            check=True,
        )
        cwd = home / "work"
        base = _agent_env(home, ljos)
        c1 = {"CLAUDE_CODE_SESSION_ID": AGENT_C1}
        c2 = {"CLAUDE_CODE_SESSION_ID": AGENT_C2}
        cursor = {"CURSOR_CONVERSATION_ID": AGENT_CURSOR}

        def env_of(extra: dict[str, str] | None = None) -> dict[str, str]:
            merged = dict(base)
            if extra:
                merged.update(extra)
            return merged

        seats = "\n".join(
            [
                _show(["ljos", "seat"], env_of(), cwd, "ljos seat"),
                _show(
                    ["ljos", "seat"],
                    env_of(c1),
                    cwd,
                    f"CLAUDE_CODE_SESSION_ID={AGENT_C1} ljos seat",
                ),
                _show(
                    ["ljos", "seat"],
                    env_of(c2),
                    cwd,
                    f"CLAUDE_CODE_SESSION_ID={AGENT_C2} ljos seat",
                ),
                _show(
                    ["ljos", "seat"],
                    env_of({"CODEX_THREAD_ID": AGENT_THREAD}),
                    cwd,
                    f"CODEX_THREAD_ID={AGENT_THREAD} ljos seat",
                ),
                _show(
                    ["ljos", "seat"],
                    env_of({"CODEX_SESSION_ID": AGENT_CODEX}),
                    cwd,
                    f"CODEX_SESSION_ID={AGENT_CODEX} ljos seat",
                ),
                _show(
                    ["ljos", "seat"],
                    env_of({"CODEX_THREAD_ID": AGENT_THREAD, "CODEX_SESSION_ID": AGENT_CODEX}),
                    cwd,
                    f"CODEX_THREAD_ID={AGENT_THREAD} CODEX_SESSION_ID={AGENT_CODEX} ljos seat",
                ),
                _show(
                    ["ljos", "seat"],
                    env_of(cursor),
                    cwd,
                    f"CURSOR_CONVERSATION_ID={AGENT_CURSOR} ljos seat",
                ),
            ]
        )
        named = "\n".join(
            [
                _show(
                    ["ljos", "seat"],
                    env_of({"LJOS_SEAT": "ada", **c1}),
                    cwd,
                    f"LJOS_SEAT=ada CLAUDE_CODE_SESSION_ID={AGENT_C1} ljos seat",
                ),
                _show(
                    ["ljos", "--seat", "ada", "seat"],
                    env_of(c1),
                    cwd,
                    f"CLAUDE_CODE_SESSION_ID={AGENT_C1} ljos --seat ada seat",
                ),
            ]
        )
        code, text = _pty_run(["packset", "ensure"], base, cwd, 120)
        if code != 0:
            raise SystemExit(f"packset ensure exited {code}:\n{text}")
        code, text = _pty_run(["ljos", "onboard", "--harness", "all"], base, cwd, 120)
        if code != 0:
            raise SystemExit(f"ljos onboard exited {code}:\n{text}")
        lesson = "Measure the files before picking a codec."
        other = "Session two waits for the build to go green."
        memory = "\n".join(
            [
                _show(
                    ["ljos", "remember", lesson],
                    env_of(c1),
                    cwd,
                    f'CLAUDE_CODE_SESSION_ID={AGENT_C1} ljos remember "{lesson}"',
                ),
                _show(
                    ["ljos", "search", "measure the files"],
                    env_of(c2),
                    cwd,
                    f'CLAUDE_CODE_SESSION_ID={AGENT_C2} ljos search "measure the files"',
                ),
                _show(
                    ["ljos", "search", "measure the files"],
                    env_of(cursor),
                    cwd,
                    f'CURSOR_CONVERSATION_ID={AGENT_CURSOR} ljos search "measure the files"',
                ),
            ]
        )
        mail = "\n".join(
            [
                _show(
                    ["ljos", "remember", other],
                    env_of(c2),
                    cwd,
                    f'CLAUDE_CODE_SESSION_ID={AGENT_C2} ljos remember "{other}"',
                ),
                _show(
                    ["ljos", "send", AGENT_SEAT_2, "the build is red"],
                    env_of(c1),
                    cwd,
                    f'CLAUDE_CODE_SESSION_ID={AGENT_C1} ljos send {AGENT_SEAT_2} "the build is red"',
                ),
                _show(
                    ["ljos", "inbox"],
                    env_of(c2),
                    cwd,
                    f"CLAUDE_CODE_SESSION_ID={AGENT_C2} ljos inbox",
                ),
                _show(
                    ["ljos", "inbox"],
                    env_of(c1),
                    cwd,
                    f"CLAUDE_CODE_SESSION_ID={AGENT_C1} ljos inbox",
                ),
                _show(
                    ["ljos", "send", "claude", "the build is red"],
                    env_of(c1),
                    cwd,
                    f'CLAUDE_CODE_SESSION_ID={AGENT_C1} ljos send claude "the build is red"',
                ),
            ]
        )
        filed = _show(
            ["ljos", "file", "Measure the files", "-p", "demo"],
            env_of(c1),
            cwd,
            f'CLAUDE_CODE_SESSION_ID={AGENT_C1} ljos file "Measure the files" -p demo',
        )
        first = _ISSUE_ID.search(filed)
        if first is None:
            raise SystemExit(f"gen_samples: file printed no id:\n{filed}")
        one = first.group(0)
        claimed = _show(
            ["ljos", "claim", one],
            env_of(c1),
            cwd,
            f"CLAUDE_CODE_SESSION_ID={AGENT_C1} ljos claim {one}",
        )
        refused = _show(
            ["ljos", "claim", one],
            env_of(c2),
            cwd,
            f"CLAUDE_CODE_SESSION_ID={AGENT_C2} ljos claim {one}",
        )
        second_file = _show(
            ["ljos", "file", "Write the note", "-p", "demo"],
            env_of(c2),
            cwd,
            f'CLAUDE_CODE_SESSION_ID={AGENT_C2} ljos file "Write the note" -p demo',
        )
        other_id = _ISSUE_ID.search(second_file)
        if other_id is None:
            raise SystemExit(f"gen_samples: the second file printed no id:\n{second_file}")
        two = other_id.group(0)
        own_claim = _show(
            ["ljos", "claim", two],
            env_of(c2),
            cwd,
            f"CLAUDE_CODE_SESSION_ID={AGENT_C2} ljos claim {two}",
        )
        finished = _show(
            ["ljos", "finish", two],
            env_of(c2),
            cwd,
            f"CLAUDE_CODE_SESSION_ID={AGENT_C2} ljos finish {two}",
        )
        sitting = _show(
            ["ljos", "sitting", one],
            env_of(c2),
            cwd,
            f"CLAUDE_CODE_SESSION_ID={AGENT_C2} ljos sitting {one}",
        )
        tickets = "\n".join([filed, claimed, refused, second_file, own_claim, finished, sitting])
        recall = _show(
            ["ljos", "recall", one],
            env_of(c2),
            cwd,
            f"CLAUDE_CODE_SESSION_ID={AGENT_C2} ljos recall {one}",
        )
        side = Path(tempfile.mkdtemp(prefix="ljos-doc-noid-"))
        try:
            for rel in ("state/ljos", "config", "run", "tmp", "cache", "data"):
                (side / rel).mkdir(parents=True)
            # The last write on this machine is the bare runner name, and
            # this process has no session id. That is the case the notice
            # describes. The page says so; the log is not a command.
            (side / "state/ljos/seat-writes.log").write_text(
                "2026-11-13T00:00:00.000Z\tclaude\tbox\n",
                encoding="utf-8",
            )
            bare = _show(["ljos", "seat"], _agent_env(side, ljos), side, "ljos seat")
            fix_code, fix_text = _pty_run(
                ["ljos", "seat"],
                _agent_env(side, ljos, {"LJOS_SEAT": "ada"}),
                side,
                30,
            )
            if fix_code != 0:
                raise SystemExit(f"ljos seat with LJOS_SEAT exited {fix_code}:\n{fix_text}")
            noid = bare + "\n$ export LJOS_SEAT=ada\n$ ljos seat\n" + fix_text.rstrip("\n") + "\n"
        finally:
            import shutil

            shutil.rmtree(side, ignore_errors=True)
        parts = {
            "seats": seats,
            "named": named,
            "memory": memory,
            "mail": mail,
            "tickets": tickets,
            "recall": recall,
            "noid": noid,
        }
    finally:
        subprocess.run(
            ["packset", "stop"],
            cwd=home / "work",
            env=_agent_env(home, ljos),
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=30,
        )
        import shutil

        shutil.rmtree(home, ignore_errors=True)
    canon = {key: _canon_agents(value, home).rstrip() + "\n" for key, value in parts.items()}
    _assert_agents(canon)
    return canon


def rewritten(doctor: str, help_text: str, agents: dict[str, str]) -> dict[Path, str]:
    out: dict[Path, str] = {}
    regions = [
        (ROOT / "docs/orgmode/howto-check.org", "# BEGIN ljos-sample doctor", "# END ljos-sample doctor", org_src(doctor)),
        (ROOT / "docs/source/howto-check.rst", ".. BEGIN ljos-sample doctor", ".. END ljos-sample doctor", rst_src(doctor)),
        (ROOT / "docs/orgmode/reference-help.org", "# BEGIN ljos-sample help", "# END ljos-sample help", org_src(help_text)),
        (ROOT / "docs/source/reference-help.rst", ".. BEGIN ljos-sample help", ".. END ljos-sample help", rst_src(help_text)),
    ]
    for key, body in agents.items():
        regions.append(
            (
                ROOT / "docs/orgmode/howto-agents.org",
                f"# BEGIN ljos-sample agents-{key}",
                f"# END ljos-sample agents-{key}",
                org_src(body, "console"),
            )
        )
        regions.append(
            (
                ROOT / "docs/source/howto-agents.rst",
                f".. BEGIN ljos-sample agents-{key}",
                f".. END ljos-sample agents-{key}",
                rst_src(body, "console"),
            )
        )
    texts: dict[Path, str] = {}
    for path, begin, end, body in regions:
        current = texts.get(path)
        if current is None:
            current = path.read_text(encoding="utf-8")
        texts[path] = replace_region(current, begin, end, body)
    out.update(texts)
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
    files = rewritten(doctor, help_text, agent_samples(ljos))
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
