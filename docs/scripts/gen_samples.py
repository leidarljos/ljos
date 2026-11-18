#!/usr/bin/env python3
"""Write the command samples the docs commit, from this tree's ljos.

Doctor on a scratch seat, ``ljos --version``, the help excerpt and the
two-agents how-to are produced here. ``docs/build.sh`` runs this before
the export. CI runs it again and ``git diff --exit-code`` fails when a
committed sample differs.

The LongMemEval_S encoder named in the explanation is the one recorded in
``docs/scripts/packset-longmemeval-s.txt``, copied from the public packset
docs. The private bench repo is not read.

The how-to samples start their own writer. It listens on a free port, never
the default 8761, and its pack home is the sample home's. A writer already
on 8761, owned by another home, is left alone.

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
        # A writer already on the default port belongs to another home.
        # Doctor on this scratch seat does not talk to it.
        env["PACKSET_URL"] = "off"
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


def free_loopback_port() -> int:
    """A free port on 127.0.0.1, never the default pack port."""
    import socket

    while True:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.bind(("127.0.0.1", 0))
            port = int(sock.getsockname()[1])
        if port != 8761:
            return port


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
    head = text.lstrip()
    refused = head.startswith(("label ", "sitting ", "claim:")) or "held by another" in head
    if code != 0 and not refused:
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
    pack_home = home / "pack"
    pack_port = free_loopback_port()
    pack = {
        "PACKSET_HOME": str(pack_home),
        "PACKSET_PORT": str(pack_port),
        "PACKSET_URL": f"http://127.0.0.1:{pack_port}",
    }
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
        pack_home.mkdir(parents=True)
        subprocess.run(
            ["git", "config", "--file", str(home / "gitconfig"), "user.name", "Ada Example"],
            check=True,
        )
        subprocess.run(
            ["git", "config", "--file", str(home / "gitconfig"), "user.email", "ada@example.com"],
            check=True,
        )
        cwd = home / "work"
        # One writer for every sample in this run, including each session
        # id. It is not the default port, so a stray writer on 8761 is
        # not this seat's.
        base = _agent_env(home, ljos, pack)
        code, text = _pty_run(["packset", "ensure"], base, cwd, 120)
        if code != 0:
            raise SystemExit(f"packset ensure exited {code}:\n{text}")
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
            bare = _show(
                ["ljos", "seat"],
                _agent_env(side, ljos, pack),
                side,
                "ljos seat",
            )
            fix_code, fix_text = _pty_run(
                ["ljos", "seat"],
                _agent_env(side, ljos, {**pack, "LJOS_SEAT": "ada"}),
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
            env=_agent_env(home, ljos, pack),
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


def _dangling_hash(line: str) -> bool:
    trim = line.strip()
    if trim.startswith("|"):
        trim = trim[1:].strip()
    if not trim.endswith("]") or trim.startswith("["):
        return False
    body = trim[:-1]
    return bool(body) and all(ch in "0123456789abcdef" for ch in body)


def tutorial_timeline(ljos: Path) -> str:
    """The getting-started note and timeline, from this binary.

    The ticket is filed and claimed first, which is where the page is
    when it reaches the note. Dates, clocks and the fence hash are
    stand-ins, so a later run prints the same block. The pack stays off,
    and no writer is started.
    """
    for name in ("vissue", "claimdag", "deedar", "git"):
        _need_tool(name)
    home = Path(tempfile.mkdtemp(prefix="ljos-doc-tutorial-"))
    try:
        for rel in (
            ".config/ljos",
            ".local/state/ljos",
            ".cache",
            ".local/share",
            "run",
            "tmp",
            "work",
            "bin",
        ):
            (home / rel).mkdir(parents=True)
        bind = home / "bin"
        bind.joinpath("ljos").symlink_to(ljos)
        from shutil import which

        for name in ("vissue", "claimdag", "deedar", "git"):
            found = which(name)
            if found:
                bind.joinpath(name).symlink_to(found)
        subprocess.run(
            ["git", "config", "--file", str(home / "gitconfig"), "user.name", "Ada Example"],
            check=True,
        )
        subprocess.run(
            ["git", "config", "--file", str(home / "gitconfig"), "user.email", "ada@example.com"],
            check=True,
        )
        env = _agent_env(home, ljos, {"PACKSET_URL": "off", "LJOS_SYNC_HOST": "laptop"})
        env["PATH"] = f"{bind}:/usr/bin:/bin"
        cwd = home / "work"
        code, text = _pty_run(["ljos", "onboard"], env, cwd, 120)
        if code != 0:
            raise SystemExit(f"ljos onboard exited {code}:\n{text}")
        alice = dict(env)
        alice["LJOS_SEAT"] = "alice"
        filed = _show(
            ["ljos", "file", "Pick a codec", "-p", "demo", "-t", "decision", "--options", "keep,drop"],
            alice,
            cwd,
            'LJOS_SEAT=alice ljos file "Pick a codec" -p demo -t decision --options keep,drop',
        )
        first = _ISSUE_ID.search(filed)
        if first is None:
            raise SystemExit(f"gen_samples: tutorial file printed no id:\n{filed}")
        one = first.group(0)
        _show(
            ["ljos", "claim", one, "--assignee", "alice"],
            alice,
            cwd,
            f"LJOS_SEAT=alice ljos claim {one} --assignee alice",
        )
        noted = _show(
            ["ljos", "note", one, "the codec is one file"],
            alice,
            cwd,
            f'LJOS_SEAT=alice ljos note {one} "the codec is one file"',
        )
        timed = _show(
            ["ljos", "timeline", one],
            env,
            cwd,
            f"ljos timeline {one}",
        )
        block = noted + timed
    finally:
        import shutil

        shutil.rmtree(home, ignore_errors=True)
    text = block.replace(str(home), "/home/you")
    text = text.replace(one, "demo-2f5j")
    text = _REVIEW.sub("next review 2026-11-12 21:40 +00:00", text)
    text = re.sub(r"\b[0-9a-f]{16}\b", "0123456789abcdef", text)
    text = re.sub(r"\d{4}-\d{2}-\d{2}", "2026-11-12", text)
    text = re.sub(r"\b\d{2}:\d{2}\b", "21:40", text)
    for line in text.splitlines():
        if _dangling_hash(line):
            raise SystemExit(f"gen_samples: timeline split the fence hash:\n{text}")
    for needle in (
        'LJOS_SEAT=alice ljos note demo-2f5j "the codec is one file"',
        "ljos timeline demo-2f5j",
        "note: [unverified 0123456789abcdef]",
        "[end 0123456789abcdef]",
        "the codec is one file",
        "Pick a codec",
    ):
        if needle not in text:
            raise SystemExit(f"gen_samples: timeline sample missing {needle!r}:\n{text}")
    if not any(line.strip().startswith("|") and "[end 0123456789abcdef]" in line for line in text.splitlines()):
        raise SystemExit(f"gen_samples: the fence closer is not on its own wrap line:\n{text}")
    if "/tmp" in text or "ljos-doc-tutorial" in text:
        raise SystemExit("gen_samples: timeline sample still names a temporary directory")
    _quiet_write(text, "timeline sample")
    return text.strip() + "\n"


def _quiet_write(block: str, where: str) -> None:
    """A landed commit, a landed push and a sealed log stay off the sample.

    A refusal may still name tracker git. A host directory and a segment
    file never appear.
    """
    if (
        "tracker git:" in block
        and "refused" not in block
        and "failed" not in block
        and "skipped" not in block
    ):
        raise SystemExit(f"gen_samples: {where} printed a commit that landed:\n{block}")
    for needle in (
        "not pushed (LJOS_TRACKER_GIT=commit)",
        "sync: sealed",
        "sync: committed",
        ".ljos/atoms/",
        "seg-",
    ):
        if needle in block:
            raise SystemExit(f"gen_samples: {where} printed {needle!r}:\n{block}")


_EPISODE = (
    "episode: the hook shows it when a prompt bears on it, "
    "and ljos graded promotes it to standing, so run `ljos due`."
)
_STANDING = "saved as a standing lesson; run `ljos search \"QUERY\"` to read it"


def as_terminal(text: str) -> str:
    """The wording a person's terminal prints for these commands.

    The script is not that terminal. A runner may be above it, and stdin
    is not a login session, so the binary stores the lesson as an agent's.
    The substitution is fixed, so the sample does not depend on where the
    script runs. The how-to for two agents keeps the runner wording.
    The tutorial-script sample is the same transcript with this
    substitution left off, so it shows `lesson, from an agent`.
    """
    text = text.replace(_EPISODE, _STANDING)
    text = text.replace("lesson, from an agent, today", "lesson, today")
    text = text.replace("lesson, the person's words, today", "lesson, today")
    text = text.replace("preference, from an agent, not checked, today", "preference, today")
    # The binary also says why this run stored an agent's lesson. A
    # person's terminal does not print that sentence.
    kept = [
        line
        for line in text.splitlines(keepends=True)
        if not line.startswith("stored from an agent because")
    ]
    return "".join(kept)


def tutorial_search(ljos: Path) -> dict[str, str]:
    """Remember, search and prefer as a terminal would print them.

    The encoder stays off PATH, so the ranking line is the lexical one.
    The writer listens on a free port and is stopped before return.
    """
    for name in ("packset", "packsetd"):
        _need_tool(name)
    home = Path(tempfile.mkdtemp(prefix="ljos-doc-search-"))
    pack_home = home / "pack"
    pack_port = free_loopback_port()
    pack = {
        "PACKSET_HOME": str(pack_home),
        "PACKSET_PORT": str(pack_port),
        "PACKSET_URL": f"http://127.0.0.1:{pack_port}",
        "LJOS_SYNC_HOST": "laptop",
    }
    try:
        for rel in (
            ".config/ljos",
            ".local/state/ljos",
            ".cache",
            ".local/share",
            "run",
            "tmp",
            "work",
            "bin",
        ):
            (home / rel).mkdir(parents=True)
        pack_home.mkdir()
        bind = home / "bin"
        bind.joinpath("ljos").symlink_to(ljos)
        from shutil import which

        for name in ("packset", "packsetd"):
            found = which(name)
            if found is None:
                raise SystemExit(f"gen_samples: {name} is not on PATH")
            bind.joinpath(name).symlink_to(found)
        env = _agent_env(home, ljos, pack)
        env["PATH"] = f"{bind}:/usr/bin:/bin"
        cwd = home / "work"
        code, text = _pty_run(["packset", "ensure"], env, cwd, 120)
        if code != 0:
            raise SystemExit(f"packset ensure exited {code}:\n{text}")
        lesson = "The lexical default is BM25+. It beat BM25 by two points on turns."
        remembered = _show(
            ["ljos", "remember", lesson],
            env,
            cwd,
            f'ljos remember "{lesson}"',
        )
        found = _show(
            ["ljos", "search", "lexical default"],
            env,
            cwd,
            'ljos search "lexical default"',
        )
        preference = "CombMNZ over RRF for fusing two ballots."
        preferred = _show(
            ["ljos", "prefer", preference],
            env,
            cwd,
            f'ljos prefer "{preference}"',
        )
        fused = _show(
            ["ljos", "search", "fusing"],
            env,
            cwd,
            "ljos search fusing",
        )
    finally:
        subprocess.run(
            ["packset", "stop"],
            cwd=home / "work",
            env=_agent_env(home, ljos, pack),
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=30,
        )
        import shutil

        shutil.rmtree(home, ignore_errors=True)

    def scrub(text: str, standin: str) -> str:
        text = text.replace(str(home), "/home/you")
        text = _REVIEW.sub("next review 2026-11-12 21:40 +00:00", text)
        text = _SHORT_ID.sub(standin, text)
        return text.strip() + "\n"

    def finish(text: str, standin: str) -> str:
        return scrub(as_terminal(text), standin)

    search = finish(remembered + found, "a6ae2751")
    prefer = finish(preferred + fused, "f8b725a9")
    script = scrub(remembered + found, "a6ae2751")
    index_lines = [
        line
        for line in search.splitlines()
        if not line.startswith("ljos: no seat")
        and not line.startswith("standing:")
        and not line.startswith("episode:")
    ]
    index_lines.append('$ ljos file "Pick a codec" -p demo')
    index = "\n".join(index_lines).strip() + "\n"
    for name, body, needle in (
        ("search", search, "lesson, today"),
        ("prefer", prefer, "preference, today"),
        ("index", index, "lesson, today"),
    ):
        if needle not in body or "from an agent" in body:
            raise SystemExit(f"gen_samples: {name} sample is not the terminal transcript:\n{body}")
        if "encoder not installed" not in body:
            raise SystemExit(f"gen_samples: {name} sample did not rank by words:\n{body}")
        if "/tmp" in body or "ljos-doc-search" in body:
            raise SystemExit(f"gen_samples: {name} sample names a temporary directory")
    if "this write uses ada" not in search:
        raise SystemExit(f"gen_samples: search sample did not use the sample login:\n{search}")
    if "unknown-" in search:
        raise SystemExit(f"gen_samples: search sample still uses an unknown host:\n{search}")
    if _STANDING not in search:
        raise SystemExit(f"gen_samples: search sample did not say the lesson stands:\n{search}")
    if "lesson, from an agent" not in script or "lesson, today" in script:
        raise SystemExit(
            f"gen_samples: script sample is not the no-terminal transcript:\n{script}"
        )
    return {"search": search, "prefer": prefer, "index": index, "script": script}


def _howto_seat(ljos: Path, prefix: str) -> tuple[Path, dict[str, str], Path]:
    """A scratch seat whose pack port is closed, so memory is down.

    curl is a stub, so a missing program's row does not take a crates.io
    version. The tools the sitting needs are linked under the seat.
    """
    for name in ("vissue", "claimdag", "deedar", "git", "bash", "packset", "packsetd"):
        _need_tool(name)
    home = Path(tempfile.mkdtemp(prefix=prefix))
    for rel in (
        ".config/ljos",
        ".local/state/ljos",
        ".cache",
        ".local/share",
        "run",
        "tmp",
        "work",
        "bin",
        "stub",
    ):
        (home / rel).mkdir(parents=True)
    bind = home / "bin"
    bind.joinpath("ljos").symlink_to(ljos)
    from shutil import which

    for name in ("vissue", "claimdag", "deedar", "git", "bash", "packset", "packsetd"):
        found = which(name)
        if found:
            bind.joinpath(name).symlink_to(found)
    stub = home / "stub/curl"
    stub.write_text("#!/bin/sh\nexit 1\n", encoding="utf-8")
    stub.chmod(stub.stat().st_mode | stat.S_IEXEC)
    subprocess.run(
        ["git", "config", "--file", str(home / "gitconfig"), "user.name", "Ada Example"],
        check=True,
    )
    subprocess.run(
        ["git", "config", "--file", str(home / "gitconfig"), "user.email", "ada@example.com"],
        check=True,
    )
    # off, not a closed port: onboard still finishes, and the sitting says
    # the pack is down.
    env = _agent_env(
        home,
        ljos,
        {
            "PACKSET_URL": "off",
            "LJOS_SYNC_HOST": "laptop",
            "LJOS_SEAT": "ada",
        },
    )
    env["PATH"] = f"{home / 'stub'}:{bind}:/usr/bin:/bin"
    return home, env, home / "work"


def _drop_home(home: Path) -> None:
    import shutil

    shutil.rmtree(home, ignore_errors=True)


_DAY_IDS = ("day-ab12", "day-cd34", "day-ef56")
_DAY_ID = re.compile(r"\bday-[a-z0-9]{4}\b")


def _stand_in_ids(text: str) -> str:
    seen: dict[str, str] = {}

    def issue(match: re.Match[str]) -> str:
        raw = match.group(0)
        if raw not in seen:
            if len(seen) >= len(_DAY_IDS):
                raise SystemExit(f"gen_samples: more plan ids than stand-ins:\n{text}")
            seen[raw] = _DAY_IDS[len(seen)]
        return seen[raw]

    return _DAY_ID.sub(issue, text)


def shell_hook_sample(ljos: Path) -> str:
    """The shell-only prompt line, from bash piping into this binary."""
    _need_tool("bash")
    home = Path(tempfile.mkdtemp(prefix="ljos-doc-hook-"))
    try:
        (home / "tmp").mkdir()
        env = _agent_env(home, ljos, {"PACKSET_URL": "off", "prompt": "hello"})
        env["PATH"] = f"{ljos.parent}:/usr/bin:/bin"
        code, text = _pty_run(
            ["bash", "-c", 'echo "$prompt" | ljos hook --prompt'],
            env,
            home,
            60,
        )
    finally:
        _drop_home(home)
    if code != 0:
        raise SystemExit(f"gen_samples: hook --prompt exited {code}:\n{text}")
    lines = text.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.startswith("label prompt-origin:"))
    except StopIteration as exc:
        raise SystemExit(f"gen_samples: hook printed no prompt-origin label:\n{text}") from exc
    label_lines = [lines[start]]
    for line in lines[start + 1 :]:
        if not line or line.startswith("{") or line.startswith("label "):
            break
        label_lines.append(line)
    label = "\n".join(label_lines)
    for needle in (
        "label prompt-origin:",
        "not stored as the person's words",
        "parent process is",
        "`bash`",
        "not a known runner",
    ):
        if needle not in label:
            raise SystemExit(f"gen_samples: hook label missing {needle!r}:\n{label}")
    if "/tmp" in label or "ljos-doc-hook" in label:
        raise SystemExit(f"gen_samples: hook label names a temporary directory:\n{label}")
    head = (
        "$ ljos onboard --harness example-host --skills ~/.example-host/skills\n"
        "ok\tskill\twrote /home/you/.example-host/skills/ljos/SKILL.md\n"
        "ok\tenv\twrote /home/you/.config/ljos/example-host.env; "
        "source /home/you/.config/ljos/example-host.env\n"
        "$ source ~/.config/ljos/example-host.env\n"
        '$ echo "$prompt" | ljos hook --prompt\n'
    )
    return f"{head}{label}\n$ ljos policy -- git push --force\n"


def calibrate_sample(ljos: Path) -> str:
    """What calibrate prints when ljos-consensus is not installed, then the settled lines."""
    home = Path(tempfile.mkdtemp(prefix="ljos-doc-calibrate-"))
    try:
        (home / "tmp").mkdir()
        env = _agent_env(home, ljos, {"PACKSET_URL": "off"})
        env["PATH"] = f"{ljos.parent}:/usr/bin:/bin"
        code, text = _pty_run(["ljos", "calibrate", "-p", "demo"], env, home, 30)
    finally:
        _drop_home(home)
    if code == 0:
        raise SystemExit(f"gen_samples: calibrate found ljos-consensus:\n{text}")
    line = text.strip()
    expect = "ljos-consensus is not on PATH. Install it with `cargo install --locked ljos-consensus`."
    if line != expect:
        raise SystemExit(f"gen_samples: calibrate missing-tool line was:\n{line}")
    return (
        "$ ljos calibrate -p demo\n"
        f"{line}\n"
        "$ ljos calibrate -p demo\n"
        "alice weighs bob at 0.917\n"
        "alice weighs carol at 0.643\n"
        "...\n"
    )


def sitting_plan_sample(ljos: Path) -> str:
    """`ljos sitting --plan` with the pack down, then finish and ready."""
    home, env, cwd = _howto_seat(ljos, "ljos-doc-sitting-")
    try:
        code, text = _pty_run(["ljos", "onboard"], env, cwd, 120)
        if code != 0:
            raise SystemExit(f"gen_samples: sitting onboard exited {code}:\n{text}")
        code, planned = _pty_run(
            [
                "ljos",
                "sitting",
                "--plan",
                "Ship the codec",
                "-p",
                "day",
                "--",
                "measure the files",
                "pick a default",
            ],
            env,
            cwd,
            120,
        )
        if code != 0:
            raise SystemExit(f"gen_samples: sitting --plan exited {code}:\n{planned}")
        ids = _DAY_ID.findall(planned)
        if len(ids) < 2:
            raise SystemExit(f"gen_samples: sitting --plan printed no ids:\n{planned}")
        child = ids[1]
        code, ticked = _pty_run(["ljos", "finish", child], env, cwd, 60)
        if code != 0:
            raise SystemExit(f"gen_samples: finish exited {code}:\n{ticked}")
        code, ready = _pty_run(["ljos", "ready", "--mine"], env, cwd, 60)
        if code != 0:
            raise SystemExit(f"gen_samples: ready exited {code}:\n{ready}")
        for number in range(19):
            code, filed = _pty_run(
                ["ljos", "file", f"queued work {number + 1:02}", "-p", "day", "--top"],
                env,
                cwd,
                60,
            )
            if code != 0:
                raise SystemExit(f"gen_samples: queued file exited {code}:\n{filed}")
        code, ready_page = _pty_run(["ljos", "ready"], env, cwd, 60)
        if code != 0:
            raise SystemExit(f"gen_samples: ready page exited {code}:\n{ready_page}")
    finally:
        _drop_home(home)
    page_lines = ready_page.strip().splitlines()
    if len(page_lines) < 5 or page_lines[0] != "ready  shown 20 of 21":
        raise SystemExit(f"gen_samples: ready page count changed:\n{ready_page}")
    hint = "See more with `--limit` or `--all`. Narrow with `-p` or `--mine`."
    if page_lines[-1] != hint:
        raise SystemExit(f"gen_samples: ready page hint changed:\n{ready_page}")
    page_excerpt = "\n".join([*page_lines[:4], "...", page_lines[-1]])
    block = (
        '$ ljos sitting --plan "Ship the codec" -p day -- "measure the files" "pick a default"\n'
        f"{planned.strip()}\n"
        f"$ ljos finish {child}\n"
        f"{ticked.strip()}\n"
        "$ ljos ready --mine\n"
        f"{ready.strip()}\n"
        "$ ljos ready\n"
        f"{page_excerpt}\n"
    )
    block = block.replace(str(home), "/home/ada")
    block = _stand_in_ids(block)
    for needle in (
        "day-ab12  Ship the codec",
        "day-cd34  measure the files",
        "day-ef56  pick a default",
        "next: day-cd34  measure the files",
        "These are word matches; the pack did not answer.",
        "matched by words (encoder not installed)",
        'day-ab12 is yours. `ljos finish day-ab12 --lesson "..."`',
        "ticked day-cd34",
        "next: day-ef56  pick a default",
        "ready  shown 20 of 21",
        hint,
    ):
        if needle not in block:
            raise SystemExit(f"gen_samples: sitting sample missing {needle!r}:\n{block}")
    _quiet_write(block, "sitting sample")
    if "/tmp" in block or "ljos-doc-sitting" in block:
        raise SystemExit(f"gen_samples: sitting sample names a temporary directory:\n{block}")
    return block


def receive_sample(ljos: Path) -> str:
    """`ljos receive` for a handover bag that cites no deeds."""
    home, env, cwd = _howto_seat(ljos, "ljos-doc-receive-")
    try:
        code, text = _pty_run(["ljos", "onboard"], env, cwd, 120)
        if code != 0:
            raise SystemExit(f"gen_samples: receive onboard exited {code}:\n{text}")
        code, filed = _pty_run(
            ["ljos", "file", "Ship the fuse change?", "-p", "proj"],
            env,
            cwd,
            60,
        )
        if code != 0:
            raise SystemExit(f"gen_samples: receive file exited {code}:\n{filed}")
        found = re.search(r"\bproj-[a-z0-9]{4}\b", filed)
        if found is None:
            raise SystemExit(f"gen_samples: receive file printed no id:\n{filed}")
        issue = found.group(0)
        bag = home / "bag"
        code, packed = _pty_run(
            ["ljos", "handover", "--out", str(bag), "--issue", issue],
            env,
            cwd,
            60,
        )
        if code != 0:
            raise SystemExit(f"gen_samples: handover exited {code}:\n{packed}")
        keep = home / "peer.head"
        code, got = _pty_run(
            ["ljos", "receive", str(bag), "--keep", str(keep)],
            env,
            cwd,
            60,
        )
        if code != 0:
            raise SystemExit(f"gen_samples: receive exited {code}:\n{got}")
    finally:
        _drop_home(home)
    body = got.strip()
    body = body.replace(str(bag), "/tmp/bag")
    body = body.replace(str(keep), "~/.local/share/ljos/peer.head")
    body = body.replace(str(home), "/home/ada")
    body = re.sub(r"\b[0-9a-f]{12,}\b", "0123456789abcdef", body)
    if "no deeds enclosed" not in body:
        raise SystemExit(f"gen_samples: receive sample is not an empty bag:\n{body}")
    if "/tmp/" in body.replace("/tmp/bag", "") or "ljos-doc-receive" in body:
        raise SystemExit(f"gen_samples: receive sample names a temporary directory:\n{body}")
    return (
        "$ ljos receive /tmp/bag --keep ~/.local/share/ljos/peer.head\n"
        f"{body}\n"
    )


_WALK_PAGES = {
    "shell-hook": "howto-runner",
    "calibrate": "howto-vote",
    "sitting-plan": "howto-sitting",
    "receive-empty": "howto-handover",
}


def rewritten(doctor: str, help_text: str, agents: dict[str, str], tutorials: dict[str, str], walks: dict[str, str]) -> dict[Path, str]:
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
    for key, body in tutorials.items():
        org_path = ROOT / "docs/orgmode/getting-started.org"
        rst_path = ROOT / "docs/source/getting-started.rst"
        if key == "index":
            org_path = ROOT / "docs/orgmode/index.org"
            rst_path = ROOT / "docs/source/index.rst"
        regions.append(
            (
                org_path,
                f"# BEGIN ljos-sample tutorial-{key}",
                f"# END ljos-sample tutorial-{key}",
                org_src(body, "console"),
            )
        )
        regions.append(
            (
                rst_path,
                f".. BEGIN ljos-sample tutorial-{key}",
                f".. END ljos-sample tutorial-{key}",
                rst_src(body, "console"),
            )
        )
    for key, page in _WALK_PAGES.items():
        body = walks[key]
        regions.append(
            (
                ROOT / "docs/orgmode" / f"{page}.org",
                f"# BEGIN ljos-sample {key}",
                f"# END ljos-sample {key}",
                org_src(body, "console"),
            )
        )
        regions.append(
            (
                ROOT / "docs/source" / f"{page}.rst",
                f".. BEGIN ljos-sample {key}",
                f".. END ljos-sample {key}",
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
    tutorials = {"timeline": tutorial_timeline(ljos)}
    tutorials.update(tutorial_search(ljos))
    walks = {
        "shell-hook": shell_hook_sample(ljos),
        "calibrate": calibrate_sample(ljos),
        "sitting-plan": sitting_plan_sample(ljos),
        "receive-empty": receive_sample(ljos),
    }
    files = rewritten(doctor, help_text, agent_samples(ljos), tutorials, walks)
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
