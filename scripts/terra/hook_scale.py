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

    PACKSET_URL=http://127.0.0.1:8797 scripts/terra/hook_scale.py --sizes 1000 5000 10000
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

SUBJECTS = ["the review clock", "a stale claim", "the fused panel", "a scratch pack", "the encoder",
            "a handover bag", "the claim graph", "a deed accession", "the tracker root", "a persona",
            "the island walk", "a trust row", "the lexical ballot", "a due card", "the seat workspace",
            "a consolidation pass", "the hook budget", "a rewrite", "the argv law", "a lease"]
VERBS = ["closes", "outranks", "reopens", "refuses", "renews", "fires", "weighs", "caps", "seeds", "cites",
         "pins", "tombstones", "reschedules", "merges", "splits", "holds", "walks", "grades", "settles", "names"]
OBJECTS = ["the earlier reading", "a busy refusal", "the session node", "the store lock", "a second sitting",
           "the dense ballot", "an as-of read", "the map size", "a persona's brief", "the entity links",
           "a weak island", "the head rule", "a stamped session", "the login user", "a bucket of sixty-four",
           "the writer's port", "a forecast", "the calibration row", "an imported atom", "the last twelve events"]
WHEN = ["when the pack is cold", "after a handover", "on the second prompt", "under eight seats", "past ten thousand atoms",
        "before the encoder is up", "at the walltime", "when two runners collide", "on a date-only stamp", "after a lapse"]


def lesson(i: int) -> str:
    r = random.Random(i)
    return (f"{r.choice(SUBJECTS).capitalize()} {r.choice(VERBS)} {r.choice(OBJECTS)} {r.choice(WHEN)}. "
            f"Case {i} of the scale run says so.")


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
    sizes: tuple[int, ...] = (1000, 5000, 10000),
    concurrent: int = 8,
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
    concurrent
        Prompts fired at once for the concurrent hook row.
    """
    have = 0
    for n in sizes:
        t0 = time.perf_counter()
        for i in range(have, n):
            post(url, workspace, lesson(i))
        have = n
        print(f"=== {n} atoms (filled in {time.perf_counter() - t0:.1f} s), live {live(url, workspace)}")
        prompts = [lesson(random.Random(1000 + k).randrange(n)).split(". ")[0] for k in range(concurrent)]
        for k in range(3):
            secs, out = hook(prompts[k], f"scale-{n}-{k}")
            print(f"hook single {secs:.3f} s ({len(out)} bytes)")
        t0 = time.perf_counter()
        with concurrent.futures.ThreadPoolExecutor(max_workers=concurrent) as pool:
            list(pool.map(lambda kp: hook(kp[1], f"scale-{n}-c{kp[0]}"), enumerate(prompts)))
        print(f"hook {concurrent} concurrent {time.perf_counter() - t0:.3f} s")
        secs, _ = timed(["ljos", "search", prompts[0]])
        print(f"search {secs:.3f} s")
        secs, out = timed(["ljos", "consolidate"], timeout=300)
        tail = out.strip().splitlines()[-1] if out.strip() else ""
        print(f"consolidate dry {secs:.3f} s: {tail[:120]}")


if __name__ == "__main__":
    app()
