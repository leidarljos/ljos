#!/usr/bin/env python3
"""Break long lines on a space before they reach the right edge.

The recorded shell's output is piped here. Typing still goes to the
terminal, so the shell stays the person's shell. Run with --self-test
to check the break.
"""

import os
import re
import select
import struct
import sys
import termios

ANSI = re.compile(r"\x1b(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])")


def char_cols(ch):
    o = ord(ch)
    if o < 32:
        return 0
    # A wide glyph is rare here. Count it as two so the edge stays clear.
    if o >= 0x1100:
        return 2
    return 1


def expand_tabs(text, start=0):
    out = []
    col = start
    i = 0
    while i < len(text):
        m = ANSI.match(text, i)
        if m:
            out.append(m.group(0))
            i = m.end()
            continue
        ch = text[i]
        i += 1
        if ch == "\t":
            n = 8 - (col % 8)
            out.append(" " * n)
            col += n
            continue
        out.append(ch)
        col += char_cols(ch)
    return "".join(out)


def display_width(text):
    col = 0
    i = 0
    while i < len(text):
        m = ANSI.match(text, i)
        if m:
            i = m.end()
            continue
        ch = text[i]
        i += 1
        if ch == "\t":
            col += 8 - (col % 8)
        else:
            col += char_cols(ch)
    return col


def pieces(text):
    """ANSI runs, spaces, and single characters, each with a column count."""
    i = 0
    while i < len(text):
        m = ANSI.match(text, i)
        if m:
            yield m.group(0), 0
            i = m.end()
            continue
        ch = text[i]
        i += 1
        if ch == " ":
            yield ch, 1
        else:
            yield ch, char_cols(ch)


def wrap_line(text, width, start_col=0):
    """Break one physical line on spaces. A word longer than width stays whole."""
    if width < 8:
        width = 8
    text = expand_tabs(text, start_col)
    lines = []
    buf = []
    col = start_col
    brk = None

    def emit_break():
        nonlocal buf, col, brk
        line = "".join(buf[:brk]).rstrip(" ")
        lines.append(line)
        buf = buf[brk:]
        while buf and buf[0] == " ":
            buf.pop(0)
        col = display_width("".join(buf))
        brk = None

    for piece, w in pieces(text):
        if piece == " ":
            buf.append(piece)
            col += w
            brk = len(buf)
            if col > width and brk > 1:
                # The space itself ran past the edge. Break before it.
                brk -= 1
                emit_break()
            continue
        if w and col + w > width and brk is not None and brk < len(buf) + 1 and col > 0:
            # This glyph would pass the edge, and a space sits earlier on the line.
            if brk == len(buf) and buf and buf[-1] == " ":
                emit_break()
            elif brk < len(buf):
                emit_break()
        if w and col + w > width and brk is not None and "".join(buf).strip() and col >= width:
            emit_break()
        buf.append(piece)
        col += w
        if w and col > width and brk is not None and brk < len(buf):
            emit_break()
    lines.append("".join(buf))
    return lines


def words_of(text):
    plain = ANSI.sub("", expand_tabs(text))
    return plain.split()


