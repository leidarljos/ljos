//! A program run on a fresh pseudo-terminal, in a process shape a test
//! names, so the person-at-the-terminal check sees what a terminal gives.

/// Runs `argv` on a fresh pty in the shape `mode` names: `terminal`, a
/// parent renamed (`renamed:NAME`), or below a process left by a double
/// fork (`double-fork`) or reparented without one (`reparented`). `adopted`
/// is a double fork whose survivor starts a session again once init holds
/// it and takes a terminal's name. `python` and `make` put that program
/// between the terminal and `argv`. Prints what the program printed and
/// exits with its status.
const SHAPE_PY: &str = r#"import ctypes, os, pty, shlex, sys, time
mode, argv = sys.argv[1], sys.argv[2:]

def named(name):
    libc = ctypes.CDLL(None)
    libc.prctl.argtypes = [ctypes.c_int, ctypes.c_char_p]
    libc.prctl.restype = ctypes.c_int
    if libc.prctl(15, name) != 0:
        raise SystemExit(91)

def run(cmd=None):
    cmd = cmd or argv
    pid, fd = pty.fork()
    if pid == 0:
        os.execv(cmd[0], cmd)
    out = []
    while True:
        try:
            data = os.read(fd, 4096)
        except OSError:
            break
        if not data:
            break
        out.append(data)
    _, status = os.waitpid(pid, 0)
    return b"".join(out), os.waitstatus_to_exitcode(status)

def orphan(setsid, again=False):
    r, w = os.pipe()
    if os.fork() == 0:
        os.close(r)
        if setsid:
            os.setsid()
        parent = os.getpid()
        if os.fork() != 0:
            os._exit(0)
        while os.getppid() == parent:
            time.sleep(0.01)
        if again:
            os.setsid()
            named(b"gnome-terminal-")
        said, code = run()
        os.write(w, bytes([code & 0xff]) + said)
        os._exit(0)
    os.close(w)
    os.wait()
    got = b""
    while True:
        chunk = os.read(r, 65536)
        if not chunk:
            break
        got += chunk
    return got[1:], (got[0] if got else 99)

if mode.startswith("renamed:"):
    named(mode.split(":", 1)[1].encode())
    said, code = run()
elif mode == "double-fork":
    said, code = orphan(True)
elif mode == "reparented":
    said, code = orphan(False)
elif mode == "adopted":
    said, code = orphan(True, again=True)
elif mode == "python":
    named(b"gnome-terminal-")
    said, code = run([sys.executable, "-c",
        "import subprocess, sys; sys.exit(subprocess.call(sys.argv[1:]))"] + argv)
elif mode == "make":
    named(b"gnome-terminal-")
    said, code = run(["/usr/bin/make", "-s", "-f", "/dev/null",
        "--eval", "run: ; @" + shlex.join(argv), "run"])
else:
    # The person at a terminal has a terminal emulator holding the pty.
    # python's own name is not one, so the default shape takes one.
    named(b"gnome-terminal-")
    said, code = run()
sys.stdout.buffer.write(said)
raise SystemExit(code)
"#;

/// The interpreter that runs the shape.
pub const PYTHON: &str = "/usr/bin/python3";

/// The arguments [`PYTHON`] takes to run `bin ARGS` in the shape `mode`.
#[must_use]
pub fn shape<'a>(mode: &'a str, bin: &'a str, args: &[&'a str]) -> Vec<&'a str> {
    let mut all = vec!["-c", SHAPE_PY, mode, bin];
    all.extend_from_slice(args);
    all
}
