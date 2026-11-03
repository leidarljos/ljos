#!/usr/bin/env python3
"""Measure the three recall caps against a local packsetd.

The pack is the committed fixture (hand-labelled relevant ids per issue).
One factor moves at a time. Variants run round-robin, six passes each.
Every wall time is perf_counter_ns around one `ljos recall ISSUE`. The
summary's median and quartiles are statistics.median and
statistics.quantiles(method="inclusive") on those rows, written out by
this process. Nothing in the table is typed by hand.

Decision, applied to those medians and written into the summary:

  F1 is 2*p*r/(p+r) on a pass's micro precision and micro recall, and 0
  when the pass showed nothing. Micro precision is the relevant hit lines
  shown, summed over the fixture's issues, divided by the hit lines shown.
  Micro recall divides the same numerator by the fixture's labelled
  relevant count, which does not depend on the cap.

  For one factor, the level with the greatest median F1 is the one the
  caps keep. An exact tie (the same median F1) goes to the smaller median
  pass wall time. If those two wall medians have overlapping inclusive
  quartiles, today's level wins when it is in the tie, otherwise the
  smaller constant. A sitting that misses a labelled memory loses more
  than one that shows an extra unlabelled line, and F1 charges both; time
  breaks a tie because a slower cap that shows the same hits is waste.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import statistics
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

# Subject words are disjoint, four letters or more, and not cue stop words,
# so a memory that states one issue's subject does not name another's.
# Counts are large enough that each cap truncates on some issues and binds
# on none for others: some issues have more than 40 own memories, some
# fewer than 10; some have more than 8 related hits, some fewer than 3.
ISSUES = [
    # suffix, title, own A, B, C, off, related A, B, C
    ("kiln", "Kiln damper schedule", 6, 8, 4, 28, 3, 4, 3),
    ("valve", "Valve gasket torque", 4, 3, 2, 14, 1, 1, 1),
    ("loom", "Loom shuttle tension", 2, 2, 1, 1, 5, 4, 3),
    ("ink", "Pigment viscosity ledger", 2, 1, 1, 39, 0, 2, 2),
    ("salt", "Salt crust thickness", 5, 6, 2, 2, 2, 2, 3),
    ("reed", "Reed flutter margin", 2, 1, 0, 1, 1, 0, 0),
    ("wax", "Beeswax sealant temperature", 8, 7, 3, 9, 4, 3, 2),
    ("yarn", "Yarn twist density", 3, 4, 2, 3, 2, 1, 1),
]

# Background lessons share no subject word with any issue. They are not
# labelled relevant to any cue.
BACKGROUND = [
    ("quartz", "copper"),
    ("birch", "amber"),
    ("granite", "linen"),
    ("marble", "cobalt"),
    ("cedar", "bronze"),
    ("ivory", "slate"),
]

# Post order is this seed, so a later run stores the same bytes in the
# same order. It is not tuned to a cap.
POST_SEED = 20261103
N_PASSES = 6
TODAY = {"pool": 4, "issue": 20, "related": 5}

# factor, level, binary name. The binary names match recall-caps-build.sh.
VARIANTS = [
    ("pool", 2, "ljos-p2-i20-r5"),
    ("pool", 4, "ljos-p4-i20-r5"),
    ("pool", 8, "ljos-p8-i20-r5"),
    ("related", 3, "ljos-p4-i20-r3"),
    ("related", 5, "ljos-p4-i20-r5"),
    ("related", 8, "ljos-p4-i20-r8"),
    ("issue", 10, "ljos-p4-i10-r5"),
    ("issue", 20, "ljos-p4-i20-r5"),
    ("issue", 40, "ljos-p4-i40-r5"),
]


def content_words(text: str) -> set[str]:
    """A stand-in for the cue's content words: four letters, not a stop.

    The runner does not import ljos. This check only guards the fixture
    against a shared subject word. The recall itself is what measures.
    """
    stops = {
        "about", "after", "also", "anything", "because", "been", "before",
        "being", "both", "change", "code", "could", "does", "doing", "each",
        "error", "everything", "from", "have", "having", "into", "issue",
        "just", "like", "making", "memory", "more", "most", "need", "nothing",
        "only", "other", "over", "please", "process", "really", "recall",
        "same", "should", "some", "something", "still", "such", "support",
        "task", "test", "than", "that", "their", "them", "then", "there",
        "these", "they", "this", "those", "through", "update", "using",
        "very", "want", "were", "what", "when", "where", "which", "while",
        "will", "with", "work", "would", "your", "that",
    }
    words = []
    cur = []
    for ch in text.lower():
        if ch.isalnum():
            cur.append(ch)
        else:
            if cur:
                words.append("".join(cur))
                cur = []
    if cur:
        words.append("".join(cur))
    return {w for w in words if len(w) >= 4 and w not in stops}


def unique_word(index: int) -> str:
    """Five letters, disjoint from every subject word.

    Three of these open each memory. The pack treats a later lesson as a
    rewrite when the token Jaccard reaches 0.6 or the two open on the same
    words, so the opener has to differ and the shared frame has to stay a
    minority of the tokens.
    """
    letters = []
    value = index
    for _ in range(4):
        letters.append(chr(ord("a") + (value % 26)))
        value //= 26
    return "z" + "".join(letters)


def jaccard(left: set[str], right: set[str]) -> float:
    if not left and not right:
        return 1.0
    return len(left & right) / len(left | right)


def pack_tokens(text: str) -> set[str]:
    words = []
    cur = []
    for ch in text.lower():
        if ch.isascii() and ch.isalnum():
            cur.append(ch)
        else:
            if cur:
                words.append("".join(cur))
                cur = []
    if cur:
        words.append("".join(cur))
    return set(words)


def head_words(text: str) -> list[str]:
    words = []
    cur = []
    for ch in text.lower():
        if ch.isalnum():
            cur.append(ch)
        else:
            if cur:
                words.append("".join(cur))
                cur = []
    if cur:
        words.append("".join(cur))
    return words


def build_fixture() -> dict:
    memories = []
    n = 0
    word_n = 0

    def token() -> str:
        nonlocal n
        n += 1
        return f"m{n:04d}"

    def opener() -> str:
        nonlocal word_n
        words = [unique_word(word_n + i) for i in range(3)]
        word_n += 3
        return " ".join(words)

    issues = []
    for suffix, title, oa, ob, oc, off, ra, rb, rc in ISSUES:
        issue = f"ufuv-{suffix}"
        words = title.split()
        if len(words) != 3:
            raise SystemExit(f"{issue} title needs three words: {title}")
        w1, w2, w3 = (w.lower() for w in words)
        issues.append({"id": issue, "title": title, "words": [w1, w2, w3]})

        def add(role: str, tier: str, text: str) -> None:
            mark = token()
            # The mark is the id a recall line is matched on. It is not a
            # subject word, so it does not change who names the subject.
            # The opener is three words used by no other memory, so the pack
            # keeps the row instead of retiring it as a rewrite.
            body = text.format(open=opener(), w1=w1, w2=w2, w3=w3, mark=mark)
            if body.count(" ") + 1 >= 12:
                raise SystemExit(f"sentence has 12 words or more: {body}")
            relevant = [issue] if tier in ("A", "B") else []
            memories.append(
                {
                    "id": mark,
                    "text": body,
                    "issue": issue if role == "own" else None,
                    "role": role,
                    "tier": tier,
                    "about": issue,
                    "relevant_to": relevant,
                }
            )

        for _ in range(oa):
            add("own", "A", "{open} holds the {w1} {w2} {w3} mark {mark}.")
        for _ in range(ob):
            add("own", "B", "{open} holds the {w1} {w2} note mark {mark}.")
        for _ in range(oc):
            add("own", "C", "{open} holds the workshop {w3} mark {mark}.")
        for _ in range(off):
            add("own", "off", "{open} holds the morning briefing mark {mark}.")
        for _ in range(ra):
            add("related", "A", "{open} holds the {w1} {w2} {w3} mark {mark}.")
        for _ in range(rb):
            add("related", "B", "{open} holds the {w1} {w2} note mark {mark}.")
        for _ in range(rc):
            add("related", "C", "{open} holds the workshop {w3} mark {mark}.")

    for i in range(24):
        a, b = BACKGROUND[i % len(BACKGROUND)]
        mark = token()
        text = f"{opener()} holds the {a} {b} mark {mark}."
        if text.count(" ") + 1 >= 12:
            raise SystemExit(f"sentence has 12 words or more: {text}")
        memories.append(
            {
                "id": mark,
                "text": text,
                "issue": None,
                "role": "background",
                "tier": "background",
                "about": None,
                "relevant_to": [],
            }
        )

    # Guard the labels. Two issues must not share a subject word, and a
    # labelled memory must carry two or three of its issue's words.
    seen: dict[str, str] = {}
    for issue in issues:
        for word in issue["words"]:
            if word in seen:
                raise SystemExit(f"{word} is in {seen[word]} and {issue['id']}")
            seen[word] = issue["id"]
    by_id = {m["id"]: m for m in memories}
    if len(by_id) != len(memories):
        raise SystemExit("duplicate memory id")
    for issue in issues:
        want = set(issue["words"])
        for mem in memories:
            have = content_words(mem["text"])
            shared = have & want
            labelled = issue["id"] in mem["relevant_to"]
            if mem["about"] != issue["id"] and mem["role"] != "background":
                if shared:
                    raise SystemExit(f"{mem['id']} leaks {shared} into {issue['id']}")
            if mem["about"] == issue["id"]:
                if mem["tier"] == "A" and shared != want:
                    raise SystemExit(f"{mem['id']} tier A shares {shared}")
                if mem["tier"] == "B" and len(shared) != 2:
                    raise SystemExit(f"{mem['id']} tier B shares {shared}")
                if mem["tier"] == "C" and len(shared) != 1:
                    raise SystemExit(f"{mem['id']} tier C shares {shared}")
                if mem["tier"] == "off" and shared:
                    raise SystemExit(f"{mem['id']} off-topic shares {shared}")
                if labelled != (mem["tier"] in ("A", "B")):
                    raise SystemExit(f"{mem['id']} label does not match its tier")
            elif labelled:
                raise SystemExit(f"{mem['id']} labelled for an issue it is not about")

    # The pack retires a lesson as a rewrite of another with the same issue
    # field when the token Jaccard reaches 0.6, or when they open on the
    # same three words for 60% of the shorter text. Either would hide rows
    # from recall, so the fixture refuses both.
    by_issue: dict[str, list[dict]] = {}
    for mem in memories:
        by_issue.setdefault(mem["issue"] or "", []).append(mem)
    worst = 0.0
    for group in by_issue.values():
        shapes = [(pack_tokens(m["text"]), head_words(m["text"]), m["id"]) for m in group]
        for i, (tok_a, head_a, id_a) in enumerate(shapes):
            for tok_b, head_b, id_b in shapes[i + 1 :]:
                score = jaccard(tok_a, tok_b)
                worst = max(worst, score)
                if score >= 0.6:
                    raise SystemExit(f"{id_a} and {id_b} jaccard {score}")
                shared = 0
                for left, right in zip(head_a, head_b):
                    if left != right:
                        break
                    shared += 1
                shorter = min(len(head_a), len(head_b))
                if (
                    shared >= 3
                    and shared < len(head_a)
                    and shared < len(head_b)
                    and shared >= 0.6 * shorter
                ):
                    raise SystemExit(f"{id_a} and {id_b} share a head of {shared}")

    order = list(range(len(memories)))
    random.Random(POST_SEED).shuffle(order)
    for posted, index in enumerate(order):
        memories[index]["post"] = posted
    memories.sort(key=lambda m: m["post"])
    return {
        "seed": POST_SEED,
        "label": (
            "A memory is relevant to an issue when its tier is A or B: "
            "the sentence states two or three of that issue's title words. "
            "Tier C names one title word. Off-topic own memories and the "
            "background name none. The mark in the sentence is the id."
        ),
        "issues": issues,
        "memories": memories,
    }


def org_text(fixture: dict) -> str:
    lines = [
        "#+TITLE: ufuv issues",
        "#+VISSUE: 1",
        "#+CATEGORY: ufuv",
        "#+TODO: TODO STARTED BLOCKED | DONE CANCELLED",
        "",
    ]
    for issue in fixture["issues"]:
        lines.append(f"* TODO [#B] {issue['title']}")
        lines.append(":PROPERTIES:")
        lines.append(f":ID:         {issue['id']}")
        lines.append(":CREATED:    [2026-06-01 Mon]")
        lines.append(":END:")
        lines.append("")
    return "\n".join(lines)


def write_tracker(fixture: dict, root: Path) -> None:
    path = root / "Software" / "ufuv" / "issues.org"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(org_text(fixture), encoding="utf-8")


def listen_port(pid: int) -> int | None:
    """The port packsetd bound. Its log prints `:0` for `--port 0`."""
    inodes = set()
    fd_dir = Path(f"/proc/{pid}/fd")
    if not fd_dir.is_dir():
        return None
    for entry in fd_dir.iterdir():
        try:
            target = os.readlink(entry)
        except OSError:
            continue
        if target.startswith("socket:[") and target.endswith("]"):
            inodes.add(target[len("socket:[") : -1])
    for table in ("/proc/net/tcp", "/proc/net/tcp6"):
        try:
            lines = Path(table).read_text(encoding="utf-8").splitlines()
        except OSError:
            continue
        for line in lines[1:]:
            cols = line.split()
            if len(cols) < 10 or cols[3] != "0A":
                continue
            if cols[9] not in inodes:
                continue
            port = int(cols[1].rsplit(":", 1)[1], 16)
            if port:
                return port
    return None


def start_packsetd(bin_path: Path, home: Path) -> tuple[subprocess.Popen, str]:
    home.mkdir(parents=True, exist_ok=True)
    log_path = home / "packsetd.log"
    log = open(log_path, "w", encoding="utf-8")
    proc = subprocess.Popen(
        [str(bin_path), "--port", "0", "--home", str(home)],
        env={
            **os.environ,
            "HOME": str(home),
            "PACKSET_HOME": str(home),
        },
        stdout=subprocess.DEVNULL,
        stderr=log,
    )
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            log.flush()
            raise SystemExit(f"packsetd exited {proc.returncode}: {log_path.read_text()}")
        port = listen_port(proc.pid)
        if port:
            url = f"http://127.0.0.1:{port}"
            if health(url):
                log.close()
                return proc, url
        time.sleep(0.05)
    log.flush()
    raise SystemExit(f"packsetd did not answer: {log_path.read_text()}")


def health(url: str) -> bool:
    try:
        with urllib.request.urlopen(url + "/health", timeout=1) as resp:
            return 200 <= resp.status < 300
    except (urllib.error.URLError, TimeoutError, OSError):
        return False


def post_memories(url: str, token: str, fixture: dict) -> None:
    for mem in fixture["memories"]:
        # One minute apart, in post order, so a tie on the words has a
        # stable age to break it. The stamp is the memory's valid_from.
        minute = mem["post"]
        hour, minute = divmod(minute, 60)
        stamp = f"2026-06-01T{hour:02d}:{minute:02d}:00.000Z"
        body = {
            "workspace": "seat",
            "kind": "lesson",
            "level": "explicit",
            "text": mem["text"],
            "valid_from": stamp,
            "ts": stamp,
        }
        if mem["issue"]:
            body["issue"] = mem["issue"]
            body["entities"] = [f"issue:{mem['issue']}"]
        data = json.dumps(body).encode()
        req = urllib.request.Request(
            url + "/v1/atoms",
            data=data,
            headers={
                "Authorization": f"Bearer {token}",
                "Content-Type": "application/json",
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                saved = json.loads(resp.read().decode())
        except urllib.error.HTTPError as err:
            detail = err.read().decode(errors="replace")
            raise SystemExit(f"post {mem['id']} failed: {err.code} {detail}") from err
        mem["stored_id"] = saved.get("id")


def vissue_stub(directory: Path) -> None:
    """`ljos recall` asks `vissue recall` before it reads the pack.

    The caps apply to the pack section. This stub prints one fixed line and
    exits 0, so that call is the same cost for every variant. Titles come
    from the tracker file, not from this stub.
    """
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / "vissue"
    path.write_text(
        "#!/bin/sh\n"
        "if [ \"$1\" = recall ]; then\n"
        "  printf 'tracker row\\n'\n"
        "fi\n"
        "exit 0\n",
        encoding="utf-8",
    )
    path.chmod(0o755)


def recall(
    binary: Path, issue: str, url: str, token_file: Path, home: Path, tracker: Path
) -> tuple[int, str, int]:
    stub = home / "bin"
    vissue_stub(stub)
    env = {
        "HOME": str(home),
        "PACKSET_HOME": str(home),
        "PACKSET_URL": url,
        "PACKSET_WORKSPACE": "seat",
        "PACKSET_TOKEN_FILE": str(token_file),
        "VISSUE_ROOT": str(tracker),
        "ISSUE_ROOT": str(tracker),
        "VISSUE_NO_ROUTE": "1",
        "PATH": f"{stub}:/usr/bin:/bin",
        "LANG": "C",
        "LC_ALL": "C",
    }
    start = time.perf_counter_ns()
    proc = subprocess.run(
        [str(binary), "recall", issue],
        env=env,
        cwd=tracker,
        capture_output=True,
        text=True,
        check=False,
    )
    elapsed = time.perf_counter_ns() - start
    if proc.returncode != 0:
        return proc.returncode, proc.stdout + proc.stderr, elapsed
    return 0, proc.stdout, elapsed


def parse_recall(text: str, by_mark: dict[str, dict], issue: str) -> dict:
    own_shown = []
    related_shown = []
    section = "own"
    left_out = None
    for line in text.splitlines():
        if line.strip() == "related:":
            section = "related"
            continue
        if "unverified logbook notes" in line:
            section = "notes"
            continue
        if "more memories on this issue left out" in line:
            digits = "".join(ch for ch in line if ch.isdigit())
            left_out = int(digits) if digits else None
            continue
        if "more logbook notes" in line:
            continue
        stripped = line.strip()
        if not stripped.startswith("- ["):
            continue
        mark = None
        for token, mem in by_mark.items():
            if token in line:
                mark = token
                break
        bucket = related_shown if section == "related" else own_shown
        bucket.append(mark)
    shown = own_shown + related_shown
    relevant = 0
    own_rel = 0
    rel_rel = 0
    unknown = 0
    for mark in own_shown:
        mem = by_mark.get(mark or "")
        if mem is None:
            unknown += 1
            continue
        if issue in mem["relevant_to"]:
            relevant += 1
            own_rel += 1
    for mark in related_shown:
        mem = by_mark.get(mark or "")
        if mem is None:
            unknown += 1
            continue
        if issue in mem["relevant_to"]:
            relevant += 1
            rel_rel += 1
    return {
        "shown": len(shown),
        "relevant": relevant,
        "own_shown": len(own_shown),
        "related_shown": len(related_shown),
        "own_relevant": own_rel,
        "related_relevant": rel_rel,
        "unknown": unknown,
        "left_out": left_out,
        "marks": ",".join(m or "?" for m in shown),
    }


def quartile(values: list[float]) -> tuple[float, float, float]:
    """Median and the inclusive first and third quartiles.

    statistics.quantiles method inclusive is Hyndman-Fan type 7. With six
    points the median is the mean of the 3rd and 4th once sorted.
    """
    med = statistics.median(values)
    if len(values) < 2:
        return med, med, med
    q1, _, q3 = statistics.quantiles(values, n=4, method="inclusive")
    return med, q1, q3


def fmt(value: float) -> str:
    """Shortest round-trip form, so a later reader gets the same float."""
    return repr(value)


def summarise(rows: list[dict], labelled: dict[str, int]) -> str:
    # One pass is every issue once. Micro counts sum those issues.
    passes: dict[tuple[str, int, int], dict[str, list]] = {}
    for row in rows:
        key = (row["factor"], row["level"], row["rep"])
        bucket = passes.setdefault(
            key,
            {
                "relevant": 0,
                "shown": 0,
                "labelled": 0,
                "wall_ns": 0,
                "own_rel": 0,
                "rel_rel": 0,
            },
        )
        bucket["relevant"] += row["relevant"]
        bucket["shown"] += row["shown"]
        bucket["labelled"] += labelled[row["issue"]]
        bucket["wall_ns"] += row["wall_ns"]
        bucket["own_rel"] += row["own_relevant"]
        bucket["rel_rel"] += row["related_relevant"]

    lines = []
    decided = []
    for factor in ("pool", "related", "issue"):
        levels = []
        for level in {k[1] for k in passes if k[0] == factor}:
            stats = []
            for rep in range(1, N_PASSES + 1):
                bucket = passes[(factor, level, rep)]
                precision = (
                    bucket["relevant"] / bucket["shown"] if bucket["shown"] else 0.0
                )
                recall = (
                    bucket["relevant"] / bucket["labelled"] if bucket["labelled"] else 0.0
                )
                f1 = (
                    0.0
                    if precision + recall == 0
                    else 2 * precision * recall / (precision + recall)
                )
                stats.append((precision, recall, f1, bucket["wall_ns"], bucket))
            prec = [s[0] for s in stats]
            rec = [s[1] for s in stats]
            f1s = [s[2] for s in stats]
            walls = [s[3] for s in stats]
            p_med, p_q1, p_q3 = quartile(prec)
            r_med, r_q1, r_q3 = quartile(rec)
            f_med, f_q1, f_q3 = quartile(f1s)
            w_med, w_q1, w_q3 = quartile(walls)
            # The six passes store the same hits, so the counts in pass 1
            # are the counts every pass showed. Wall time is what varies.
            one = stats[0][4]
            levels.append(
                {
                    "level": level,
                    "p_med": p_med,
                    "p_q1": p_q1,
                    "p_q3": p_q3,
                    "r_med": r_med,
                    "r_q1": r_q1,
                    "r_q3": r_q3,
                    "f_med": f_med,
                    "f_q1": f_q1,
                    "f_q3": f_q3,
                    "w_med": w_med,
                    "w_q1": w_q1,
                    "w_q3": w_q3,
                    "relevant": one["relevant"],
                    "shown": one["shown"],
                    "labelled": one["labelled"],
                    "own_rel": one["own_rel"],
                    "rel_rel": one["rel_rel"],
                    "walls": walls,
                }
            )
        levels.sort(key=lambda item: item["level"])
        lines.append(f"factor {factor}")
        for item in levels:
            lines.append(
                "  level {level} passes {n} "
                "precision median {p} quartile {pq1} {pq3} "
                "recall median {r} quartile {rq1} {rq3} "
                "f1 median {f} quartile {fq1} {fq3} "
                "wall_ns median {w} quartile {wq1} {wq3} "
                "iqr_ns {iqr} "
                "pass_relevant {rel} pass_shown {shown} pass_labelled {lab} "
                "pass_own_relevant {own} pass_related_relevant {rrel} "
                "wall_ns_passes {walls}".format(
                    level=item["level"],
                    n=N_PASSES,
                    p=fmt(item["p_med"]),
                    pq1=fmt(item["p_q1"]),
                    pq3=fmt(item["p_q3"]),
                    r=fmt(item["r_med"]),
                    rq1=fmt(item["r_q1"]),
                    rq3=fmt(item["r_q3"]),
                    f=fmt(item["f_med"]),
                    fq1=fmt(item["f_q1"]),
                    fq3=fmt(item["f_q3"]),
                    w=fmt(item["w_med"]),
                    wq1=fmt(item["w_q1"]),
                    wq3=fmt(item["w_q3"]),
                    iqr=fmt(item["w_q3"] - item["w_q1"]),
                    rel=item["relevant"],
                    shown=item["shown"],
                    lab=item["labelled"],
                    own=item["own_rel"],
                    rrel=item["rel_rel"],
                    walls=",".join(str(w) for w in item["walls"]),
                )
            )
        best_f = max(item["f_med"] for item in levels)
        tied = [item for item in levels if item["f_med"] == best_f]
        if len(tied) == 1:
            winner = tied[0]
            why = "highest median F1"
        else:
            fastest = min(item["w_med"] for item in tied)
            quick = [item for item in tied if item["w_med"] == fastest]

            def overlaps(a: dict, b: dict) -> bool:
                return a["w_q1"] <= b["w_q3"] and b["w_q1"] <= a["w_q3"]

            separated = len(quick) == 1 and not any(
                overlaps(quick[0], other) for other in tied if other is not quick[0]
            )
            if separated:
                winner = quick[0]
                why = "tied median F1, lower median wall time, quartiles do not overlap"
            else:
                today_level = TODAY[factor]
                holding = [item for item in tied if item["level"] == today_level]
                if holding:
                    winner = holding[0]
                    why = "tied median F1 and overlapping wall quartiles, today's level"
                else:
                    winner = min(tied, key=lambda item: item["level"])
                    why = "tied median F1 and overlapping wall quartiles, smaller level"
        lines.append(
            f"  decision {factor}={winner['level']} because {why} "
            f"f1 {fmt(winner['f_med'])} wall_ns {fmt(winner['w_med'])}"
        )
        decided.append((factor, winner["level"], why))
    lines.append(
        "decision "
        + " ".join(f"{factor}={level}" for factor, level, _ in decided)
    )
    for factor, level, why in decided:
        lines.append(f"decision_why {factor} {level} {why}")
    return "\n".join(lines) + "\n"


def hardware_block(commit: str) -> str:
    def run(cmd: list[str]) -> str:
        got = subprocess.run(cmd, capture_output=True, text=True, check=False)
        return got.stdout.strip()

    cpu = ""
    cores = ""
    for line in Path("/proc/cpuinfo").read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("model name") and not cpu:
            cpu = line.split(":", 1)[1].strip()
        if line.startswith("cpu cores") and not cores:
            cores = line.split(":", 1)[1].strip()
    nproc = run(["nproc"])
    uname = run(["uname", "-srm"])
    rustc = run(["rustc", "--version"])
    return (
        f"cpu: {cpu}\n"
        f"cpu_cores_field: {cores}\n"
        f"nproc: {nproc}\n"
        f"uname: {uname}\n"
        f"rustc: {rustc}\n"
        f"ljos_commit: {commit}\n"
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write-fixture", type=Path)
    parser.add_argument("--fixture", type=Path)
    parser.add_argument("--bins", type=Path)
    parser.add_argument("--packsetd", type=Path)
    parser.add_argument("--summary", type=Path)
    parser.add_argument("--draws", type=Path)
    parser.add_argument("--stdout", type=Path)
    parser.add_argument("--work", type=Path, default=Path("/tmp/recall-caps-work"))
    args = parser.parse_args()

    fixture = build_fixture()
    if args.write_fixture:
        args.write_fixture.parent.mkdir(parents=True, exist_ok=True)
        args.write_fixture.write_text(
            json.dumps(fixture, indent=2) + "\n", encoding="utf-8"
        )
        print(f"wrote {args.write_fixture} memories {len(fixture['memories'])}")
        if not args.bins:
            return

    if args.fixture:
        loaded = json.loads(args.fixture.read_text(encoding="utf-8"))
        # The committed file is what is posted. Regeneration must match it,
        # so a hand edit cannot drift from the generator.
        fresh = fixture["memories"]
        if loaded["memories"] != fresh or loaded["issues"] != fixture["issues"]:
            raise SystemExit("fixture file does not match the generator")
        fixture = loaded

    if not args.bins:
        return

    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True
    ).stdout.strip()
    status = subprocess.run(
        ["git", "status", "--porcelain"], capture_output=True, text=True, check=True
    ).stdout
    work = args.work
    if work.exists():
        raise SystemExit(f"{work} already exists")
    home = work / "home"
    tracker = work / "tracker"
    write_tracker(fixture, tracker)
    proc, url = start_packsetd(args.packsetd, home)
    try:
        token = (home / "token").read_text(encoding="utf-8").strip().splitlines()[0]
        post_memories(url, token, fixture)
        by_mark = {m["id"]: m for m in fixture["memories"]}
        labelled = {
            issue["id"]: sum(
                1 for m in fixture["memories"] if issue["id"] in m["relevant_to"]
            )
            for issue in fixture["issues"]
        }
        # Unique binaries, in the round-robin order. The baseline is one
        # binary and one measured level, then copied into each factor.
        order = []
        seen = set()
        for factor, level, name in VARIANTS:
            if name in seen:
                continue
            seen.add(name)
            binary = args.bins / name
            if not binary.is_file():
                raise SystemExit(f"missing binary {binary}")
            order.append((factor, level, name, binary))
        # Map every (factor, level) onto the binary that was measured for
        # that constant combination. The baseline rows are recorded once
        # and then attributed to pool=4, related=5 and issue=20.
        combo = {
            "ljos-p2-i20-r5": {"pool": 2, "issue": 20, "related": 5},
            "ljos-p4-i20-r5": {"pool": 4, "issue": 20, "related": 5},
            "ljos-p8-i20-r5": {"pool": 8, "issue": 20, "related": 5},
            "ljos-p4-i20-r3": {"pool": 4, "issue": 20, "related": 3},
            "ljos-p4-i20-r8": {"pool": 4, "issue": 20, "related": 8},
            "ljos-p4-i10-r5": {"pool": 4, "issue": 10, "related": 5},
            "ljos-p4-i40-r5": {"pool": 4, "issue": 40, "related": 5},
        }
        draws = []
        stdout_parts = []
        for rep in range(1, N_PASSES + 1):
            for _factor, _level, name, binary in order:
                caps = combo[name]
                for issue in fixture["issues"]:
                    code, text, wall = recall(
                        binary, issue["id"], url, home / "token", home, tracker
                    )
                    block = (
                        f"--- binary={name} rep={rep} issue={issue['id']} "
                        f"wall_ns={wall} exit={code}\n{text}\n--- end\n"
                    )
                    stdout_parts.append(block)
                    if code != 0:
                        if args.stdout:
                            args.stdout.parent.mkdir(parents=True, exist_ok=True)
                            args.stdout.write_text("".join(stdout_parts), encoding="utf-8")
                        raise SystemExit(
                            f"recall failed {name} {issue['id']} rep {rep} exit {code}"
                        )
                    parsed = parse_recall(text, by_mark, issue["id"])
                    if parsed["unknown"]:
                        raise SystemExit(
                            f"unmatched hit line in {name} {issue['id']}: {text}"
                        )
                    base = {
                        "binary": name,
                        "rep": rep,
                        "issue": issue["id"],
                        "wall_ns": wall,
                        "pool": caps["pool"],
                        "issue_cap": caps["issue"],
                        "related": caps["related"],
                        **parsed,
                    }
                    # One measured recall feeds each factor whose other two
                    # caps are today's, which is every binary here. The
                    # baseline feeds all three factors.
                    factors = []
                    if caps["issue"] == TODAY["issue"] and caps["related"] == TODAY["related"]:
                        factors.append(("pool", caps["pool"]))
                    if caps["pool"] == TODAY["pool"] and caps["issue"] == TODAY["issue"]:
                        factors.append(("related", caps["related"]))
                    if caps["pool"] == TODAY["pool"] and caps["related"] == TODAY["related"]:
                        factors.append(("issue", caps["issue"]))
                    for factor, level in factors:
                        row = dict(base)
                        row["factor"] = factor
                        row["level"] = level
                        draws.append(row)

        header = (
            "factor\tlevel\trep\tissue\tbinary\twall_ns\tshown\trelevant\t"
            "own_shown\trelated_shown\town_relevant\trelated_relevant\t"
            "left_out\tmarks\n"
        )
        body = "".join(
            "\t".join(
                [
                    row["factor"],
                    str(row["level"]),
                    str(row["rep"]),
                    row["issue"],
                    row["binary"],
                    str(row["wall_ns"]),
                    str(row["shown"]),
                    str(row["relevant"]),
                    str(row["own_shown"]),
                    str(row["related_shown"]),
                    str(row["own_relevant"]),
                    str(row["related_relevant"]),
                    "" if row["left_out"] is None else str(row["left_out"]),
                    row["marks"],
                ]
            )
            + "\n"
            for row in draws
        )
        if args.draws:
            args.draws.parent.mkdir(parents=True, exist_ok=True)
            args.draws.write_text(header + body, encoding="utf-8")
        if args.stdout:
            args.stdout.parent.mkdir(parents=True, exist_ok=True)
            args.stdout.write_text("".join(stdout_parts), encoding="utf-8")
        summary = summarise(draws, labelled)
        versions = subprocess.run(
            [str(args.packsetd), "--version"],
            capture_output=True,
            text=True,
            check=False,
        )
        ljos_ver = subprocess.run(
            [str(args.bins / "ljos-p4-i20-r5"), "--version"],
            capture_output=True,
            text=True,
            check=False,
        )
        text = (
            hardware_block(commit)
            + f"git_status:\n{status if status else '(clean)\n'}"
            + f"packsetd: {versions.stdout.strip()} {versions.stderr.strip()}\n"
            + f"ljos_base: {ljos_ver.stdout.strip()} {ljos_ver.stderr.strip()}\n"
            + f"packsetd_bin: {args.packsetd}\n"
            + f"passes: {N_PASSES}\n"
            + "order: variants round-robin, one pass recalls every issue in fixture order\n"
            + "quartiles: statistics.quantiles method inclusive (Hyndman-Fan type 7); "
            + "median is statistics.median\n"
            + "wall: perf_counter_ns around one ljos recall ISSUE, including process start\n"
            + "vissue: a stub on PATH answers `vissue recall` with one fixed line; "
            + "the issue title is read from Software/ufuv/issues.org\n"
            + f"fixture_memories: {len(fixture['memories'])}\n"
            + f"labelled: {json.dumps(labelled, sort_keys=True)}\n"
            + "commands:\n"
            + "  scripts/recall-caps-build.sh /tmp/recall-bins\n"
            + "  scripts/recall-caps.py --fixture scripts/fixtures/recall-caps.json "
            + "--bins /tmp/recall-bins --packsetd /tmp/recall-bins/packsetd "
            + "--summary measurements/recall-caps.txt "
            + "--draws measurements/recall-caps-draws.tsv "
            + "--stdout measurements/recall-caps-stdout.txt\n"
            + summary
        )
        if args.summary:
            args.summary.parent.mkdir(parents=True, exist_ok=True)
            args.summary.write_text(text, encoding="utf-8")
        sys.stdout.write(text)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


if __name__ == "__main__":
    main()