class Stream:
    """Reveal text only up to a point that will not be split later.

    With twelve columns spare, the whole fragment is shown, so typing
    stays even. Near the edge, only a finished word (one a space has
    closed) is shown. A newline breaks the line on spaces.
    """

    def __init__(self, width, follow=False):
        self.width = width
        self.follow = follow
        self.buf = ""
        self.shown = 0

    def feed(self, text):
        self.buf += text
        return self._drain(final=False)

    def flush_soft(self):
        return self._drain(final=False)

    def flush_end(self):
        return self._drain(final=True)

    def _ansi_open(self):
        i = self.buf.rfind("\x1b")
        if i < 0 or i < self.shown:
            return False
        return ANSI.match(self.buf, i) is None

    def _drain(self, final):
        out = []
        # The picture's window is applied after the shell starts.
        if self.follow:
            self.width = filter_width()
        while True:
            cut = None
            for i in range(self.shown, len(self.buf)):
                if self.buf[i] in "\n\r":
                    cut = i
                    break
            if cut is None:
                break
            line = self.buf[:cut]
            sep = self.buf[cut]
            self.buf = self.buf[cut + 1 :]
            out.append(self._close(line))
            out.append(sep)
            self.shown = 0
        if self.buf and not self._ansi_open():
            if final:
                out.append(self._finish())
            else:
                out.append(self._reveal(final))
        return "".join(out)

    def _finish(self):
        """End of the stream. Break what is left, including a partial word."""
        text = self.buf
        shown = self.shown
        self.buf = ""
        self.shown = 0
        if not text:
            return ""
        wrapped = wrap_line(text, self.width)
        if shown == 0:
            return "\n".join(wrapped)
        first = wrapped[0]
        if shown and (text[:shown] == first or first.startswith(text[:shown])):
            rest = first[len(text[:shown]) :]
            if len(wrapped) > 1:
                rest += "\n" + "\n".join(wrapped[1:])
            return rest
        tail = text[shown:].lstrip(" ")
        if not tail:
            return ""
        return "\n" + "\n".join(wrap_line(tail, self.width))

    def _close(self, line):
        wrapped = wrap_line(line, self.width)
        shown = line[: self.shown]
        if self.shown == 0:
            return "\n".join(wrapped)
        first = wrapped[0]
        if shown == first or first.startswith(shown):
            rest = first[len(shown) :]
            if len(wrapped) > 1:
                rest += "\n" + "\n".join(wrapped[1:])
            return rest
        # The cursor is already past the break. Start the next row.
        tail = line[self.shown :].lstrip(" ")
        if not tail:
            return ""
        return "\n" + "\n".join(wrap_line(tail, self.width))

    def _reveal(self, final):
        text = self.buf
        if not text or self.shown >= len(text):
            return ""
        # A clear or a cursor home is sent at once and the row restarts.
        matched = ANSI.match(text, self.shown) if text[self.shown] == "\x1b" else None
        if matched and ("H" in matched.group(0)[-1:] or matched.group(0).endswith("J")):
            chunk = matched.group(0)
            self.buf = text[matched.end() :]
            self.shown = 0
            return chunk
        # Show a partial word only while the whole line still has room for
        # the longest token we print (a path). Closer than that, a word is
        # shown when its following space arrives, so it can still move.
        if display_width(text) + 72 <= self.width:
            chunk = text[self.shown :]
            self.shown = len(text)
            return chunk
        # Near the edge, show only through the last space that still fits.
        last = self.shown
        col = display_width(text[: self.shown])
        i = self.shown
        while i < len(text):
            seq = ANSI.match(text, i)
            if seq:
                i = seq.end()
                continue
            ch = text[i]
            w = (8 - (col % 8) or 8) if ch == "\t" else char_cols(ch)
            if col + w > self.width:
                break
            col += w
            i += 1
            if ch == " ":
                last = i
        if last <= self.shown:
            return ""
        chunk = text[self.shown : last]
        self.shown = last
        return chunk


def fcntl_winsize(fd):
    import fcntl

    return struct.unpack("HHHH", fcntl.ioctl(fd, termios.TIOCGWINSZ, b"\0" * 8))


