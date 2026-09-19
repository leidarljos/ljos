#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["cyclopts>=3"]
# ///
"""Time the seat's hook on a pack that grows to ten thousand memories.

A scratch pack writer is filled with distinct lessons, and at each size the
hook is timed on one prompt, three times, then on eight prompts at once;
`ljos search` and a dry `ljos consolidate` are timed beside it. The hook's
budget is a prompt, so the number that matters is the single hook at the
largest size. Nothing here calls a model: the writer ranks by words alone
unless an encoder is beside it.

    PACKSET_URL=http://127.0.0.1:8797 scripts/terra/hook_scale.py --sizes 1000 --sizes 5000 --sizes 10000
"""

from __future__ import annotations

import concurrent.futures
import json
import os
import random
import subprocess
import time
import urllib.request

import cyclopts

app = cyclopts.App(help=__doc__)

CONSONANTS = "bdfghjklmnprstvz"
VOWELS = "aeiou"
SYLLABLES = [c + v for c in CONSONANTS for v in VOWELS]


def word(r: random.Random) -> str:
    """A pronounceable pseudo-word from two or three syllables: a vocabulary
    of half a million, so two lessons share a token by accident about once
    in a thousand and the pack's overlap rule leaves every one live."""
    return "".join(r.choice(SYLLABLES) for _ in range(r.randint(2, 3)))


def lesson(i: int) -> str:
    """Distinct lessons: the case number opens the sentence and the rest is
    six pseudo-words, so neither the head rule nor the overlap rule pairs
    two of them and every write stays live."""
    r = random.Random(i)
    body = " ".join(word(r) for _ in range(6))
    return f"Case {i}: {body}."


def post(url: str, workspace: str, text: str) -> None:
    body = json.dumps({"schema": "inside.atom/v1", "kind": "lesson", "level": "explicit",
                       "text": text, "workspace": workspace}).encode()
    req = urllib.request.Request(f"{url}/v1/atoms", data=body, headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as r:
        r.read()


def live(url: str, workspace: str) -> int:
    with urllib.request.urlopen(f"{url}/v1/atoms?workspace={workspace}", timeout=60) as r:
        body = json.load(r)
    atoms = body.get("atoms", body if isinstance(body, list) else [])
    return len(atoms)


def timed(argv: list[str], stdin: str | None = None, timeout: float = 180) -> tuple[float, str]:
    t0 = time.perf_counter()
    try:
        p = subprocess.run(argv, input=stdin, capture_output=True, text=True, timeout=timeout)
        out = p.stdout if p.returncode == 0 else f"exit {p.returncode}: {p.stderr.strip()[-200:]}"
    except subprocess.TimeoutExpired:
        out = f"timed out after {timeout:.0f} s"
    return time.perf_counter() - t0, out


def hook(prompt: str, session: str) -> tuple[float, str]:
    call = json.dumps({"hook_event_name": "UserPromptSubmit", "session_id": session, "prompt": prompt})
    return timed(["ljos", "hook"], stdin=call)


@app.default
def main(
    *,
    url: str = os.environ.get("PACKSET_URL", "http://127.0.0.1:8761"),
    workspace: str = "seat",
    sizes: list[int] = [1000, 5000, 10000],
    parallel: int = 8,
):
    """Fill a scratch pack step by step and time the seat at each size.

    Parameters
    ----------
    url
        The pack writer; a scratch one, since this writes thousands of atoms.
    workspace
        The workspace written and read; the seat's is `seat`.
    sizes
        Atom counts to time at, ascending; each step adds the difference.
    parallel
        Prompts fired at once for the concurrent hook row.
    """
    have = 0
    for n in sizes:
        t0 = time.perf_counter()
        for i in range(have, n):
            post(url, workspace, lesson(i))
        have = n
        print(f"=== {n} atoms (filled in {time.perf_counter() - t0:.1f} s), live {live(url, workspace)}")
        prompts = [lesson(random.Random(1000 + k).randrange(n)).split(". ")[0] for k in range(parallel)]
        for k in range(3):
            secs, out = hook(prompts[k], f"scale-{n}-{k}")
            print(f"hook single {secs:.3f} s ({len(out)} bytes)")
        t0 = time.perf_counter()
        with concurrent.futures.ThreadPoolExecutor(max_workers=parallel) as pool:
            list(pool.map(lambda kp: hook(kp[1], f"scale-{n}-c{kp[0]}"), enumerate(prompts)))
        print(f"hook {parallel} concurrent {time.perf_counter() - t0:.3f} s")
        secs, _ = timed(["ljos", "search", prompts[0]])
        print(f"search {secs:.3f} s")
        secs, out = timed(["ljos", "consolidate"], timeout=300)
        tail = out.strip().splitlines()[-1] if out.strip() else ""
        print(f"consolidate dry {secs:.3f} s: {tail[:120]}")


if __name__ == "__main__":
    app()
