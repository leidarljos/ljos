#!/usr/bin/env python3
"""Resident memory of one idle ljos-mcp, read from /proc/PID/status.

    scripts/mcp-rss.py [--bin target/release/ljos-mcp] [--idle 140]

The server runs in a throwaway home with a tracker and one seat. It is
initialised, sampled, given one tools/call of each core tool, then left
alone for --idle seconds and sampled once a second. PACKSET_URL and
PACKSET_TOKEN pass through when set, so the pack calls reach a running
packsetd; unset, they fail fast and the rest of the call still runs.

Printed: VmRSS, VmHWM (peak), RssAnon and Threads after initialize,
right after the burst, and at the end of the idle wait, plus the first
second RSS fell under 30 MiB. Linux only; CPU only.
"""

import argparse
import json
import os
import select
import subprocess
import sys
import tempfile
import time

CALLS = [
    ("ljos_recall", {"issue": "probe-abcd"}),
    ("ljos_search", {"query": "release review set"}),
    ("ljos_note", {"text": "Probe note for the memory measurement."}),
    ("ljos_remember", {"text": "Pin the review set before a release."}),
    ("ljos_prefer", {"text": "Run cargo test before a push."}),
    ("ljos_guard_check", {"command": "rm -rf /tmp/probe"}),
    ("ljos_rule", {"text": "Run cargo test before a push."}),
    ("ljos_options", {}),
    ("ljos_start", {}),
    ("ljos_sitting", {}),
    ("ljos_file", {"title": "probe child"}),
    ("ljos_claim", {"issue": "probe-abcd"}),
    ("ljos_release", {"issue": "probe-abcd"}),
    ("ljos_inbox", {}),
    ("ljos_send", {"to": "probe", "text": "hello"}),
    ("ljos_vote", {"issue": "probe-abcd", "choice": "accept"}),
    ("ljos_consensus", {"issue": "probe-abcd"}),
    ("ljos_doctor", {}),
]

MIB = 1024


def status(pid):
    out = {}
    try:
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                key, _, value = line.partition(":")
                if key in ("VmRSS", "VmHWM", "RssAnon"):
                    out[key] = int(value.split()[0])
                elif key == "Threads":
                    out[key] = int(value)
    except FileNotFoundError:
        return None
    return out


def show(label, s):
    if s is None:
        print(f"{label:>14}: process gone")
        return
    print(
        f"{label:>14}: rss {s['VmRSS'] / MIB:7.1f} MiB  peak {s['VmHWM'] / MIB:7.1f} MiB"
        f"  anon {s['RssAnon'] / MIB:7.1f} MiB  threads {s['Threads']}"
    )


def read_answer(proc, want, deadline):
    buf = b""
    while time.time() < deadline:
        ready, _, _ = select.select([proc.stdout], [], [], 0.5)
        if not ready:
            continue
        chunk = os.read(proc.stdout.fileno(), 65536)
        if not chunk:
            return None
        buf += chunk
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            try:
                msg = json.loads(line)
            except ValueError:
                continue
            if msg.get("id") == want:
                return msg
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default="target/release/ljos-mcp")
    ap.add_argument("--idle", type=int, default=140)
    args = ap.parse_args()

    home = tempfile.mkdtemp(prefix="ljos-rss-")
    tracker = os.path.join(home, "tracker")
    os.makedirs(tracker)
    with open(os.path.join(tracker, "vissue.toml"), "w") as f:
        f.write('seats = ["probe"]\n')
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": home,
        "XDG_CONFIG_HOME": os.path.join(home, ".config"),
        "XDG_STATE_HOME": os.path.join(home, "state"),
        "XDG_RUNTIME_DIR": home,
        "VISSUE_ROOT": tracker,
        "LJOS_SEAT": "probe",
        "PACKSET_WORKSPACE": "probe",
        "PACKSET_URL": os.environ.get("PACKSET_URL", "http://127.0.0.1:9"),
    }
    for key in ("PACKSET_TOKEN", "LJOS_MCP_IDLE_SECS"):
        if key in os.environ:
            env[key] = os.environ[key]

    proc = subprocess.Popen(
        [args.bin],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        env=env,
        cwd=home,
    )

    def send(msg):
        proc.stdin.write((json.dumps(msg) + "\n").encode())
        proc.stdin.flush()

    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "probe", "version": "0"}}})
    if read_answer(proc, 1, time.time() + 30) is None:
        sys.exit("no answer to initialize")
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    time.sleep(1)
    show("initialized", status(proc.pid))

    for i, (name, arguments) in enumerate(CALLS, start=10):
        send({"jsonrpc": "2.0", "id": i, "method": "tools/call",
              "params": {"name": name, "arguments": arguments}})
        if read_answer(proc, i, time.time() + 60) is None:
            print(f"no answer to {name}", file=sys.stderr)
    burst = status(proc.pid)
    show("after burst", burst)

    start = time.time()
    under = None
    last = burst
    while time.time() - start < args.idle:
        time.sleep(1)
        last = status(proc.pid)
        if last is None:
            break
        if under is None and last["VmRSS"] < 30 * MIB:
            under = time.time() - start
    show(f"idle {args.idle}s", last)
    print(f"{'under 30 MiB':>14}: " + (f"after {under:.0f}s" if under is not None else "never"))

    proc.stdin.close()
    t0 = time.time()
    try:
        code = proc.wait(timeout=10)
        print(f"{'stdin EOF':>14}: exit {code} after {time.time() - t0:.2f}s")
    except subprocess.TimeoutExpired:
        print(f"{'stdin EOF':>14}: still running after 10s")
        proc.kill()


if __name__ == "__main__":
    main()