def self_test():
    deny = (
        "deny: rm-rf-outside-tmp: a recursive delete outside /tmp cannot be undone. "
        "Leave it, or move it to the trash (`trash-put` or `gio trash`) so the person "
        "can restore it, or ask the person to delete it. Moving it under /tmp to delete "
        "it there is the same delete (rule: rm-rf-outside-tmp)"
    )
    cite = (
        "push cite: does not stand for ubuntu: no seat voted on demo-1jb8, so it decides "
        "nothing for a push; a seat votes, or the person approves this push; left out: "
        "inky is not a standing seat, and no slot on demo-1jb8 names it; ljosbot is not "
        "a standing seat, and no slot on demo-1jb8 names it"
    )
    protected = (
        "protected push: demo-1jb8 does not clear a push onto `develop`, `main`: "
        + cite.split(": ", 1)[-1]
    )
    doctor = (
        "info\tsitting gate\tnot active here: no agent runner is above this shell, "
        "so the person's git is not gated; under one, a commit, merge, rebase, "
        "cherry-pick or push waits for a live sitting"
    )
    note = (
        "Note: /home/ubuntu/ljos-demo-root/scene1/.local/bin is not on PATH. "
        "Add this line to your shell profile, and run it here:"
    )
    samples = [
        deny,
        cite,
        protected,
        doctor,
        note,
        "  unanimous: ship (2 of 2); `vissue consensus demo-e7k3` weighs it",
    ]
    failed = False
    for width in range(90, 111):
        for src in samples:
            lines = wrap_line(src, width)
            if words_of(src) != words_of("\n".join(lines)):
                print("words moved", width)
                failed = True
            for line in lines:
                w = display_width(line)
                if w > width and " " in line.strip():
                    print("over", width, w, line)
                    failed = True
                if w > width:
                    # A single token longer than the window.
                    print("long token", width, w, line)
                    failed = True
    def screen(emitted):
        rows = [""]
        for ch in emitted:
            if ch == "\n":
                rows.append("")
            elif ch != "\r":
                rows[-1] += ch
        return rows

    def play(src, width, step):
        st = Stream(width)
        emitted = ""
        if step == 1:
            for ch in src:
                emitted += st.feed(ch)
                emitted += st.flush_soft()
        else:
            emitted += st.feed(src)
            emitted += st.flush_soft()
        rows = screen(emitted)
        for row in rows:
            if display_width(row) > width and " " in row.strip():
                print("row too wide", width, display_width(row), row)
                return False
        if words_of(src.replace("\n", " ")) != words_of("\n".join(rows)):
            print("words", width, step)
            print(words_of(src))
            print(words_of("\n".join(rows)))
            return False
        return True

    sentence = deny + "\n" + cite + "\n"
    for width in (90, 100, 102, 104):
        for step in (1, 80):
            if not play(sentence, width, step):
                failed = True
        if not play("> ljos policy -- rm -rf $HOME/notes\n" + deny + "\n", width, 1):
            failed = True
    if failed:
        raise SystemExit(1)
    print("self-test ok")


def _live_cols():
    """Columns the terminal has right now. The pty size changes after startup."""
    for opener in (
        lambda: os.open("/dev/tty", os.O_RDONLY),
        lambda: sys.stdout.fileno(),
    ):
        fd = None
        close = False
        try:
            fd = opener()
            close = fd != sys.stdout.fileno()
            live = fcntl_winsize(fd)[1]
        except OSError:
            live = 0
        finally:
            if close and fd is not None and fd != sys.stdout.fileno():
                os.close(fd)
        if 40 <= live <= 200:
            return live
    raw = os.environ.get("COLUMNS", "")
    if raw.isdigit() and 40 <= int(raw) <= 200:
        return int(raw)
    return 104


def filter_width():
    """Two columns inside the live window, so the terminal never wraps."""
    cols = _live_cols()
    ceiling = os.environ.get("LJOS_DEMO_COLS", "")
    if ceiling.isdigit():
        cols = min(cols, int(ceiling))
    return max(8, cols - 2)


def filter_stdin():
    width = filter_width()
    stream = Stream(width, follow=True)
    decoder = __import__("codecs").getincrementaldecoder("utf-8")("surrogateescape")
    while True:
        try:
            ready, _, _ = select.select([sys.stdin], [], [], 0.02)
        except InterruptedError:
            continue
        if not ready:
            soft = stream.flush_soft()
            if soft:
                sys.stdout.write(soft)
                sys.stdout.flush()
            continue
        data = os.read(sys.stdin.fileno(), 65536)
        if not data:
            soft = stream.flush_end()
            if soft:
                sys.stdout.write(soft)
                sys.stdout.flush()
            break
        out = stream.feed(decoder.decode(data))
        if out:
            sys.stdout.write(out)
            sys.stdout.flush()


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        self_test()
    else:
        filter_stdin()
